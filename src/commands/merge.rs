use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{CiStatus, MergeMethod, PrMergeStatus};
use crate::github::GitHubClient;
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

/// Information about a branch in the merge scope
#[derive(Debug, Clone)]
struct MergeBranchInfo {
    branch: String,
    pr_number: Option<u64>,
    pr_status: Option<PrMergeStatus>,
    is_current: bool,
    position: usize,
}

/// Result of the merge scope calculation
struct MergeScope {
    /// Branches to merge (bottom to current)
    to_merge: Vec<MergeBranchInfo>,
    /// Branches not included (above current)
    remaining: Vec<MergeBranchInfo>,
    /// The trunk branch name
    trunk: String,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all: bool,
    dry_run: bool,
    method: MergeMethod,
    no_delete: bool,
    no_wait: bool,
    timeout_mins: u64,
    yes: bool,
    quiet: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;

    // Check if we're on a tracked branch
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
                format!("Branch '{}' is not tracked. Run 'stax branch track' first.", current).yellow()
            );
        }
        return Ok(());
    }

    // Calculate merge scope based on current position
    let mut scope = calculate_merge_scope(&repo, &stack, &current, all)?;

    if scope.to_merge.is_empty() {
        if !quiet {
            println!("{}", "No branches to merge.".yellow());
        }
        return Ok(());
    }

    // Set up GitHub client for PR lookups
    let remote_info = RemoteInfo::from_repo(&repo, &config);
    let rt = tokio::runtime::Runtime::new()?;

    // Try to create GitHub client (may fail if no remote or no token)
    let client = remote_info.as_ref().ok().and_then(|info| {
        rt.block_on(async {
            GitHubClient::new(info.owner(), &info.repo, info.api_base_url.clone())
        })
        .ok()
    });

    // For branches missing PR metadata, check GitHub for existing PRs
    if let Some(ref client) = client {
        for branch_info in &mut scope.to_merge {
            if branch_info.pr_number.is_none() {
                if let Ok(Some(pr_info)) = rt.block_on(async { client.find_pr(&branch_info.branch).await }) {
                    branch_info.pr_number = Some(pr_info.number);
                }
            }
        }
    }

    // Check that all branches have PRs (after GitHub lookup)
    let missing_prs: Vec<_> = scope
        .to_merge
        .iter()
        .filter(|b| b.pr_number.is_none())
        .map(|b| b.branch.clone())
        .collect();

    if !missing_prs.is_empty() {
        anyhow::bail!(
            "The following branches don't have PRs:\n  {}\n\nRun 'stax submit' first to create PRs.",
            missing_prs.join("\n  ")
        );
    }

    // Get remote info and client (will fail with clear error if not available)
    let remote_info = remote_info?;
    let client = client.ok_or_else(|| anyhow::anyhow!("Failed to connect to GitHub. Check your token and remote configuration."))?;

    if !quiet {
        print!("Fetching PR status... ");
        std::io::stdout().flush().ok();
    }

    // Fetch status for branches to merge
    for branch_info in &mut scope.to_merge {
        if let Some(pr_num) = branch_info.pr_number {
            let status = rt.block_on(async { client.get_pr_merge_status(pr_num).await })?;
            branch_info.pr_status = Some(status);
        }
    }

    // Fetch status for remaining branches too (for display)
    for branch_info in &mut scope.remaining {
        if let Some(pr_num) = branch_info.pr_number {
            if let Ok(status) = rt.block_on(async { client.get_pr_merge_status(pr_num).await }) {
                branch_info.pr_status = Some(status);
            }
        }
    }

    if !quiet {
        println!("{}", "done".green());
        println!();
    }

    // Display the merge preview
    if !quiet {
        print_merge_preview(&scope, &method);
    }

    // Dry run - just show plan and exit
    if dry_run {
        if !quiet {
            println!();
            println!("{}", "Dry run - no changes made.".dimmed());
        }
        return Ok(());
    }

    // Confirm with user
    if !yes && !quiet {
        let has_waiting = scope.to_merge.iter().any(|b| {
            b.pr_status
                .as_ref()
                .map(|s| s.is_waiting())
                .unwrap_or(false)
        });

        let prompt = if has_waiting && !no_wait {
            "Proceed with merge? (will wait for pending checks)"
        } else {
            "Proceed with merge?"
        };

        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .default(false)
            .interact()?;

        if !confirm {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }
    }

    // Execute the merge
    if !quiet {
        println!();
        print_header("Merging Stack");
    }

    let timeout = Duration::from_secs(timeout_mins * 60);
    let mut merged_prs: Vec<(String, u64)> = Vec::new();
    let mut failed_pr: Option<(String, u64, String)> = None;
    let total = scope.to_merge.len();

    for (idx, branch_info) in scope.to_merge.iter().enumerate() {
        let pr_number = branch_info.pr_number.unwrap();
        let position = idx + 1;

        if !quiet {
            println!();
            println!(
                "[{}/{}] {} (#{})",
                position.to_string().cyan(),
                total,
                branch_info.branch.bold(),
                pr_number
            );
        }

        // Check if already merged
        let is_merged = rt.block_on(async { client.is_pr_merged(pr_number).await })?;
        if is_merged {
            if !quiet {
                println!("      {} Already merged", "✓".green());
            }
            merged_prs.push((branch_info.branch.clone(), pr_number));
            continue;
        }

        // Wait for CI and approval if needed
        if !no_wait {
            match wait_for_pr_ready(&rt, &client, pr_number, timeout, quiet)? {
                WaitResult::Ready => {}
                WaitResult::Failed(reason) => {
                    failed_pr = Some((branch_info.branch.clone(), pr_number, reason));
                    break;
                }
                WaitResult::Timeout => {
                    failed_pr = Some((
                        branch_info.branch.clone(),
                        pr_number,
                        "Timeout waiting for CI".to_string(),
                    ));
                    break;
                }
            }
        } else {
            // Check if ready without waiting
            let status = rt.block_on(async { client.get_pr_merge_status(pr_number).await })?;
            if !status.is_ready() {
                failed_pr = Some((
                    branch_info.branch.clone(),
                    pr_number,
                    format!("PR not ready: {}", status.status_text()),
                ));
                break;
            }
        }

        // Merge the PR
        if !quiet {
            print!("      {} Merging ({})... ", "↻".cyan(), method.as_str());
            std::io::stdout().flush().ok();
        }

        match rt.block_on(async { client.merge_pr(pr_number, method, None, None).await }) {
            Ok(()) => {
                if !quiet {
                    println!("{}", "done".green());
                }
                merged_prs.push((branch_info.branch.clone(), pr_number));
            }
            Err(e) => {
                if !quiet {
                    println!("{}", "failed".red());
                }
                failed_pr = Some((branch_info.branch.clone(), pr_number, e.to_string()));
                break;
            }
        }

        // If there are more PRs, rebase and update the next one
        if idx + 1 < total {
            let next_branch = &scope.to_merge[idx + 1];
            let next_pr = next_branch.pr_number.unwrap();

            // Fetch latest from remote
            if !quiet {
                print!("      {} Fetching latest... ", "↻".cyan());
                std::io::stdout().flush().ok();
            }

            let fetch_output = Command::new("git")
                .args(["fetch", &remote_info.name])
                .current_dir(repo.workdir()?)
                .output()
                .context("Failed to fetch")?;

            if !fetch_output.status.success() {
                if !quiet {
                    println!("{}", "warning".yellow());
                }
            } else if !quiet {
                println!("{}", "done".green());
            }

            // Rebase next branch onto trunk
            if !quiet {
                print!(
                    "      {} Rebasing {} onto {}... ",
                    "↻".cyan(),
                    next_branch.branch,
                    scope.trunk
                );
                std::io::stdout().flush().ok();
            }

            repo.checkout(&next_branch.branch)?;

            let rebase_status = Command::new("git")
                .args(["rebase", &format!("{}/{}", remote_info.name, scope.trunk)])
                .current_dir(repo.workdir()?)
                .output()
                .context("Failed to rebase")?;

            if !rebase_status.status.success() {
                // Abort rebase on failure
                let _ = Command::new("git")
                    .args(["rebase", "--abort"])
                    .current_dir(repo.workdir()?)
                    .output();

                if !quiet {
                    println!("{}", "conflict".red());
                }
                failed_pr = Some((
                    next_branch.branch.clone(),
                    next_pr,
                    "Rebase conflict".to_string(),
                ));
                break;
            }

            if !quiet {
                println!("{}", "done".green());
            }

            // Update PR base to trunk
            if !quiet {
                print!("      {} Updating PR base to {}... ", "↻".cyan(), scope.trunk);
                std::io::stdout().flush().ok();
            }

            match rt.block_on(async { client.update_pr_base(next_pr, &scope.trunk).await }) {
                Ok(()) => {
                    if !quiet {
                        println!("{}", "done".green());
                    }
                }
                Err(e) => {
                    if !quiet {
                        println!("{} {}", "warning:".yellow(), e);
                    }
                }
            }

            // Force push the rebased branch
            if !quiet {
                print!("      {} Pushing... ", "↻".cyan());
                std::io::stdout().flush().ok();
            }

            let push_status = Command::new("git")
                .args(["push", "-f", &remote_info.name, &next_branch.branch])
                .current_dir(repo.workdir()?)
                .output()
                .context("Failed to push")?;

            if !push_status.status.success() {
                if !quiet {
                    println!("{}", "failed".red());
                }
                failed_pr = Some((
                    next_branch.branch.clone(),
                    next_pr,
                    "Failed to push rebased branch".to_string(),
                ));
                break;
            }

            if !quiet {
                println!("{}", "done".green());
            }

            // Update local metadata
            if let Some(meta) = BranchMetadata::read(repo.inner(), &next_branch.branch)? {
                let trunk_commit = repo.branch_commit(&scope.trunk)?;
                let updated_meta = BranchMetadata {
                    parent_branch_name: scope.trunk.clone(),
                    parent_branch_revision: trunk_commit,
                    ..meta
                };
                updated_meta.write(repo.inner(), &next_branch.branch)?;
            }
        }
    }

    // Rebase remaining branches onto trunk if any PRs were merged
    if !merged_prs.is_empty() && !scope.remaining.is_empty() && failed_pr.is_none() {
        if !quiet {
            println!();
            println!("{}", "Rebasing remaining stack branches...".dimmed());
        }

        for remaining in &scope.remaining {
            if !quiet {
                print!("  {} {}... ", "↻".cyan(), remaining.branch);
                std::io::stdout().flush().ok();
            }

            repo.checkout(&remaining.branch)?;

            let rebase_result = Command::new("git")
                .args(["rebase", &format!("{}/{}", remote_info.name, scope.trunk)])
                .current_dir(repo.workdir()?)
                .output();

            if let Ok(output) = rebase_result {
                if output.status.success() {
                    // Update PR base
                    if let Some(pr_num) = remaining.pr_number {
                        let _ = rt.block_on(async { client.update_pr_base(pr_num, &scope.trunk).await });
                    }

                    // Update metadata
                    if let Some(meta) = BranchMetadata::read(repo.inner(), &remaining.branch)? {
                        let trunk_commit = repo.branch_commit(&scope.trunk)?;
                        let updated_meta = BranchMetadata {
                            parent_branch_name: scope.trunk.clone(),
                            parent_branch_revision: trunk_commit,
                            ..meta
                        };
                        updated_meta.write(repo.inner(), &remaining.branch)?;
                    }

                    // Push
                    let _ = Command::new("git")
                        .args(["push", "-f", &remote_info.name, &remaining.branch])
                        .current_dir(repo.workdir()?)
                        .output();

                    if !quiet {
                        println!("{}", "done".green());
                    }
                } else {
                    let _ = Command::new("git")
                        .args(["rebase", "--abort"])
                        .current_dir(repo.workdir()?)
                        .output();
                    if !quiet {
                        println!("{}", "conflict (skipped)".yellow());
                    }
                }
            } else if !quiet {
                println!("{}", "failed".red());
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
            // Delete local branch
            let local_deleted = Command::new("git")
                .args(["branch", "-D", branch])
                .current_dir(repo.workdir()?)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            // Delete remote branch
            let remote_deleted = Command::new("git")
                .args(["push", &remote_info.name, "--delete", branch])
                .current_dir(repo.workdir()?)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            // Delete metadata
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

    // Print summary
    println!();

    if let Some((branch, pr, reason)) = failed_pr {
        print_header_error("Merge Stopped");
        println!();
        println!("Progress:");
        for (merged_branch, merged_pr) in &merged_prs {
            println!("  {} #{} {} → merged", "✓".green(), merged_pr, merged_branch);
        }
        println!("  {} #{} {} → {}", "✗".red(), pr, branch, reason);
        println!();
        println!(
            "{}",
            "Already merged PRs remain merged.".dimmed()
        );
        println!(
            "{}",
            "Fix the issue and run 'stax merge' to continue.".dimmed()
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

        if !scope.remaining.is_empty() {
            println!();
            println!("Remaining in stack (rebased onto {}):", scope.trunk.cyan());
            for remaining in &scope.remaining {
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

        if !scope.remaining.is_empty() {
            println!();
            println!(
                "{}",
                "Tip: Run 'stax merge' again to continue merging the rest of the stack.".dimmed()
            );
        }
    }

    Ok(())
}

/// Calculate which branches to merge based on current position
fn calculate_merge_scope(
    _repo: &GitRepo,
    stack: &Stack,
    current: &str,
    all: bool,
) -> Result<MergeScope> {
    // Get ancestors of current branch (from current up to trunk)
    let mut ancestors = stack.ancestors(current);
    ancestors.reverse(); // Now bottom-to-top (trunk-adjacent first)

    // Remove trunk from ancestors if present
    ancestors.retain(|b| b != &stack.trunk);

    // Build list of branches from bottom to current
    let mut to_merge: Vec<MergeBranchInfo> = Vec::new();

    for (idx, branch) in ancestors.iter().enumerate() {
        let branch_info = stack.branches.get(branch);
        let pr_number = branch_info.and_then(|b| b.pr_number);

        to_merge.push(MergeBranchInfo {
            branch: branch.clone(),
            pr_number,
            pr_status: None,
            is_current: false,
            position: idx + 1,
        });
    }

    // Add current branch
    let current_info = stack.branches.get(current);
    let current_pr = current_info.and_then(|b| b.pr_number);
    let current_position = to_merge.len() + 1;

    to_merge.push(MergeBranchInfo {
        branch: current.to_string(),
        pr_number: current_pr,
        pr_status: None,
        is_current: true,
        position: current_position,
    });

    // Get descendants (branches above current)
    let descendants = stack.descendants(current);

    let mut remaining: Vec<MergeBranchInfo> = Vec::new();
    for (idx, branch) in descendants.iter().enumerate() {
        let branch_info = stack.branches.get(branch);
        let pr_number = branch_info.and_then(|b| b.pr_number);

        remaining.push(MergeBranchInfo {
            branch: branch.clone(),
            pr_number,
            pr_status: None,
            is_current: false,
            position: current_position + idx + 1,
        });
    }

    // If --all flag, merge everything
    if all && !remaining.is_empty() {
        to_merge.extend(remaining);
        remaining = Vec::new();
    }

    Ok(MergeScope {
        to_merge,
        remaining,
        trunk: stack.trunk.clone(),
    })
}

/// Print the merge preview box
fn print_merge_preview(scope: &MergeScope, method: &MergeMethod) {
    print_header("Stack Merge");
    println!();

    // Find the current branch for display
    let current = scope
        .to_merge
        .iter()
        .find(|b| b.is_current)
        .map(|b| b.branch.as_str())
        .unwrap_or("unknown");

    let current_pr = scope
        .to_merge
        .iter()
        .find(|b| b.is_current)
        .and_then(|b| b.pr_number)
        .map(|n| format!(" (PR #{})", n))
        .unwrap_or_default();

    println!("You are on: {}{}", current.cyan().bold(), current_pr.dimmed());
    println!();

    let pr_word = if scope.to_merge.len() == 1 { "PR" } else { "PRs" };
    println!(
        "This will merge {} {} from bottom → current:",
        scope.to_merge.len().to_string().bold(),
        pr_word
    );
    println!();

    // Print branches to merge
    print_branch_box(&scope.to_merge, true);

    // Print remaining branches if any
    if !scope.remaining.is_empty() {
        println!();
        print_branch_box(&scope.remaining, false);
    }

    println!();
    println!(
        "Merge method: {} {}",
        method.as_str().cyan(),
        "(change with --method)".dimmed()
    );
}

/// Print branch info as a checklist
fn print_branch_box(branches: &[MergeBranchInfo], included: bool) {
    println!();

    for (idx, branch) in branches.iter().enumerate() {
        let pr_text = branch
            .pr_number
            .map(|n| format!("#{}", n))
            .unwrap_or_else(|| "no PR".to_string());

        // Branch header
        println!(
            "  {}. {} {}",
            branch.position.to_string().bold(),
            branch.branch.bold(),
            format!("({})", pr_text).dimmed()
        );

        if included {
            if let Some(ref pr_status) = branch.pr_status {
                // Checklist items
                let ci_check = match pr_status.ci_status {
                    CiStatus::Success => format!("  {} CI checks passed", "✓".green()),
                    CiStatus::Pending => format!("  {} CI checks running...", "○".yellow()),
                    CiStatus::Failure => format!("  {} CI checks failed", "✗".red()),
                    CiStatus::NoCi => format!("  {} No CI checks required", "✓".green()),
                };
                println!("{}", ci_check);

                let review_check = if pr_status.changes_requested {
                    format!("  {} Changes requested", "✗".red())
                } else if pr_status.approvals > 0 {
                    format!("  {} Approved ({} review{})", "✓".green(), pr_status.approvals, if pr_status.approvals == 1 { "" } else { "s" })
                } else {
                    format!("  {} Awaiting review...", "○".yellow())
                };
                println!("{}", review_check);

                let mergeable_check = if pr_status.mergeable == Some(false) {
                    format!("  {} Has merge conflicts", "✗".red())
                } else if pr_status.mergeable == Some(true) {
                    format!("  {} No conflicts", "✓".green())
                } else {
                    format!("  {} Checking conflicts...", "○".yellow())
                };
                println!("{}", mergeable_check);

                // Merge target
                let merge_into = if branch.position == 1 {
                    "main".to_string()
                } else {
                    "main (after rebase)".to_string()
                };
                println!("  {} Merge into {}", "→".dimmed(), merge_into);
            } else {
                println!("  {} Fetching status...", "○".yellow());
            }
        } else {
            println!("  {} Not included in this merge", "·".dimmed());
            println!("  {} Will be rebased onto main", "→".dimmed());
        }

        // Add spacing between branches
        if idx < branches.len() - 1 {
            println!();
        }
    }

    println!();
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

/// Calculate the display width of a string, accounting for ANSI codes and wide Unicode chars
fn display_width(s: &str) -> usize {
    let stripped = strip_ansi(s);
    stripped.chars().map(char_width).sum()
}

/// Get the display width of a single character
fn char_width(c: char) -> usize {
    // Use unicode_width crate logic for accurate width calculation
    // For now, use a simplified approach that works for our specific use case
    match c {
        // Control characters and zero-width
        '\x00'..='\x1f' | '\x7f' => 0,
        // ASCII is width 1
        '\x20'..='\x7e' => 1,
        // Box drawing characters are width 1
        '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' 
        | '╭' | '╮' | '╯' | '╰' | '║' | '═' => 1,
        // Arrows - typically width 1 in most terminals
        '←' | '→' | '↑' | '↓' => 1,
        // Checkmarks and X marks - width 1 in most monospace fonts
        '✓' | '✗' | '✔' | '✘' => 1,
        // Everything else (including emojis) - assume width 2
        _ => 2,
    }
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

/// Result of waiting for a PR to be ready
enum WaitResult {
    Ready,
    Failed(String),
    Timeout,
}

/// Wait for a PR to be ready to merge (CI passed, approved)
fn wait_for_pr_ready(
    rt: &tokio::runtime::Runtime,
    client: &GitHubClient,
    pr_number: u64,
    timeout: Duration,
    quiet: bool,
) -> Result<WaitResult> {
    let start = Instant::now();
    let poll_interval = Duration::from_secs(10);
    let mut last_status: Option<String> = None;

    loop {
        let status = rt.block_on(async { client.get_pr_merge_status(pr_number).await })?;

        // Check if ready
        if status.is_ready() {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
            }
            return Ok(WaitResult::Ready);
        }

        // Check if blocked (won't become ready)
        if status.is_blocked() {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
            }
            return Ok(WaitResult::Failed(status.status_text().to_string()));
        }

        // Check timeout
        if start.elapsed() > timeout {
            if !quiet && last_status.is_some() {
                println!(); // End the waiting line
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

            // Clear and rewrite the line
            if last_status.is_some() {
                print!("\r{}\r", " ".repeat(80));
            }
            print!("{}", status_text);
            std::io::stdout().flush().ok();
            last_status = Some(status_text);
        }

        // Wait before next poll
        std::thread::sleep(poll_interval);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_no_codes() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_with_color_codes() {
        // Red text: \x1b[31mred\x1b[0m
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_ansi_with_multiple_codes() {
        // Bold + red: \x1b[1m\x1b[31mtext\x1b[0m
        assert_eq!(strip_ansi("\x1b[1m\x1b[31mtext\x1b[0m"), "text");
    }

    #[test]
    fn test_strip_ansi_complex() {
        let colored = "\x1b[32m✓\x1b[0m \x1b[1mBold\x1b[0m \x1b[33mYellow\x1b[0m";
        assert_eq!(strip_ansi(colored), "✓ Bold Yellow");
    }

    #[test]
    fn test_strip_ansi_preserves_unicode() {
        let with_emoji = "\x1b[32m✓\x1b[0m Success 🎉";
        assert_eq!(strip_ansi(with_emoji), "✓ Success 🎉");
    }

    #[test]
    fn test_display_width_ascii() {
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("hello world"), 11);
    }

    #[test]
    fn test_display_width_symbols() {
        // Check marks and X marks are width 1
        assert_eq!(display_width("✓"), 1);
        assert_eq!(display_width("✗"), 1);
        // Other emojis are width 2
        assert_eq!(display_width("⏳"), 2);
    }

    #[test]
    fn test_display_width_mixed() {
        // "✓ passed" = 1 (checkmark) + 1 (space) + 6 (passed) = 8
        assert_eq!(display_width("✓ passed"), 8);
        // "~ pending" = 1 (~) + 1 (space) + 7 (pending) = 9 (using ASCII now)
        assert_eq!(display_width("~ pending"), 9);
    }

    #[test]
    fn test_display_width_with_ansi() {
        // ANSI codes should be ignored
        assert_eq!(display_width("\x1b[32m✓\x1b[0m passed"), 8);
    }

    #[test]
    fn test_merge_branch_info_creation() {
        let info = MergeBranchInfo {
            branch: "feature-test".to_string(),
            pr_number: Some(42),
            pr_status: None,
            is_current: true,
            position: 1,
        };

        assert_eq!(info.branch, "feature-test");
        assert_eq!(info.pr_number, Some(42));
        assert!(info.is_current);
        assert_eq!(info.position, 1);
    }

    #[test]
    fn test_merge_scope_creation() {
        let scope = MergeScope {
            to_merge: vec![
                MergeBranchInfo {
                    branch: "feature-a".to_string(),
                    pr_number: Some(1),
                    pr_status: None,
                    is_current: false,
                    position: 1,
                },
                MergeBranchInfo {
                    branch: "feature-b".to_string(),
                    pr_number: Some(2),
                    pr_status: None,
                    is_current: true,
                    position: 2,
                },
            ],
            remaining: vec![
                MergeBranchInfo {
                    branch: "feature-c".to_string(),
                    pr_number: Some(3),
                    pr_status: None,
                    is_current: false,
                    position: 3,
                },
            ],
            trunk: "main".to_string(),
        };

        assert_eq!(scope.to_merge.len(), 2);
        assert_eq!(scope.remaining.len(), 1);
        assert_eq!(scope.trunk, "main");
    }

    #[test]
    fn test_wait_result_variants() {
        // Test that all variants can be created
        let _ready = WaitResult::Ready;
        let _failed = WaitResult::Failed("CI failed".to_string());
        let _timeout = WaitResult::Timeout;
    }
}

