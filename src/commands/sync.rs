use crate::cache::CiCache;
use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::github::GitHubClient;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::io::Write;
use std::process::Command;

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
pub fn run(
    restack: bool,
    delete_merged: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?;
    let config = Config::load()?;
    let remote_name = config.remote_name().to_string();

    if r#continue {
        crate::commands::continue_cmd::run()?;
        if repo.rebase_in_progress()? {
            return Ok(());
        }
    }

    let auto_confirm = force;
    let mut stashed = false;
    if repo.is_dirty()? {
        if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        }

        let stash = if auto_confirm {
            true
        } else {
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Working tree has uncommitted changes. Stash them before sync?")
                .default(true)
                .interact()?
        };

        if stash {
            stashed = repo.stash_push()?;
            if !quiet {
                println!("{}", "✓ Stashed working tree changes.".green());
            }
        } else {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
    }

    if !quiet {
        println!("{}", "Syncing repository...".bold());
    }

    // 1. Fetch from remote
    if !quiet {
        print!("  Fetching from {}... ", remote_name);
        let _ = std::io::stdout().flush();
    }

    let output = Command::new("git")
        .args(["fetch", &remote_name])
        .current_dir(workdir)
        .output()
        .context("Failed to fetch")?;

    if !quiet {
        if output.status.success() {
            println!("{}", "done".green());
            if verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else {
            // Fetch may fail partially (lock files, etc.) but still update most refs
            println!("{}", "done (with warnings)".yellow());
            if verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        }
    }

    // 2. Update trunk branch (before merged branch detection, so detection works correctly)
    // Note: If we're not on trunk, we use a refspec fetch which may fail if local trunk
    // has diverged. This is fine - we'll retry after branch deletions if we end up on trunk.
    let was_on_trunk = current == stack.trunk;
    let mut trunk_update_deferred = false;

    if was_on_trunk {
        // We're on trunk - pull directly
        if !quiet {
            print!("  Updating {}... ", stack.trunk.cyan());
            let _ = std::io::stdout().flush();
        }

        let output = Command::new("git")
            .args(["pull", "--ff-only", &remote_name, &stack.trunk])
            .current_dir(workdir)
            .output()
            .context("Failed to pull trunk")?;

        if output.status.success() {
            if !quiet {
                println!("{}", "done".green());
                if verbose {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if !stdout.trim().is_empty() {
                        for line in stdout.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        } else if safe {
            if !quiet {
                println!("{}", "failed (safe mode, no reset)".yellow());
                if verbose {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        } else {
            // Try reset to remote
            let reset_output = Command::new("git")
                .args([
                    "reset",
                    "--hard",
                    &format!("{}/{}", remote_name, stack.trunk),
                ])
                .current_dir(workdir)
                .output()
                .context("Failed to reset trunk")?;

            if !quiet {
                if reset_output.status.success() {
                    println!("{}", "reset to remote".yellow());
                } else {
                    println!("{}", "failed".red());
                    if verbose {
                        let stderr = String::from_utf8_lossy(&reset_output.stderr);
                        if !stderr.trim().is_empty() {
                            for line in stderr.lines() {
                                println!("    {}", line.dimmed());
                            }
                        }
                    }
                }
            }
        }
    } else {
        if !quiet {
            print!("  Updating {}... ", stack.trunk.cyan());
            let _ = std::io::stdout().flush();
        }

        if let Some(trunk_worktree_path) = repo.branch_worktree_path(&stack.trunk)? {
            let output = Command::new("git")
                .args(["pull", "--ff-only", &remote_name, &stack.trunk])
                .current_dir(&trunk_worktree_path)
                .output()
                .context("Failed to pull trunk in its worktree")?;

            if output.status.success() {
                if !quiet {
                    println!("{}", "done".green());
                }
            } else if safe {
                if !quiet {
                    println!("{}", "failed (safe mode, no reset)".yellow());
                    if verbose {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        if !stderr.trim().is_empty() {
                            for line in stderr.lines() {
                                println!("    {}", line.dimmed());
                            }
                        }
                    }
                }
            } else {
                let reset_output = Command::new("git")
                    .args([
                        "reset",
                        "--hard",
                        &format!("{}/{}", remote_name, stack.trunk),
                    ])
                    .current_dir(&trunk_worktree_path)
                    .output()
                    .context("Failed to reset trunk in its worktree")?;

                if !quiet {
                    if reset_output.status.success() {
                        println!("{}", "reset to remote".yellow());
                    } else {
                        println!("{}", "failed".red());
                        if verbose {
                            let stderr = String::from_utf8_lossy(&reset_output.stderr);
                            if !stderr.trim().is_empty() {
                                for line in stderr.lines() {
                                    println!("    {}", line.dimmed());
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Trunk isn't checked out in any worktree; update via refspec fetch.
            let output = Command::new("git")
                .args([
                    "fetch",
                    &remote_name,
                    &format!("{}:{}", stack.trunk, stack.trunk),
                ])
                .current_dir(workdir)
                .output()
                .context("Failed to update trunk")?;

            if output.status.success() {
                if !quiet {
                    println!("{}", "done".green());
                }
            } else {
                // Defer trunk update - we'll retry after branch deletions if we end up on trunk
                trunk_update_deferred = true;
                if !quiet {
                    println!("{}", "deferred".dimmed());
                }
            }
        }
    }

    // 3. Delete merged branches
    if delete_merged {
        let merged = find_merged_branches(workdir, &stack, &remote_name)?;

        // Lazy-initialize GitHub client for updating PR bases (only if needed)
        let github_client: Option<(tokio::runtime::Runtime, GitHubClient)> = {
            let remote_info = RemoteInfo::from_repo(&repo, &config).ok();
            let has_github_token = Config::github_token().is_some();

            if has_github_token {
                if let Some(info) = remote_info {
                    tokio::runtime::Runtime::new().ok().and_then(|rt| {
                        // Must create client inside block_on - Octocrab requires runtime context
                        rt.block_on(async {
                            GitHubClient::new(info.owner(), &info.repo, info.api_base_url.clone())
                                .ok()
                        })
                        .map(|client| (rt, client))
                    })
                } else {
                    None
                }
            } else {
                None
            }
        };

        if !merged.is_empty() {
            if !quiet {
                let branch_word = if merged.len() == 1 {
                    "branch"
                } else {
                    "branches"
                };
                println!(
                    "  Found {} merged {}:",
                    merged.len().to_string().cyan(),
                    branch_word
                );
                for branch in &merged {
                    println!("    {} {}", "▸".bright_black(), branch);
                }
                println!();
            }

            // Record CI history for merged branches before deleting them
            if let Some((ref rt, ref client)) = github_client {
                record_ci_history_for_merged(&repo, rt, client, &merged, &stack, quiet);
            }

            for branch in &merged {
                let is_current_branch = branch == &current;

                // Get parent branch for context
                let parent_branch = stack
                    .branches
                    .get(branch)
                    .and_then(|b| b.parent.clone())
                    .unwrap_or_else(|| stack.trunk.clone());

                let prompt = if is_current_branch {
                    format!("Delete '{}' and checkout '{}'?", branch, parent_branch)
                } else {
                    format!("Delete '{}'?", branch)
                };

                let confirm = if auto_confirm {
                    true
                } else if quiet {
                    false
                } else {
                    Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt(prompt)
                        .default(true)
                        .interact()?
                };

                if confirm {
                    // If we're on this branch, checkout parent first
                    if is_current_branch {
                        let checkout_status = Command::new("git")
                            .args(["checkout", &parent_branch])
                            .current_dir(workdir)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();

                        if checkout_status.map(|s| s.success()).unwrap_or(false) {
                            if !quiet {
                                println!("    {} checked out {}", "→".cyan(), parent_branch.cyan());
                            }

                            // Pull latest changes for the parent branch
                            let pull_status = Command::new("git")
                                .args(["pull", "--ff-only", &remote_name, &parent_branch])
                                .current_dir(workdir)
                                .stdout(std::process::Stdio::null())
                                .stderr(std::process::Stdio::null())
                                .status();

                            if let Ok(status) = pull_status {
                                if status.success() && !quiet {
                                    println!(
                                        "    {} pulled latest {}",
                                        "↓".cyan(),
                                        parent_branch.cyan()
                                    );
                                }
                            }
                        } else {
                            if !quiet {
                                println!(
                                    "    {} {}",
                                    branch.bright_black(),
                                    "failed to checkout parent, skipping".red()
                                );
                            }
                            continue;
                        }
                    }

                    // Reparent children of this branch to its parent before deleting
                    let children: Vec<String> = stack
                        .branches
                        .iter()
                        .filter(|(_, info)| info.parent.as_deref() == Some(branch))
                        .map(|(name, _)| name.clone())
                        .collect();

                    for child in &children {
                        if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
                            // Set parent_branch_revision to empty to force needs_restack
                            let updated_meta = BranchMetadata {
                                parent_branch_name: parent_branch.clone(),
                                parent_branch_revision: String::new(), // Forces needs_restack
                                ..child_meta.clone()
                            };
                            updated_meta.write(repo.inner(), child)?;

                            // Update PR base on GitHub if this branch has a PR
                            if let Some(pr_info) = &child_meta.pr_info {
                                if let Some((rt, client)) = &github_client {
                                    match rt.block_on(
                                        client.update_pr_base(pr_info.number, &parent_branch),
                                    ) {
                                        Ok(()) => {
                                            if !quiet {
                                                println!(
                                                    "    {} updated PR #{} base → {}",
                                                    "↪".cyan(),
                                                    pr_info.number,
                                                    parent_branch.cyan()
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            // Log warning but don't fail - PR might already be closed/merged
                                            if !quiet {
                                                println!(
                                                    "    {} couldn't update PR #{} base: {}",
                                                    "⚠".yellow(),
                                                    pr_info.number,
                                                    e
                                                );
                                            }
                                        }
                                    }
                                }
                            }

                            if !quiet {
                                println!(
                                    "    {} reparented {} → {}",
                                    "↪".cyan(),
                                    child.cyan(),
                                    parent_branch.cyan()
                                );
                            }
                        }
                    }

                    // Delete local branch (force delete since we confirmed)
                    let local_output = Command::new("git")
                        .args(["branch", "-D", branch])
                        .current_dir(workdir)
                        .output();

                    let (local_deleted, local_worktree_blocked) = match local_output {
                        Ok(out) => {
                            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                            (out.status.success(), stderr.contains("used by worktree"))
                        }
                        Err(_) => (false, false),
                    };

                    // Delete remote branch
                    let remote_status = Command::new("git")
                        .args(["push", &remote_name, "--delete", branch])
                        .current_dir(workdir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();

                    let remote_deleted = remote_status.map(|s| s.success()).unwrap_or(false);

                    // Only delete metadata if branch no longer exists locally.
                    let local_ref = format!("refs/heads/{}", branch);
                    let local_still_exists = Command::new("git")
                        .args(["show-ref", "--verify", "--quiet", &local_ref])
                        .current_dir(workdir)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(true);

                    let metadata_deleted = if !local_still_exists {
                        let _ = crate::git::refs::delete_metadata(repo.inner(), branch);
                        true
                    } else {
                        false
                    };

                    if !quiet {
                        if local_deleted && remote_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (local + remote)".green()
                            );
                        } else if local_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (local only)".green()
                            );
                        } else if remote_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (remote only)".green()
                            );
                            if !metadata_deleted {
                                println!(
                                    "    {} {}",
                                    "↷".yellow(),
                                    "local branch still exists, metadata kept".dimmed()
                                );
                            }
                        } else {
                            if local_worktree_blocked {
                                println!(
                                    "    {} {}",
                                    branch.bright_black(),
                                    "not deleted locally (checked out in another worktree)"
                                        .yellow()
                                );
                            } else {
                                println!("    {} {}", branch.bright_black(), "skipped".dimmed());
                            }
                            if !metadata_deleted {
                                println!(
                                    "    {} {}",
                                    "↷".yellow(),
                                    "metadata kept because local branch still exists".dimmed()
                                );
                            }
                        }
                    }
                } else if !quiet {
                    println!("    {} {}", branch.bright_black(), "skipped".dimmed());
                }
            }
        } else if !quiet {
            println!("  {}", "No merged branches to delete.".dimmed());
        }
    }

    // Re-check current branch since it may have changed during branch deletion
    let current_after_deletions = repo.current_branch()?;

    // If we deferred trunk update (refspec fetch failed while not on trunk) and we're
    // now on trunk after branch deletions, retry with git pull which is more reliable
    if trunk_update_deferred && current_after_deletions == stack.trunk {
        if !quiet {
            print!("  Updating {}... ", stack.trunk.cyan());
            let _ = std::io::stdout().flush();
        }

        let output = Command::new("git")
            .args(["pull", "--ff-only", &remote_name, &stack.trunk])
            .current_dir(workdir)
            .output()
            .context("Failed to pull trunk")?;

        if output.status.success() {
            if !quiet {
                println!("{}", "done".green());
                if verbose {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    if !stdout.trim().is_empty() {
                        for line in stdout.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        } else if safe {
            if !quiet {
                println!("{}", "failed (safe mode, no reset)".yellow());
                if verbose {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        } else {
            // Try reset to remote
            let reset_output = Command::new("git")
                .args([
                    "reset",
                    "--hard",
                    &format!("{}/{}", remote_name, stack.trunk),
                ])
                .current_dir(workdir)
                .output()
                .context("Failed to reset trunk")?;

            if !quiet {
                if reset_output.status.success() {
                    println!("{}", "reset to remote".yellow());
                } else {
                    println!("{}", "failed".red());
                    if verbose {
                        let stderr = String::from_utf8_lossy(&reset_output.stderr);
                        if !stderr.trim().is_empty() {
                            for line in stderr.lines() {
                                println!("    {}", line.dimmed());
                            }
                        }
                    }
                }
            }
        }
    }

    // 4. Optionally restack
    if restack {
        if !quiet {
            println!();
            println!("{}", "Restacking...".bold());
        }

        // Scope restacking to the stack we started on, even if sync switched branches
        // (for example, if the current branch was deleted after merge).
        let scope_order: Vec<String> =
            if current != stack.trunk && stack.branches.contains_key(&current) {
                stack.current_stack(&current)
            } else {
                Vec::new()
            };
        // Reload stack to use fresh metadata after sync/deletion steps.
        let restack_stack = Stack::load(&repo)?;
        let branches_to_restack: Vec<String> = scope_order
            .into_iter()
            .filter(|branch| {
                restack_stack
                    .branches
                    .get(branch)
                    .map(|br| br.needs_restack)
                    .unwrap_or(false)
            })
            .collect();

        if branches_to_restack.is_empty() {
            if !quiet {
                println!("  {}", "All branches up to date.".dimmed());
            }
        } else {
            // Begin transaction for restack phase
            let mut tx = Transaction::begin(OpKind::SyncRestack, &repo, quiet)?;
            tx.plan_branches(&repo, &branches_to_restack)?;
            let restack_count = branches_to_restack.len();
            let summary = PlanSummary {
                branches_to_rebase: restack_count,
                branches_to_push: 0,
                description: vec![format!(
                    "Sync restack {} {}",
                    restack_count,
                    if restack_count == 1 {
                        "branch"
                    } else {
                        "branches"
                    }
                )],
            };
            tx::print_plan(tx.kind(), &summary, quiet);
            tx.set_plan_summary(summary);
            tx.snapshot()?;

            let mut summary: Vec<(String, String)> = Vec::new();

            for branch in &branches_to_restack {
                if !quiet {
                    print!("  Restacking {}... ", branch.cyan());
                }

                let meta = match BranchMetadata::read(repo.inner(), branch)? {
                    Some(meta) => meta,
                    None => continue,
                };

                match repo.rebase_branch_onto(branch, &meta.parent_branch_name, auto_stash_pop)? {
                    RebaseResult::Success => {
                        let parent_commit = repo.branch_commit(&meta.parent_branch_name)?;
                        let updated_meta = BranchMetadata {
                            parent_branch_revision: parent_commit,
                            ..meta
                        };
                        updated_meta.write(repo.inner(), branch)?;

                        // Record after-OID
                        tx.record_after(&repo, branch)?;

                        if !quiet {
                            println!("{}", "done".green());
                        }
                        summary.push((branch.clone(), "ok".to_string()));
                    }
                    RebaseResult::Conflict => {
                        if !quiet {
                            println!("{}", "conflict".yellow());
                            println!("  {}", "Resolve conflicts and run:".yellow());
                            println!("    {}", "stax continue".cyan());
                            println!("    {}", "stax sync --continue".cyan());
                        }
                        if stashed && !quiet {
                            println!("{}", "Stash kept to avoid conflicts.".yellow());
                        }
                        summary.push((branch.clone(), "conflict".to_string()));

                        // Finish transaction with error
                        tx.finish_err("Rebase conflict", Some("restack"), Some(branch))?;

                        return Ok(());
                    }
                }
            }

            repo.checkout(&current_after_deletions)?;

            // Finish transaction successfully
            tx.finish_ok()?;

            if !quiet && !summary.is_empty() {
                println!();
                println!("{}", "Restack summary:".dimmed());
                for (branch, status) in &summary {
                    let symbol = if status == "ok" { "✓" } else { "✗" };
                    println!("  {} {} {}", symbol, branch, status);
                }
            }
        }
    }

    if stashed {
        repo.stash_pop()?;
        if !quiet {
            println!("{}", "✓ Restored stashed changes.".green());
        }
    }

    // Refresh CI cache in background (non-blocking for user experience)
    let git_dir = repo.git_dir()?;
    let branches: Vec<String> = stack.branches.keys().cloned().collect();
    refresh_ci_cache(&repo, &config, &stack, &branches, git_dir);

    if !quiet {
        println!();
        println!("{}", "Sync complete!".green().bold());
    }

    Ok(())
}

/// Find branches that have been merged into trunk or are orphaned (no longer exist locally/remotely)
fn find_merged_branches(
    workdir: &std::path::Path,
    stack: &Stack,
    remote_name: &str,
) -> Result<Vec<String>> {
    let mut merged = Vec::new();
    let remote_trunk_ref = format!("{}/{}", remote_name, stack.trunk);

    // Method 1: git branch --merged (finds local branches merged into trunk)
    let output = Command::new("git")
        .args(["branch", "--merged", &stack.trunk])
        .current_dir(workdir)
        .output()
        .context("Failed to list merged branches")?;

    let merged_output = String::from_utf8_lossy(&output.stdout);

    for line in merged_output.lines() {
        let branch = line.trim().trim_start_matches("* ");

        // Skip trunk itself and any non-tracked branches
        if branch == stack.trunk || branch.is_empty() {
            continue;
        }

        // Only include branches we're tracking
        if stack.branches.contains_key(branch) {
            merged.push(branch.to_string());
        }
    }

    // Method 1b: git branch --merged origin/trunk (handles stale/diverged local trunk)
    let output = Command::new("git")
        .args(["branch", "--merged", &remote_trunk_ref])
        .current_dir(workdir)
        .output();

    if let Ok(output) = output {
        let merged_output = String::from_utf8_lossy(&output.stdout);

        for line in merged_output.lines() {
            let branch = line.trim().trim_start_matches("* ");

            // Skip trunk itself and any non-tracked branches
            if branch == stack.trunk || branch.is_empty() {
                continue;
            }

            // Only include branches we're tracking (and avoid duplicates)
            if stack.branches.contains_key(branch) && !merged.iter().any(|b| b == branch) {
                merged.push(branch.to_string());
            }
        }
    }

    // Method 2: Check PR state from metadata - if PR is merged, branch should be deleted
    for (branch, info) in &stack.branches {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

        // Check if PR state is "merged" (case-insensitive)
        if matches!(
            info.pr_state.as_deref(),
            Some(state) if state.eq_ignore_ascii_case("merged")
        ) {
            merged.push(branch.clone());
        }
    }

    // Method 3: Check if branch has empty diff against trunk (catches squash/rebase merges)
    // First get list of local branches to avoid diffing non-existent branches
    let local_output = Command::new("git")
        .args(["branch", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches")?;

    let local_branches: std::collections::HashSet<String> =
        String::from_utf8_lossy(&local_output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .collect();

    for branch in stack.branches.keys() {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

        // Skip if branch doesn't exist locally (will be caught by orphan check)
        if !local_branches.contains(branch) {
            continue;
        }

        // Check if branch has any changes vs trunk
        let diff_output = Command::new("git")
            .args(["diff", "--quiet", &stack.trunk, branch])
            .current_dir(workdir)
            .stderr(std::process::Stdio::null())
            .status();

        // --quiet returns 0 if no diff, 1 if there are differences
        if let Ok(status) = diff_output {
            if status.success() {
                // No diff = branch is effectively merged
                merged.push(branch.clone());
            }
        }
    }

    // Method 3b: Empty diff against origin/trunk (handles stale/diverged local trunk)
    for branch in stack.branches.keys() {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

        // Skip if branch doesn't exist locally (will be caught by orphan check)
        if !local_branches.contains(branch) {
            continue;
        }

        let diff_output = Command::new("git")
            .args(["diff", "--quiet", &remote_trunk_ref, branch])
            .current_dir(workdir)
            .stderr(std::process::Stdio::null())
            .status();

        if let Ok(status) = diff_output {
            if status.success() {
                merged.push(branch.clone());
            }
        }
    }

    // Method 4: Check if remote branch was deleted (GitHub deletes branch after merge)
    // Get list of remote branches
    let remote_output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list remote branches")?;

    let remote_branches: std::collections::HashSet<String> =
        String::from_utf8_lossy(&remote_output.stdout)
            .lines()
            .map(|s| s.trim().to_string())
            .collect();

    for (branch, info) in &stack.branches {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

        // Only consider "remote deleted" if branch had a PR before (was pushed)
        // This prevents false positives for branches that were never pushed
        if info.pr_number.is_none() {
            continue;
        }

        // Check if remote branch was deleted (strong signal it was merged)
        let remote_ref = format!("{}/{}", remote_name, branch);
        if !remote_branches.contains(&remote_ref) {
            // Remote branch doesn't exist and had a PR - likely merged and deleted
            merged.push(branch.clone());
        }
    }

    // Method 5: Find orphaned branches (tracked but no longer exist locally or remotely)
    // Reuse local_branches from Method 3, remote_branches from Method 4
    for branch in stack.branches.keys() {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

        let local_exists = local_branches.contains(branch);
        let remote_ref = format!("{}/{}", remote_name, branch);
        let remote_exists = remote_branches.contains(&remote_ref);

        // If branch doesn't exist locally AND doesn't exist remotely, it's orphaned
        if !local_exists && !remote_exists {
            merged.push(branch.clone());
        }
    }

    Ok(merged)
}

/// Refresh CI cache by fetching latest CI states from GitHub
fn refresh_ci_cache(
    repo: &GitRepo,
    config: &Config,
    stack: &Stack,
    branches: &[String],
    git_dir: &std::path::Path,
) {
    let remote_info = match RemoteInfo::from_repo(repo, config) {
        Ok(info) => info,
        Err(_) => return,
    };

    if Config::github_token().is_none() {
        return;
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };

    let client = match rt.block_on(async {
        GitHubClient::new(
            remote_info.owner(),
            &remote_info.repo,
            remote_info.api_base_url.clone(),
        )
    }) {
        Ok(client) => client,
        Err(_) => return,
    };

    let mut cache = CiCache::load(git_dir);

    for branch in branches {
        let has_pr = stack
            .branches
            .get(branch)
            .and_then(|b| b.pr_number)
            .is_some();

        if !has_pr {
            continue;
        }

        let sha = match repo.branch_commit(branch) {
            Ok(sha) => sha,
            Err(_) => continue,
        };

        let state = rt
            .block_on(async { client.combined_status_state(&sha).await })
            .ok()
            .flatten();

        cache.update(branch, state, None);
    }

    cache.mark_refreshed();
    cache.cleanup(branches);
    let _ = cache.save(git_dir);
}

/// Record CI history for merged branches before they are deleted
fn record_ci_history_for_merged(
    repo: &GitRepo,
    rt: &tokio::runtime::Runtime,
    client: &GitHubClient,
    merged_branches: &[String],
    stack: &Stack,
    quiet: bool,
) {
    // Only process branches that still exist locally (can get their commit SHA)
    let branches_to_check: Vec<String> = merged_branches
        .iter()
        .filter(|b| repo.branch_commit(b).is_ok())
        .cloned()
        .collect();

    if branches_to_check.is_empty() {
        return;
    }

    if !quiet {
        print!("  Recording CI history for merged branches... ");
        let _ = std::io::stdout().flush();
    }

    // Fetch CI statuses for merged branches
    match fetch_ci_statuses(repo, rt, client, stack, &branches_to_check) {
        Ok(statuses) => {
            // Record the CI history
            record_ci_history(repo, &statuses);

            if !quiet {
                println!("{}", "done".green());
            }
        }
        Err(_) => {
            if !quiet {
                println!("{}", "skipped (couldn't fetch)".dimmed());
            }
        }
    }
}
