use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::commands::merge::sync_head_after_push;
use crate::commands::merge_rebase::{
    fetch_remote_for_descendant_rebase, rebase_descendant_onto_remote_trunk_with_provenance,
};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::{GitRepo, RebaseResult};
use crate::github::pr::{MergeMethod, PrMergeStatus};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

/// Information about a branch in the land scope
#[derive(Debug, Clone)]
struct LandBranchInfo {
    branch: String,
    pr_number: u64,
    status: LandStatus,
}

/// Information about branches above the current merge scope that still need to be rebased
#[derive(Debug, Clone)]
struct RemainingBranchInfo {
    branch: String,
    pr_number: Option<u64>,
}

/// Result of the merge scope calculation
struct MergeWhenReadyScope {
    /// Branches to merge (bottom to current, or full stack with --all)
    to_merge: Vec<String>,
    /// Descendants above current not included in merge (unless --all)
    remaining: Vec<String>,
    /// Trunk branch name
    trunk: String,
}

/// Status of a branch during the land process
#[derive(Debug, Clone, PartialEq)]
enum LandStatus {
    Pending,
    WaitingForCi,
    Merging,
    Merged,
    Failed(String),
}

impl LandStatus {
    fn symbol(&self) -> String {
        match self {
            LandStatus::Pending => "○".dimmed().to_string(),
            LandStatus::WaitingForCi => "⏳".yellow().to_string(),
            LandStatus::Merging => "⏳".cyan().to_string(),
            LandStatus::Merged => "✓".green().to_string(),
            LandStatus::Failed(_) => "✗".red().to_string(),
        }
    }

    fn label(&self) -> String {
        match self {
            LandStatus::Pending => "pending".dimmed().to_string(),
            LandStatus::WaitingForCi => "waiting for CI...".yellow().to_string(),
            LandStatus::Merging => "merging...".cyan().to_string(),
            LandStatus::Merged => "merged".green().to_string(),
            LandStatus::Failed(reason) => format!("failed: {}", reason).red().to_string(),
        }
    }
}

/// Result of waiting for a PR to be ready
enum WaitResult {
    Ready,
    Failed(String),
    Timeout,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all: bool,
    method: MergeMethod,
    timeout_mins: u64,
    interval_secs: u64,
    no_delete: bool,
    no_sync: bool,
    yes: bool,
    quiet: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    // Validate: not on trunk
    if current == stack.trunk {
        if !quiet {
            println!(
                "{}",
                "You are on trunk. Checkout a branch in a stack to merge.".yellow()
            );
        }
        return Ok(());
    }

    // Validate: branch is tracked
    if !stack.branches.contains_key(&current) {
        if !quiet {
            println!(
                "{}",
                format!(
                    "Branch '{}' is not tracked. Run 'stax branch track' first.",
                    current
                )
                .yellow()
            );
        }
        return Ok(());
    }

    // Calculate scope: bottom->current are merged; descendants are either merged (--all)
    // or rebased after merges so their PR bases stay valid.
    let scope = calculate_merge_scope(&stack, &current, all);

    // Build branch info list
    let mut branches: Vec<LandBranchInfo> = Vec::new();

    // Set up forge client
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let client = ForgeClient::new(&remote_info).context(
        "Failed to connect to the configured forge. Check your token and remote configuration.",
    )?;

    // Resolve PR numbers for merge scope and optional PR numbers for remaining scope.
    let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching PR info...");

    for branch_name in &scope.to_merge {
        let branch_info = stack.branches.get(branch_name);
        let mut pr_number = branch_info.and_then(|b| b.pr_number);

        // If no PR in metadata, try looking it up on the forge
        if pr_number.is_none() {
            if let Ok(Some(pr_info)) = rt.block_on(async { client.find_pr(branch_name).await }) {
                pr_number = Some(pr_info.number);
            }
        }

        match pr_number {
            Some(num) => branches.push(LandBranchInfo {
                branch: branch_name.clone(),
                pr_number: num,
                status: LandStatus::Pending,
            }),
            None => {
                LiveTimer::maybe_finish_err(fetch_timer, "missing PR");
                anyhow::bail!(
                    "Branch '{}' has no PR. Run 'stax submit' first to create PRs.",
                    branch_name
                );
            }
        }
    }

    let mut remaining_branches: Vec<RemainingBranchInfo> = Vec::new();
    for branch_name in &scope.remaining {
        let branch_info = stack.branches.get(branch_name);
        let mut pr_number = branch_info.and_then(|b| b.pr_number);

        if pr_number.is_none() {
            if let Ok(Some(pr_info)) = rt.block_on(async { client.find_pr(branch_name).await }) {
                pr_number = Some(pr_info.number);
            }
        }

        remaining_branches.push(RemainingBranchInfo {
            branch: branch_name.clone(),
            pr_number,
        });
    }

    LiveTimer::maybe_finish_ok(fetch_timer, "done");

    if branches.is_empty() {
        if !quiet {
            println!("{}", "No branches to merge.".yellow());
        }
        return Ok(());
    }

    // Show preview
    if !quiet {
        println!();
        print_land_preview(&branches, &scope.trunk, &method);
    }

    // Confirm
    if !yes {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Proceed with merge --when-ready?")
            .default(false)
            .interact()?;

        if !confirm {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    // Begin transaction
    if !quiet {
        println!();
        print_header("Merge When Ready");
    }

    let mut branch_names: Vec<String> = branches.iter().map(|b| b.branch.clone()).collect();
    branch_names.extend(remaining_branches.iter().map(|b| b.branch.clone()));
    let mut tx = Transaction::begin(OpKind::MergeWhenReady, &repo, quiet)?;
    tx.plan_branches(&repo, &branch_names)?;
    let summary = PlanSummary {
        branches_to_rebase: branches.len(),
        branches_to_push: 0,
        description: vec![format!(
            "Merge {} {} bottom-up via {}",
            branches.len(),
            if branches.len() == 1 { "PR" } else { "PRs" },
            method.as_str()
        )],
    };
    tx::print_plan(tx.kind(), &summary, quiet);
    tx.set_plan_summary(summary);
    tx.snapshot()?;

    let timeout = Duration::from_secs(timeout_mins * 60);
    let poll_interval = Duration::from_secs(interval_secs);
    let total = branches.len();
    let mut merged_prs: Vec<(String, u64)> = Vec::new();
    let mut failed_pr: Option<(String, u64, String)> = None;

    for idx in 0..total {
        let pr_number = branches[idx].pr_number;
        let branch_name = branches[idx].branch.clone();
        let next_branch = branches.get(idx + 1).cloned();

        if !quiet {
            println!();
            println!(
                "[{}/{}] {} (#{})",
                (idx + 1).to_string().cyan(),
                total,
                branch_name.bold(),
                pr_number
            );
        }

        // Check if already merged
        let is_merged = rt.block_on(async { client.is_pr_merged(pr_number).await })?;
        if is_merged {
            branches[idx].status = LandStatus::Merged;
            if !quiet {
                println!("      {} Already merged", "✓".green());
            }
            merged_prs.push((branch_name.clone(), pr_number));
        } else {
            // Wait for CI and approval
            branches[idx].status = LandStatus::WaitingForCi;
            if !quiet {
                print_dashboard(&branches, quiet);
            }

            match wait_for_pr_ready(&rt, &client, pr_number, timeout, poll_interval, quiet)? {
                WaitResult::Ready => {}
                WaitResult::Failed(reason) => {
                    branches[idx].status = LandStatus::Failed(reason.clone());
                    failed_pr = Some((branch_name, pr_number, reason));
                    break;
                }
                WaitResult::Timeout => {
                    let reason = "Timeout waiting for CI".to_string();
                    branches[idx].status = LandStatus::Failed(reason.clone());
                    failed_pr = Some((branch_name, pr_number, reason));
                    break;
                }
            }

            // Merge the PR
            branches[idx].status = LandStatus::Merging;
            let merge_timer =
                LiveTimer::maybe_new(!quiet, &format!("Merging ({})...", method.as_str()));

            match rt.block_on(async { client.merge_pr(pr_number, method, None, None).await }) {
                Ok(()) => {
                    LiveTimer::maybe_finish_ok(merge_timer, "done");
                    branches[idx].status = LandStatus::Merged;
                    merged_prs.push((branch_name.clone(), pr_number));

                    // Record CI history
                    record_ci_history_for_branch(&repo, &rt, &client, &stack, &branch_name);
                }
                Err(e) => {
                    LiveTimer::maybe_finish_err(merge_timer, "failed");
                    let reason = e.to_string();
                    branches[idx].status = LandStatus::Failed(reason.clone());
                    failed_pr = Some((branch_name, pr_number, reason));
                    break;
                }
            }

            // Retarget next PR to trunk after successful merge
            if let Some(next_branch) = &next_branch {
                let update_base_timer = LiveTimer::maybe_new(
                    !quiet,
                    &format!(
                        "Retargeting #{} to {}...",
                        next_branch.pr_number, scope.trunk
                    ),
                );

                match rt.block_on(async {
                    client
                        .update_pr_base(next_branch.pr_number, &scope.trunk)
                        .await
                }) {
                    Ok(()) => {
                        LiveTimer::maybe_finish_ok(update_base_timer, "done");
                    }
                    Err(e) => {
                        LiveTimer::maybe_finish_err(update_base_timer, "failed");
                        let reason = format!(
                            "Failed to retarget dependent PR #{}: {}",
                            next_branch.pr_number, e
                        );
                        branches[idx].status = LandStatus::Failed(reason.clone());
                        failed_pr = Some((branch_name, pr_number, reason));
                        break;
                    }
                }
            }
        }

        // If there are more PRs, rebase the next one onto trunk.
        if let Some(next_branch) = next_branch {
            let next_branch_name = next_branch.branch.clone();
            let next_pr = next_branch.pr_number;

            // Fetch latest from remote
            let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching latest...");
            let fetch_ok = fetch_remote_for_descendant_rebase(&repo, &remote_info.name)?;
            if !fetch_ok {
                LiveTimer::maybe_finish_warn(fetch_timer, "warning");
            } else {
                LiveTimer::maybe_finish_ok(fetch_timer, "done");
            }

            // Rebase next branch onto trunk
            let rebase_timer = LiveTimer::maybe_new(
                !quiet,
                &format!("Rebasing {} onto {}...", next_branch_name, scope.trunk),
            );

            let rebase_result = rebase_descendant_onto_remote_trunk_with_provenance(
                &repo,
                &next_branch_name,
                &scope.trunk,
                &remote_info.name,
            )?;
            match rebase_result {
                RebaseResult::Success => {
                    LiveTimer::maybe_finish_ok(rebase_timer, "done");
                }
                RebaseResult::Conflict => {
                    let abort_dir = repo
                        .branch_worktree_path(&next_branch_name)?
                        .unwrap_or(repo.workdir()?.to_path_buf());
                    let _ = Command::new("git")
                        .args(["rebase", "--abort"])
                        .current_dir(&abort_dir)
                        .output();

                    LiveTimer::maybe_finish_err(rebase_timer, "conflict");
                    let reason = "Rebase conflict".to_string();
                    branches[idx + 1].status = LandStatus::Failed(reason.clone());
                    failed_pr = Some((next_branch_name, next_pr, reason));
                    break;
                }
            }

            let push_timer =
                LiveTimer::maybe_new(!quiet, &format!("Pushing {}...", next_branch_name));

            let push_status = Command::new("git")
                .args([
                    "push",
                    "--force-with-lease",
                    &remote_info.name,
                    &next_branch_name,
                ])
                .current_dir(repo.workdir()?)
                .output()
                .context("Failed to push")?;

            if !push_status.status.success() {
                LiveTimer::maybe_finish_err(push_timer, "failed");
                let reason = "Failed to push rebased branch".to_string();
                branches[idx + 1].status = LandStatus::Failed(reason.clone());
                failed_pr = Some((next_branch_name, next_pr, reason));
                break;
            }

            LiveTimer::maybe_finish_ok(push_timer, "done");
            sync_head_after_push(&rt, &client, next_pr, &repo, &next_branch_name);
        }
    }

    // Rebase branches above the merge scope to keep descendant PRs open and correctly based.
    if !merged_prs.is_empty() && !remaining_branches.is_empty() && failed_pr.is_none() {
        if !quiet {
            println!();
            println!("{}", "Rebasing remaining stack branches...".dimmed());
        }

        for remaining in &remaining_branches {
            let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching latest...");
            let fetch_ok = fetch_remote_for_descendant_rebase(&repo, &remote_info.name)?;
            if !fetch_ok {
                LiveTimer::maybe_finish_warn(fetch_timer, "warning");
            } else {
                LiveTimer::maybe_finish_ok(fetch_timer, "done");
            }

            let remaining_timer =
                LiveTimer::maybe_new(!quiet, &format!("Rebasing {}...", remaining.branch));

            let rebase_result = rebase_descendant_onto_remote_trunk_with_provenance(
                &repo,
                &remaining.branch,
                &scope.trunk,
                &remote_info.name,
            );

            match rebase_result {
                Ok(RebaseResult::Success) => {
                    if let Some(pr_num) = remaining.pr_number {
                        let _ = rt
                            .block_on(async { client.update_pr_base(pr_num, &scope.trunk).await });
                    }

                    let _ = Command::new("git")
                        .args([
                            "push",
                            "--force-with-lease",
                            &remote_info.name,
                            &remaining.branch,
                        ])
                        .current_dir(repo.workdir()?)
                        .output();

                    LiveTimer::maybe_finish_ok(remaining_timer, "done");
                }
                Ok(RebaseResult::Conflict) => {
                    let abort_dir = repo
                        .branch_worktree_path(&remaining.branch)?
                        .unwrap_or(repo.workdir()?.to_path_buf());
                    let _ = Command::new("git")
                        .args(["rebase", "--abort"])
                        .current_dir(&abort_dir)
                        .output();
                    LiveTimer::maybe_finish_warn(remaining_timer, "conflict (skipped)");
                }
                Err(_) => {
                    LiveTimer::maybe_finish_err(remaining_timer, "failed");
                }
            }
        }
    }

    // Cleanup merged branches
    if !no_delete && !merged_prs.is_empty() {
        if !quiet {
            println!();
            println!("{}", "Cleaning up merged branches...".dimmed());
        }

        for (branch, _pr) in &merged_prs {
            let local_deleted = Command::new("git")
                .args(["branch", "-D", branch])
                .current_dir(repo.workdir()?)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            let remote_deleted = Command::new("git")
                .args(["push", &remote_info.name, "--delete", branch])
                .current_dir(repo.workdir()?)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            let _ = crate::git::refs::delete_metadata(repo.inner(), branch);

            if !quiet {
                if local_deleted && remote_deleted {
                    println!("  {} {} deleted", "✓".green(), branch.dimmed());
                } else if local_deleted {
                    println!("  {} {} deleted (local only)", "✓".green(), branch.dimmed());
                }
            }
        }

        // Checkout trunk after cleanup
        let _ = repo.checkout(&scope.trunk);
    }

    // Finish transaction
    if failed_pr.is_some() {
        tx.finish_err("Merge stopped", Some("merge-when-ready"), None)?;
    } else {
        tx.finish_ok()?;
    }

    // Print summary
    println!();

    if let Some((branch, pr, reason)) = &failed_pr {
        print_header_error("Merge Stopped");
        println!();
        println!("Progress:");
        for (merged_branch, merged_pr) in &merged_prs {
            println!(
                "  {} #{} {} → merged",
                "✓".green(),
                merged_pr,
                merged_branch
            );
        }
        println!("  {} #{} {} → {}", "✗".red(), pr, branch, reason);
        println!();
        println!("{}", "Already merged PRs remain merged.".dimmed());
        println!(
            "{}",
            "Fix the issue and run 'stax merge --when-ready' to continue.".dimmed()
        );
    } else {
        print_header_success("Stack Merged!");
        println!();
        println!(
            "Merged {} {} into {}:",
            merged_prs.len(),
            if merged_prs.len() == 1 { "PR" } else { "PRs" },
            scope.trunk.cyan()
        );
        for (branch, pr) in &merged_prs {
            println!("  {} #{} {}", "✓".green(), pr, branch);
        }

        if !remaining_branches.is_empty() {
            println!();
            println!("Remaining in stack (rebased onto {}):", scope.trunk.cyan());
            for remaining in &remaining_branches {
                if let Some(pr) = remaining.pr_number {
                    println!("  {} #{} {}", "○".dimmed(), pr, remaining.branch);
                } else {
                    println!("  {} {}", "○".dimmed(), remaining.branch);
                }
            }
        }

        if !no_delete && !merged_prs.is_empty() {
            println!();
            println!("Cleanup:");
            println!(
                "  • Deleted {} local {}",
                merged_prs.len(),
                if merged_prs.len() == 1 {
                    "branch"
                } else {
                    "branches"
                }
            );
            println!("  • Switched to: {}", scope.trunk.cyan());
        }

        // Send macOS notification
        send_notification(
            "stax merge --when-ready",
            &format!(
                "Merged {} {} into {}",
                merged_prs.len(),
                if merged_prs.len() == 1 { "PR" } else { "PRs" },
                scope.trunk
            ),
        );

        if !no_sync {
            if !quiet {
                println!();
                println!("{}", "Running post-merge sync...".dimmed());
            }

            // Release merge-side handles before sync opens a fresh repo view.
            drop(rt);
            drop(client);
            drop(repo);

            if let Err(err) = crate::commands::sync::run(
                false,      // restack
                false,      // prune
                false,      // full (fast trunk + ls-remote when deleting merged)
                !no_delete, // delete merged branches unless explicitly kept
                false,      // delete upstream-gone branches
                true,       // force
                false,      // safe
                false,      // continue
                quiet, false, // verbose
                false, // auto_stash_pop
            ) {
                if !quiet {
                    println!();
                    println!(
                        "{} {}",
                        "warning:".yellow().bold(),
                        format!("post-merge sync failed: {}", err).yellow()
                    );
                    println!(
                        "{}",
                        "Run 'stax rs --force' manually to sync local state.".dimmed()
                    );
                }
            }
        }
    }

    Ok(())
}

/// Calculate which branches to merge and which descendants remain to be rebased.
fn calculate_merge_scope(stack: &Stack, current: &str, all: bool) -> MergeWhenReadyScope {
    let mut to_merge = stack.ancestors(current);
    to_merge.reverse();
    to_merge.retain(|b| b != &stack.trunk);
    to_merge.push(current.to_string());

    let mut remaining = stack.descendants(current);

    if all && !remaining.is_empty() {
        to_merge.extend(remaining);
        remaining = Vec::new();
    }

    MergeWhenReadyScope {
        to_merge,
        remaining,
        trunk: stack.trunk.clone(),
    }
}

/// Print the merge-when-ready preview
fn print_land_preview(branches: &[LandBranchInfo], trunk: &str, method: &MergeMethod) {
    print_header("Merge When Ready");
    println!();

    let pr_word = if branches.len() == 1 { "PR" } else { "PRs" };
    println!(
        "Will merge {} {} bottom-up into {}:",
        branches.len().to_string().bold(),
        pr_word,
        trunk.cyan()
    );
    println!();

    for (idx, branch) in branches.iter().enumerate() {
        println!(
            "  {}. {} (#{}) {}",
            (idx + 1).to_string().bold(),
            branch.branch.bold(),
            branch.pr_number,
            branch.status.label()
        );
    }

    println!();
    println!(
        "Merge method: {} {}",
        method.as_str().cyan(),
        "(change with --method)".dimmed()
    );
    println!(
        "{}",
        "Each PR will be polled for CI + approval before merging.".dimmed()
    );
}

/// Print a dashboard of branch statuses during polling
fn print_dashboard(branches: &[LandBranchInfo], quiet: bool) {
    if quiet {
        return;
    }
    for (idx, branch) in branches.iter().enumerate() {
        let status_str = format!(
            "      [{}] {} (#{})\t{}",
            idx + 1,
            branch.branch,
            branch.pr_number,
            branch.status.label()
        );
        // Only print non-pending branches for concise output
        if branch.status != LandStatus::Pending {
            println!("      {} {}", branch.status.symbol(), status_str.dimmed());
        }
    }
}

/// Wait for a PR to be ready to merge (CI passed, approved)
fn wait_for_pr_ready(
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    pr_number: u64,
    timeout: Duration,
    poll_interval: Duration,
    quiet: bool,
) -> Result<WaitResult> {
    let start = Instant::now();
    let mut last_status: Option<String> = None;

    loop {
        let status: PrMergeStatus =
            rt.block_on(async { client.get_pr_merge_status(pr_number).await })?;

        if status.is_ready() {
            if !quiet && last_status.is_some() {
                println!();
            }
            return Ok(WaitResult::Ready);
        }

        if status.is_blocked() {
            if !quiet && last_status.is_some() {
                println!();
            }
            return Ok(WaitResult::Failed(status.status_text().to_string()));
        }

        if start.elapsed() > timeout {
            if !quiet && last_status.is_some() {
                println!();
            }
            return Ok(WaitResult::Timeout);
        }

        // Show waiting status
        if !quiet {
            let elapsed = start.elapsed().as_secs();
            let status_text = format!(
                "      {} Waiting for {}... ({}s)",
                "⏳".yellow(),
                status.status_text().to_lowercase(),
                elapsed
            );

            if last_status.is_some() {
                print!("\r{}\r", " ".repeat(80));
            }
            print!("{}", status_text);
            std::io::stdout().flush().ok();
            last_status = Some(status_text);
        }

        std::thread::sleep(poll_interval);
    }
}

/// Record CI history for a single branch after it's merged
fn record_ci_history_for_branch(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &ForgeClient,
    stack: &Stack,
    branch: &str,
) {
    if repo.branch_commit(branch).is_err() {
        return;
    }

    let branches = vec![branch.to_string()];
    if let Ok(statuses) = fetch_ci_statuses(repo, rt, client, stack, &branches) {
        record_ci_history(repo, &statuses);
    }
}

/// Send a macOS desktop notification
fn send_notification(title: &str, message: &str) {
    if cfg!(target_os = "macos") {
        let script = format!(
            r#"display notification "{}" with title "{}""#,
            message.replace('"', "\\\""),
            title.replace('"', "\\\""),
        );
        let _ = Command::new("osascript").args(["-e", &script]).output();
    }
}

/// Strip ANSI codes for length calculation
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_escape = false;

    for c in s.chars() {
        if c == '\x1b' {
            in_escape = true;
            continue;
        }
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
            continue;
        }
        result.push(c);
    }

    result
}

fn display_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped
        .chars()
        .map(|c| match c {
            '\x00'..='\x1f' | '\x7f' => 0,
            '\x20'..='\x7e' => 1,
            '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯'
            | '╰' | '║' | '═' => 1,
            '←' | '→' | '↑' | '↓' => 1,
            '✓' | '✗' | '✔' | '✘' => 1,
            _ => 2,
        })
        .sum()
}

fn print_header(title: &str) {
    let width: usize = 56;
    let title_width = display_width(title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        title.bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

fn print_header_success(title: &str) {
    let width: usize = 56;
    let full_title = format!("✓ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.green().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

fn print_header_error(title: &str) {
    let width: usize = 56;
    let full_title = format!("✗ {}", title);
    let title_width = display_width(&full_title);
    let padding = width.saturating_sub(title_width) / 2;
    println!("╭{}╮", "─".repeat(width));
    println!(
        "│{}{}{}│",
        " ".repeat(padding),
        full_title.red().bold(),
        " ".repeat(width.saturating_sub(padding + title_width))
    );
    println!("╰{}╯", "─".repeat(width));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    fn create_test_stack() -> Stack {
        let mut branches = HashMap::new();

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                children: vec!["feature-a".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "feature-a".to_string(),
            StackBranch {
                name: "feature-a".to_string(),
                parent: Some("main".to_string()),
                children: vec!["feature-b".to_string()],
                needs_restack: false,
                pr_number: Some(1),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        branches.insert(
            "feature-b".to_string(),
            StackBranch {
                name: "feature-b".to_string(),
                parent: Some("feature-a".to_string()),
                children: vec!["feature-c".to_string()],
                needs_restack: false,
                pr_number: Some(2),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        branches.insert(
            "feature-c".to_string(),
            StackBranch {
                name: "feature-c".to_string(),
                parent: Some("feature-b".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: Some(3),
                pr_state: Some("OPEN".to_string()),
                pr_is_draft: Some(false),
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn test_land_status_symbols() {
        // Just verify we can create each status variant
        let _ = LandStatus::Pending.symbol();
        let _ = LandStatus::WaitingForCi.symbol();
        let _ = LandStatus::Merging.symbol();
        let _ = LandStatus::Merged.symbol();
        let _ = LandStatus::Failed("test".to_string()).symbol();
    }

    #[test]
    fn test_land_status_labels() {
        let _ = LandStatus::Pending.label();
        let _ = LandStatus::WaitingForCi.label();
        let _ = LandStatus::Merging.label();
        let _ = LandStatus::Merged.label();
        let _ = LandStatus::Failed("test error".to_string()).label();
    }

    #[test]
    fn test_land_status_equality() {
        assert_eq!(LandStatus::Pending, LandStatus::Pending);
        assert_eq!(LandStatus::Merged, LandStatus::Merged);
        assert_ne!(LandStatus::Pending, LandStatus::Merged);
        assert_eq!(
            LandStatus::Failed("a".to_string()),
            LandStatus::Failed("a".to_string())
        );
        assert_ne!(
            LandStatus::Failed("a".to_string()),
            LandStatus::Failed("b".to_string())
        );
    }

    #[test]
    fn test_land_branch_info_creation() {
        let info = LandBranchInfo {
            branch: "feature-test".to_string(),
            pr_number: 42,
            status: LandStatus::Pending,
        };

        assert_eq!(info.branch, "feature-test");
        assert_eq!(info.pr_number, 42);
        assert_eq!(info.status, LandStatus::Pending);
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi(""), "");
        assert_eq!(strip_ansi("hello"), "hello");
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_display_width() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("✓"), 1);
        assert_eq!(display_width("\x1b[32m✓\x1b[0m passed"), 8);
    }

    #[test]
    fn test_calculate_merge_scope_from_middle_without_all_keeps_descendants_remaining() {
        let stack = create_test_stack();

        let scope = calculate_merge_scope(&stack, "feature-b", false);

        assert_eq!(scope.to_merge, vec!["feature-a", "feature-b"]);
        assert_eq!(scope.remaining, vec!["feature-c"]);
        assert_eq!(scope.trunk, "main");
    }

    #[test]
    fn test_calculate_merge_scope_with_all_includes_descendants() {
        let stack = create_test_stack();

        let scope = calculate_merge_scope(&stack, "feature-b", true);

        assert_eq!(scope.to_merge, vec!["feature-a", "feature-b", "feature-c"]);
        assert!(scope.remaining.is_empty());
    }
}
