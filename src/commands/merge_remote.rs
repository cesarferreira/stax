//! Merge a stack using only the GitHub API — no local checkout, rebase, or push.
//!
//! Dependent PR branches are updated via GitHub's "Update branch" endpoint (`PUT .../update-branch`).

use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::config::Config;
use crate::engine::Stack;
use crate::forge::ForgeClient;
use crate::git::GitRepo;
use crate::github::pr::{MergeMethod, PrMergeStatus};
use crate::progress::LiveTimer;
use crate::remote::{ForgeType, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct LandBranchInfo {
    branch: String,
    pr_number: u64,
}

#[derive(Debug, Clone)]
struct RemainingBranchInfo {
    branch: String,
    pr_number: Option<u64>,
}

struct MergeRemoteScope {
    to_merge: Vec<String>,
    remaining: Vec<String>,
    trunk: String,
}

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

    if current == stack.trunk {
        if !quiet {
            println!(
                "{}",
                "You are on trunk. Checkout a branch in a stack to merge.".yellow()
            );
        }
        return Ok(());
    }

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

    let scope = calculate_merge_scope(&stack, &current, all);

    let mut branches: Vec<LandBranchInfo> = Vec::new();

    let remote_info = RemoteInfo::from_repo(&repo, &config);
    let rt = tokio::runtime::Runtime::new()?;
    let _enter = rt.enter();
    let probe_client = remote_info
        .as_ref()
        .ok()
        .and_then(|info| ForgeClient::new(info).ok());

    let fetch_timer = LiveTimer::maybe_new(!quiet, "Fetching PR info...");

    for branch_name in &scope.to_merge {
        let branch_info = stack.branches.get(branch_name);
        let mut pr_number = branch_info.and_then(|b| b.pr_number);

        if pr_number.is_none() {
            if let Some(ref client) = probe_client {
                if let Ok(Some(pr_info)) = rt.block_on(async { client.find_pr(branch_name).await })
                {
                    pr_number = Some(pr_info.number);
                }
            }
        }

        match pr_number {
            Some(num) => branches.push(LandBranchInfo {
                branch: branch_name.clone(),
                pr_number: num,
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
            if let Some(ref client) = probe_client {
                if let Ok(Some(pr_info)) = rt.block_on(async { client.find_pr(branch_name).await })
                {
                    pr_number = Some(pr_info.number);
                }
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

    let remote_info = remote_info.context("Failed to read git remote configuration")?;
    if remote_info.forge != ForgeType::GitHub {
        anyhow::bail!(
            "`stax merge --remote` is only supported for GitHub remotes (found {})",
            remote_info.forge
        );
    }

    let client = ForgeClient::new(&remote_info).context(
        "Failed to connect to the configured forge. Check your token and remote configuration.",
    )?;

    if !quiet {
        println!();
        print_remote_preview(&branches, &scope.trunk, &method);
    }

    if !yes {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Proceed with merge --remote?")
            .default(false)
            .interact()?;

        if !confirm {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    if !quiet {
        println!();
        print_header("Merge Remote");
    }

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

        let is_merged = rt.block_on(async { client.is_pr_merged(pr_number).await })?;
        if is_merged {
            if !quiet {
                println!("      {} Already merged", "✓".green());
            }
            merged_prs.push((branch_name.clone(), pr_number));

            if let Some(ref next_branch) = next_branch {
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
                    Ok(()) => LiveTimer::maybe_finish_ok(update_base_timer, "done"),
                    Err(e) => {
                        LiveTimer::maybe_finish_err(update_base_timer, "failed");
                        failed_pr = Some((
                            branch_name,
                            pr_number,
                            format!(
                                "Failed to retarget dependent PR #{}: {}",
                                next_branch.pr_number, e
                            ),
                        ));
                        break;
                    }
                }
            }
        } else {
            match wait_for_pr_ready(&rt, &client, pr_number, timeout, poll_interval, quiet)? {
                WaitResult::Ready => {}
                WaitResult::Failed(reason) => {
                    failed_pr = Some((branch_name, pr_number, reason));
                    break;
                }
                WaitResult::Timeout => {
                    failed_pr =
                        Some((branch_name, pr_number, "Timeout waiting for CI".to_string()));
                    break;
                }
            }

            let merge_timer =
                LiveTimer::maybe_new(!quiet, &format!("Merging ({})...", method.as_str()));

            match rt.block_on(async { client.merge_pr(pr_number, method, None, None).await }) {
                Ok(()) => {
                    LiveTimer::maybe_finish_ok(merge_timer, "done");
                    merged_prs.push((branch_name.clone(), pr_number));
                    record_ci_history_for_branch(&repo, &rt, &client, &stack, &branch_name);
                }
                Err(e) => {
                    LiveTimer::maybe_finish_err(merge_timer, "failed");
                    failed_pr = Some((branch_name, pr_number, e.to_string()));
                    break;
                }
            }

            // Retarget next PR to trunk after successful merge
            if let Some(ref next_branch) = next_branch {
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
                    Ok(()) => LiveTimer::maybe_finish_ok(update_base_timer, "done"),
                    Err(e) => {
                        LiveTimer::maybe_finish_err(update_base_timer, "failed");
                        failed_pr = Some((
                            branch_name,
                            pr_number,
                            format!(
                                "Failed to retarget dependent PR #{}: {}",
                                next_branch.pr_number, e
                            ),
                        ));
                        break;
                    }
                }
            }
        }

        if let Some(next_branch) = next_branch {
            let update_timer = LiveTimer::maybe_new(
                !quiet,
                &format!(
                    "Updating branch for #{} on GitHub (merge {} → head)...",
                    next_branch.pr_number, scope.trunk
                ),
            );

            match rt.block_on(async { client.update_pr_branch(next_branch.pr_number).await }) {
                Ok(()) => LiveTimer::maybe_finish_ok(update_timer, "done"),
                Err(e) => {
                    LiveTimer::maybe_finish_err(update_timer, "failed");
                    failed_pr = Some((
                        next_branch.branch.clone(),
                        next_branch.pr_number,
                        format!(
                            "Failed to update branch for PR #{}: {}",
                            next_branch.pr_number, e
                        ),
                    ));
                    break;
                }
            }
        }
    }

    if !merged_prs.is_empty() && !remaining_branches.is_empty() && failed_pr.is_none() {
        if !quiet {
            println!();
            println!(
                "{}",
                "Updating remaining stack branches on GitHub...".dimmed()
            );
        }

        for remaining in &remaining_branches {
            let Some(pr_num) = remaining.pr_number else {
                continue;
            };

            let base_timer = LiveTimer::maybe_new(
                !quiet,
                &format!("Retargeting #{} to {}...", pr_num, scope.trunk),
            );
            match rt.block_on(async { client.update_pr_base(pr_num, &scope.trunk).await }) {
                Ok(()) => LiveTimer::maybe_finish_ok(base_timer, "done"),
                Err(e) => {
                    LiveTimer::maybe_finish_err(base_timer, "failed");
                    failed_pr = Some((
                        remaining.branch.clone(),
                        pr_num,
                        format!("Failed to retarget PR #{}: {}", pr_num, e),
                    ));
                    break;
                }
            }

            let update_timer = LiveTimer::maybe_new(
                !quiet,
                &format!("Updating branch for #{} on GitHub...", pr_num),
            );
            match rt.block_on(async { client.update_pr_branch(pr_num).await }) {
                Ok(()) => LiveTimer::maybe_finish_ok(update_timer, "done"),
                Err(e) => {
                    LiveTimer::maybe_finish_err(update_timer, "failed");
                    failed_pr = Some((
                        remaining.branch.clone(),
                        pr_num,
                        format!("Failed to update branch for PR #{}: {}", pr_num, e),
                    ));
                    break;
                }
            }
        }
    }

    if !no_delete && !merged_prs.is_empty() {
        if !quiet {
            println!();
            println!(
                "{}",
                "Cleaning up stax metadata for merged branches...".dimmed()
            );
        }

        for (branch, _) in &merged_prs {
            let _ = crate::git::refs::delete_metadata(repo.inner(), branch);
            if !quiet {
                println!("  {} metadata cleared for {}", "✓".green(), branch.dimmed());
            }
        }
    }

    println!();

    if let Some((branch, pr, reason)) = &failed_pr {
        print_header_error("Merge Remote Stopped");
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
            "Fix the issue and run 'stax merge --remote' to continue.".dimmed()
        );
    } else {
        print_header_success("Stack Merged (remote)");
        println!();
        println!(
            "Merged {} {} into {} via GitHub API:",
            merged_prs.len(),
            if merged_prs.len() == 1 { "PR" } else { "PRs" },
            scope.trunk.cyan()
        );
        for (branch, pr) in &merged_prs {
            println!("  {} #{} {}", "✓".green(), pr, branch);
        }

        if !remaining_branches.is_empty() {
            println!();
            println!("Remaining in stack (branches updated on GitHub):");
            for remaining in &remaining_branches {
                if let Some(pr) = remaining.pr_number {
                    println!("  {} #{} {}", "○".dimmed(), pr, remaining.branch);
                } else {
                    println!("  {} {}", "○".dimmed(), remaining.branch);
                }
            }
        }

        send_notification(
            "stax merge --remote",
            &format!(
                "Merged {} {} into {}",
                merged_prs.len(),
                if merged_prs.len() == 1 { "PR" } else { "PRs" },
                scope.trunk
            ),
        );
    }

    if failed_pr.is_none() && !merged_prs.is_empty() && !no_sync && !quiet {
        println!();
        println!(
            "{}",
            "Run `stax rs` to sync your local repository (delete merged branches, reparent children)."
                .dimmed()
        );
    }

    Ok(())
}

fn calculate_merge_scope(stack: &Stack, current: &str, all: bool) -> MergeRemoteScope {
    let mut to_merge = stack.ancestors(current);
    to_merge.reverse();
    to_merge.retain(|b| b != &stack.trunk);
    to_merge.push(current.to_string());

    let mut remaining = stack.descendants(current);

    if all && !remaining.is_empty() {
        to_merge.extend(remaining);
        remaining = Vec::new();
    }

    MergeRemoteScope {
        to_merge,
        remaining,
        trunk: stack.trunk.clone(),
    }
}

fn print_remote_preview(branches: &[LandBranchInfo], trunk: &str, method: &MergeMethod) {
    print_header("Merge Remote");
    println!();

    let pr_word = if branches.len() == 1 { "PR" } else { "PRs" };
    println!(
        "Will merge {} {} bottom-up into {} (GitHub API only — no local git):",
        branches.len().to_string().bold(),
        pr_word,
        trunk.cyan()
    );
    println!();

    for (idx, branch) in branches.iter().enumerate() {
        println!(
            "  {}. {} (#{})",
            (idx + 1).to_string().bold(),
            branch.branch.bold(),
            branch.pr_number,
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
        "Each PR is polled for CI + approval; dependent branches use GitHub \"Update branch\"."
            .dimmed()
    );
}

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
