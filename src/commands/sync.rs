use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::commands::restack_conflict::{print_restack_conflict, RestackConflictContext};
use crate::commands::restack_parent::normalize_scope_parents_for_restack;
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::github::GitHubClient;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use crate::remote::RemoteInfo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::process::Command;
use std::time::{Duration, Instant};

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
#[allow(clippy::too_many_arguments)]
pub fn run(
    restack: bool,
    prune: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    auto_stash_pop: bool,
) -> Result<()> {
    let sync_started_at = Instant::now();
    let mut step_timings: Vec<(String, Duration)> = Vec::new();

    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let reopen_repo_path = repo.git_dir()?.to_path_buf();
    let config = Config::load()?;
    let remote_name = config.remote_name().to_string();
    let remote_trunk_ref = format!("{}/{}", remote_name, stack.trunk);

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
            let stash_started_at = Instant::now();
            stashed = repo.stash_push()?;
            step_timings.push(("stash working tree".to_string(), stash_started_at.elapsed()));
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
    let fetch_timer = LiveTimer::maybe_new(!quiet, &format!("Fetch {}", remote_name));

    let fetch_started_at = Instant::now();
    let fetch_args: Vec<&str> = if prune {
        vec!["fetch", "--prune", "--no-tags", &remote_name]
    } else {
        vec!["fetch", "--no-tags", &remote_name]
    };
    let output = Command::new("git")
        .args(&fetch_args)
        .current_dir(&workdir)
        .output()
        .context("Failed to fetch")?;
    step_timings.push((format!("fetch {}", remote_name), fetch_started_at.elapsed()));

    if output.status.success() {
        LiveTimer::maybe_finish_timed(fetch_timer);
        if !quiet && verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                for line in stderr.lines() {
                    println!("    {}", line.dimmed());
                }
            }
        }
    } else {
        // Fetch may fail partially (lock files, etc.) but still update most refs
        LiveTimer::maybe_finish_warn(fetch_timer, "done (with warnings)");
        if !quiet && verbose {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.trim().is_empty() {
                for line in stderr.lines() {
                    println!("    {}", line.dimmed());
                }
            }
        }
    }

    // 2. Update trunk branch (before merged branch detection, so detection works correctly)
    // Note: If we're not on trunk, we use a refspec fetch which may fail if local trunk
    // has diverged. This is fine - we'll retry after branch deletions if we end up on trunk.
    let was_on_trunk = current == stack.trunk;
    let mut trunk_update_deferred = false;
    let update_trunk_started_at = Instant::now();

    if was_on_trunk {
        // We're on trunk - pull directly
        let update_timer = LiveTimer::maybe_new(!quiet, &format!("Update {}", stack.trunk));

        let output = Command::new("git")
            .args(["merge", "--ff-only", &remote_trunk_ref])
            .current_dir(&workdir)
            .output()
            .context("Failed to fast-forward trunk")?;

        if output.status.success() {
            LiveTimer::maybe_finish_timed(update_timer);
            if !quiet && verbose {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    for line in stdout.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else if safe {
            LiveTimer::maybe_finish_warn(update_timer, "failed (safe mode, no reset)");
            if !quiet && verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else {
            // Try reset to remote
            let reset_output = Command::new("git")
                .args(["reset", "--hard", &remote_trunk_ref])
                .current_dir(&workdir)
                .output()
                .context("Failed to reset trunk")?;

            if reset_output.status.success() {
                LiveTimer::maybe_finish_warn(update_timer, "reset to remote");
            } else {
                LiveTimer::maybe_finish_err(update_timer, "failed");
                if !quiet && verbose {
                    let stderr = String::from_utf8_lossy(&reset_output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        }
    } else {
        let update_timer = LiveTimer::maybe_new(!quiet, &format!("Update {}", stack.trunk));

        if let Some(trunk_worktree_path) = repo.branch_worktree_path(&stack.trunk)? {
            let output = Command::new("git")
                .args(["merge", "--ff-only", &remote_trunk_ref])
                .current_dir(&trunk_worktree_path)
                .output()
                .context("Failed to fast-forward trunk in its worktree")?;

            if output.status.success() {
                LiveTimer::maybe_finish_timed(update_timer);
            } else if safe {
                LiveTimer::maybe_finish_warn(update_timer, "failed (safe mode, no reset)");
                if !quiet && verbose {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            } else {
                let reset_output = Command::new("git")
                    .args(["reset", "--hard", &remote_trunk_ref])
                    .current_dir(&trunk_worktree_path)
                    .output()
                    .context("Failed to reset trunk in its worktree")?;

                if reset_output.status.success() {
                    LiveTimer::maybe_finish_warn(update_timer, "reset to remote");
                } else {
                    LiveTimer::maybe_finish_err(update_timer, "failed");
                    if !quiet && verbose {
                        let stderr = String::from_utf8_lossy(&reset_output.stderr);
                        if !stderr.trim().is_empty() {
                            for line in stderr.lines() {
                                println!("    {}", line.dimmed());
                            }
                        }
                    }
                }
            }
        } else {
            // Trunk isn't checked out in any worktree.
            // Resolve the two SHAs so we can give an accurate status message.
            let local_sha = Command::new("git")
                .args(["rev-parse", &stack.trunk])
                .current_dir(&workdir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

            let remote_sha = Command::new("git")
                .args(["rev-parse", &remote_trunk_ref])
                .current_dir(&workdir)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

            match (local_sha, remote_sha) {
                (Some(ref local), Some(ref remote)) if local == remote => {
                    // Already up to date — nothing to do.
                    LiveTimer::maybe_finish_timed(update_timer);
                }
                (Some(_), Some(_)) => {
                    // Check if a fast-forward is safe (local trunk is an ancestor of remote).
                    let ff_possible = Command::new("git")
                        .args([
                            "merge-base",
                            "--is-ancestor",
                            &stack.trunk,
                            &remote_trunk_ref,
                        ])
                        .current_dir(&workdir)
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false);

                    if ff_possible {
                        let output = Command::new("git")
                            .args([
                                "update-ref",
                                &format!("refs/heads/{}", stack.trunk),
                                &format!("refs/remotes/{}/{}", remote_name, stack.trunk),
                            ])
                            .current_dir(&workdir)
                            .output()
                            .context("Failed to fast-forward local trunk ref")?;

                        if output.status.success() {
                            LiveTimer::maybe_finish_timed(update_timer);
                        } else {
                            trunk_update_deferred = true;
                            LiveTimer::maybe_finish_skipped(
                                update_timer,
                                "couldn't update — run 'stax trunk' to pull",
                            );
                        }
                    } else {
                        // Local trunk has commits not on the remote — can't fast-forward.
                        trunk_update_deferred = true;
                        LiveTimer::maybe_finish_skipped(
                            update_timer,
                            &format!(
                                "local {} has unpushed commits — run 'stax trunk' to sync",
                                stack.trunk
                            ),
                        );
                    }
                }
                _ => {
                    // Couldn't resolve one or both refs (shouldn't happen after a successful fetch).
                    trunk_update_deferred = true;
                    LiveTimer::maybe_finish_skipped(
                        update_timer,
                        "couldn't resolve ref — run 'stax trunk' to pull",
                    );
                }
            }
        }
    }
    step_timings.push((
        format!("update {}", stack.trunk),
        update_trunk_started_at.elapsed(),
    ));

    // 3. Delete merged branches
    let repo = if delete_merged {
        let detect_merged_started_at = Instant::now();
        let detect_timer = LiveTimer::maybe_new(!quiet, "Detect merged branches");
        let merged = find_merged_branches(&repo, &workdir, &stack, &remote_name)?;
        step_timings.push((
            "detect merged branches".to_string(),
            detect_merged_started_at.elapsed(),
        ));
        LiveTimer::maybe_finish_timed(detect_timer);

        let delete_merged_started_at = Instant::now();
        drop(repo);
        let repo = GitRepo::open_from_path(&reopen_repo_path)?;

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
                    "    Found {} merged {}:",
                    merged.len().to_string().cyan(),
                    branch_word
                );
                for branch in &merged {
                    println!("      {} {}", "▸".bright_black(), branch);
                }
                println!();
            }

            // Record CI history for merged branches before deleting them
            if let Some((ref rt, ref client)) = github_client {
                record_ci_history_for_merged(&repo, rt, client, &merged, &stack, quiet);
            }

            for branch in &merged {
                let is_current_branch = branch == &current;

                // Resolve parent branch for checkout/reparent.
                // Metadata can reference a deleted branch; in that case fall back to trunk.
                let recorded_parent_branch = stack
                    .branches
                    .get(branch)
                    .and_then(|b| b.parent.clone())
                    .unwrap_or_else(|| stack.trunk.clone());
                let (parent_branch, parent_fallback_from) =
                    resolve_effective_parent(&workdir, &recorded_parent_branch, &stack.trunk);
                let parent_exists_locally = local_branch_exists(&workdir, &parent_branch);

                if !quiet {
                    if let Some(missing_parent) = &parent_fallback_from {
                        println!(
                            "    {} parent {} not found locally; using {}",
                            "↪".yellow(),
                            missing_parent.yellow(),
                            parent_branch.cyan()
                        );
                    }
                }

                if !parent_exists_locally {
                    if !quiet {
                        println!(
                            "    {} {}",
                            branch.bright_black(),
                            format!(
                                "couldn't resolve a local parent branch (wanted '{}'), skipping",
                                parent_branch
                            )
                            .red()
                        );
                    }
                    continue;
                }

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
                        match checkout_branch_for_cleanup(&repo, &workdir, &parent_branch) {
                            Ok(()) => {
                                if !quiet {
                                    println!(
                                        "    {} checked out {}",
                                        "→".cyan(),
                                        parent_branch.cyan()
                                    );
                                }

                                // Pull latest changes for the parent branch
                                let pull_status = Command::new("git")
                                    .args(["pull", "--ff-only", &remote_name, &parent_branch])
                                    .current_dir(&workdir)
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
                            }
                            Err(checkout_error) => {
                                if !quiet {
                                    println!(
                                        "    {} {}",
                                        branch.bright_black(),
                                        format!(
                                            "failed to checkout '{}': {}, skipping",
                                            parent_branch, checkout_error
                                        )
                                        .red()
                                    );
                                }
                                continue;
                            }
                        }
                    }

                    // Reparent children of this branch to its parent before deleting
                    let children: Vec<String> = stack
                        .branches
                        .iter()
                        .filter(|(_, info)| info.parent.as_deref() == Some(branch))
                        .map(|(name, _)| name.clone())
                        .collect();
                    let merged_branch_tip = repo.branch_commit(branch).ok();

                    for child in &children {
                        if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
                            // Preserve the old-parent boundary so restack can run
                            // `git rebase --onto <new> <old>` precisely.
                            let old_parent_boundary = merged_branch_tip
                                .clone()
                                .unwrap_or_else(|| child_meta.parent_branch_revision.clone());

                            let updated_meta = BranchMetadata {
                                parent_branch_name: parent_branch.clone(),
                                parent_branch_revision: old_parent_boundary,
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
                        .current_dir(&workdir)
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
                        .current_dir(&workdir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();

                    let remote_deleted = remote_status.map(|s| s.success()).unwrap_or(false);

                    // Only delete metadata if branch no longer exists locally.
                    let local_still_exists = local_branch_exists(&workdir, branch);

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
                                if let Ok(Some(hint)) = repo.branch_delete_resolution_hint(branch) {
                                    println!(
                                        "    {} {}",
                                        "↷".yellow(),
                                        format!("To remove it, {}", hint).dimmed()
                                    );
                                }
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
            println!("    {}", "No merged branches to delete.".dimmed());
        }

        let delete_elapsed = delete_merged_started_at.elapsed();
        step_timings.push(("delete merged branches".to_string(), delete_elapsed));
        if !quiet && !merged.is_empty() {
            println!(
                "  {:<35} {}",
                "delete merged branches",
                format!("{:.3}s", delete_elapsed.as_secs_f64()).dimmed()
            );
        }
        repo
    } else {
        repo
    };

    // Re-check current branch since it may have changed during branch deletion
    let mut current_after_deletions = repo.current_branch()?;

    // 3b. Optionally delete local branches whose upstream is gone
    if delete_upstream_gone {
        let detect_gone_started_at = Instant::now();
        let detect_timer = LiveTimer::maybe_new(!quiet, "Detect upstream-gone branches");
        let gone = find_upstream_gone_branches(&workdir, &stack.trunk)?;
        step_timings.push((
            "detect upstream-gone branches".to_string(),
            detect_gone_started_at.elapsed(),
        ));
        LiveTimer::maybe_finish_timed(detect_timer);

        let delete_gone_started_at = Instant::now();

        if !gone.is_empty() {
            if !quiet {
                let branch_word = if gone.len() == 1 {
                    "branch"
                } else {
                    "branches"
                };
                println!(
                    "    Found {} upstream-gone {}:",
                    gone.len().to_string().cyan(),
                    branch_word
                );
                for branch in &gone {
                    println!("      {} {}", "▸".bright_black(), branch);
                }
                println!();
            }

            for branch in &gone {
                if !local_branch_exists(&workdir, branch) {
                    continue;
                }

                let is_current_branch = branch == &current_after_deletions;
                let fallback_parent = &stack.trunk;
                let prompt = if is_current_branch {
                    format!(
                        "Delete '{}' (upstream gone) and checkout '{}'?",
                        branch, fallback_parent
                    )
                } else {
                    format!("Delete '{}' (upstream gone)?", branch)
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

                if !confirm {
                    if !quiet {
                        println!("    {} {}", branch.bright_black(), "skipped".dimmed());
                    }
                    continue;
                }

                if is_current_branch {
                    match checkout_branch_for_cleanup(&repo, &workdir, fallback_parent) {
                        Ok(()) => {
                            current_after_deletions = fallback_parent.clone();
                            if !quiet {
                                println!(
                                    "    {} checked out {}",
                                    "→".cyan(),
                                    fallback_parent.cyan()
                                );
                            }
                        }
                        Err(checkout_error) => {
                            if !quiet {
                                println!(
                                    "    {} {}",
                                    branch.bright_black(),
                                    format!(
                                        "failed to checkout '{}': {}, skipping",
                                        fallback_parent, checkout_error
                                    )
                                    .red()
                                );
                            }
                            continue;
                        }
                    }
                }

                let local_output = Command::new("git")
                    .args(["branch", "-D", branch])
                    .current_dir(&workdir)
                    .output();

                let (local_deleted, local_worktree_blocked) = match local_output {
                    Ok(out) => {
                        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                        (out.status.success(), stderr.contains("used by worktree"))
                    }
                    Err(_) => (false, false),
                };

                // Only delete metadata if branch no longer exists locally.
                let local_still_exists = local_branch_exists(&workdir, branch);

                let metadata_deleted = if !local_still_exists {
                    let _ = crate::git::refs::delete_metadata(repo.inner(), branch);
                    true
                } else {
                    false
                };

                if !quiet {
                    if local_deleted {
                        println!(
                            "    {} {}",
                            branch.bright_black(),
                            "deleted (local only)".green()
                        );
                    } else if local_worktree_blocked {
                        println!(
                            "    {} {}",
                            branch.bright_black(),
                            "not deleted locally (checked out in another worktree)".yellow()
                        );
                        if let Ok(Some(hint)) = repo.branch_delete_resolution_hint(branch) {
                            println!(
                                "    {} {}",
                                "↷".yellow(),
                                format!("To remove it, {}", hint).dimmed()
                            );
                        }
                    } else {
                        println!("    {} {}", branch.bright_black(), "skipped".dimmed());
                    }

                    if !metadata_deleted && local_still_exists {
                        println!(
                            "    {} {}",
                            "↷".yellow(),
                            "metadata kept because local branch still exists".dimmed()
                        );
                    }
                }
            }
        } else if !quiet {
            println!("    {}", "No upstream-gone branches to delete.".dimmed());
        }

        let delete_elapsed = delete_gone_started_at.elapsed();
        step_timings.push(("delete upstream-gone branches".to_string(), delete_elapsed));
        if !quiet && !gone.is_empty() {
            println!(
                "  {:<35} {}",
                "delete upstream-gone branches",
                format!("{:.3}s", delete_elapsed.as_secs_f64()).dimmed()
            );
        }
    }

    // If we deferred trunk update (refspec fetch failed while not on trunk) and we're
    // now on trunk after branch deletions, retry with git pull which is more reliable
    if trunk_update_deferred && current_after_deletions == stack.trunk {
        let deferred_update_started_at = Instant::now();
        let deferred_timer = LiveTimer::maybe_new(!quiet, &format!("Update {}", stack.trunk));

        let output = Command::new("git")
            .args(["merge", "--ff-only", &remote_trunk_ref])
            .current_dir(&workdir)
            .output()
            .context("Failed to fast-forward trunk")?;

        if output.status.success() {
            LiveTimer::maybe_finish_timed(deferred_timer);
            if !quiet && verbose {
                let stdout = String::from_utf8_lossy(&output.stdout);
                if !stdout.trim().is_empty() {
                    for line in stdout.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else if safe {
            LiveTimer::maybe_finish_warn(deferred_timer, "failed (safe mode, no reset)");
            if !quiet && verbose {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if !stderr.trim().is_empty() {
                    for line in stderr.lines() {
                        println!("    {}", line.dimmed());
                    }
                }
            }
        } else {
            // Try reset to remote
            let reset_output = Command::new("git")
                .args(["reset", "--hard", &remote_trunk_ref])
                .current_dir(&workdir)
                .output()
                .context("Failed to reset trunk")?;

            if reset_output.status.success() {
                LiveTimer::maybe_finish_warn(deferred_timer, "reset to remote");
            } else {
                LiveTimer::maybe_finish_err(deferred_timer, "failed");
                if !quiet && verbose {
                    let stderr = String::from_utf8_lossy(&reset_output.stderr);
                    if !stderr.trim().is_empty() {
                        for line in stderr.lines() {
                            println!("    {}", line.dimmed());
                        }
                    }
                }
            }
        }

        step_timings.push((
            format!("retry update {}", stack.trunk),
            deferred_update_started_at.elapsed(),
        ));
    }

    // 4. Optionally restack
    if restack {
        let restack_started_at = Instant::now();
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
        // Normalize parents for branches whose parent was squash-merged into trunk,
        // so the rebase uses the correct --onto boundary.
        normalize_scope_parents_for_restack(&repo, &scope_order, quiet)?;

        // Reload stack to use fresh metadata after sync/deletion and normalization steps.
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
            tx.set_auto_stash_pop(auto_stash_pop);
            tx.snapshot()?;

            let mut summary: Vec<(String, String)> = Vec::new();

            for (index, branch) in branches_to_restack.iter().enumerate() {
                let restack_timer = LiveTimer::maybe_new(!quiet, &format!("Restack {}", branch));

                let meta = match BranchMetadata::read(repo.inner(), branch)? {
                    Some(meta) => meta,
                    None => continue,
                };

                match repo.rebase_branch_onto_with_provenance(
                    branch,
                    &meta.parent_branch_name,
                    &meta.parent_branch_revision,
                    auto_stash_pop,
                )? {
                    RebaseResult::Success => {
                        let parent_commit = repo.branch_commit(&meta.parent_branch_name)?;
                        let updated_meta = BranchMetadata {
                            parent_branch_revision: parent_commit,
                            ..meta
                        };
                        updated_meta.write(repo.inner(), branch)?;

                        // Record after-OID
                        tx.record_after(&repo, branch)?;

                        LiveTimer::maybe_finish_timed(restack_timer);
                        summary.push((branch.clone(), "ok".to_string()));
                    }
                    RebaseResult::Conflict => {
                        LiveTimer::maybe_finish_warn(restack_timer, "conflict");
                        let completed_branches: Vec<String> = summary
                            .iter()
                            .filter(|(_, status)| status == "ok")
                            .map(|(name, _)| name.clone())
                            .collect();
                        print_restack_conflict(
                            &repo,
                            &RestackConflictContext {
                                branch,
                                parent_branch: &meta.parent_branch_name,
                                completed_branches: &completed_branches,
                                remaining_branches: branches_to_restack
                                    .len()
                                    .saturating_sub(index + 1),
                                continue_commands: &[
                                    "stax resolve",
                                    "stax continue",
                                    "stax sync --continue",
                                ],
                            },
                        );
                        if stashed {
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

        step_timings.push(("restack".to_string(), restack_started_at.elapsed()));
    }

    if stashed {
        let stash_pop_started_at = Instant::now();
        repo.stash_pop()?;
        step_timings.push(("restore stash".to_string(), stash_pop_started_at.elapsed()));
        if !quiet {
            println!("{}", "✓ Restored stashed changes.".green());
        }
    }

    if verbose && !quiet {
        println!();
        println!("{}", "Sync timing summary:".bold());
        for (step, duration) in &step_timings {
            println!("  {:<35} {}", step, format_duration(*duration).dimmed());
        }
        println!(
            "  {:<35} {}",
            "total",
            format_duration(sync_started_at.elapsed()).cyan()
        );
    }

    if !quiet {
        println!();
        println!("{}", "Sync complete!".green().bold());
    }

    Ok(())
}

/// Find branches that have been merged into trunk or are orphaned (no longer exist locally/remotely)
fn find_merged_branches(
    repo: &GitRepo,
    workdir: &std::path::Path,
    stack: &Stack,
    remote_name: &str,
) -> Result<Vec<String>> {
    let mut merged = Vec::new();
    let remote_trunk_ref = format!("{}/{}", remote_name, stack.trunk);
    let remote_branches = repo.remote_branch_names(remote_name)?;

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

    // Method 4: Check if the tracked remote branch was deleted (GitHub deletes
    // branch after merge). This is cheaper and more robust than enumerating the
    // entire remote ref namespace in very large repos.
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
        if !remote_branches.contains(branch.as_str()) {
            // Remote branch doesn't exist and had a PR - likely merged and deleted
            merged.push(branch.clone());
        }
    }

    // Method 5: Find orphaned branches (tracked but no longer exist locally or remotely)
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

        let local_exists = local_branches.contains(branch);
        let remote_exists = remote_branches.contains(branch.as_str());

        // If branch doesn't exist locally AND doesn't exist remotely, it's orphaned
        if !local_exists && !remote_exists {
            merged.push(branch.clone());
        }
    }

    // Method 3: Patch-id provenance check — detects squash/rebase merges even
    // when trunk has advanced past the merge point (where a simple tree diff
    // would show false negatives). Run this last so cheaper signals resolve
    // most cases before the provenance path touches more refs.
    for branch in stack.branches.keys() {
        if branch == &stack.trunk || merged.contains(branch) {
            continue;
        }
        if repo
            .is_branch_merged_equivalent_to_trunk(branch)
            .unwrap_or(false)
        {
            merged.push(branch.clone());
        }
    }

    Ok(merged)
}

fn find_upstream_gone_branches(workdir: &std::path::Path, trunk: &str) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args([
            "for-each-ref",
            "--format=%(refname:short)%00%(upstream:short)%00%(upstream:track)",
            "refs/heads",
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to list local branches with upstream tracking info")?;

    let mut branches = std::collections::BTreeSet::new();
    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        let mut fields = line.split('\0');
        let branch = fields.next().unwrap_or("").trim();
        let _upstream = fields.next().unwrap_or("").trim();
        let tracking = fields.next().unwrap_or("").trim();

        if branch.is_empty() || branch == trunk {
            continue;
        }

        if tracking.contains("[gone]") {
            branches.insert(branch.to_string());
        }
    }

    Ok(branches.into_iter().collect())
}

fn local_branch_exists(workdir: &std::path::Path, branch: &str) -> bool {
    let local_ref = format!("refs/heads/{}", branch);
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &local_ref])
        .current_dir(workdir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn checkout_branch_for_cleanup(
    repo: &GitRepo,
    workdir: &std::path::Path,
    branch: &str,
) -> std::result::Result<(), String> {
    if let Ok(Some(other_worktree_path)) = repo.branch_worktree_path(branch) {
        let current_path = std::fs::canonicalize(workdir).unwrap_or_else(|_| workdir.to_path_buf());
        let other_path = std::fs::canonicalize(&other_worktree_path)
            .unwrap_or_else(|_| other_worktree_path.clone());
        if other_path != current_path {
            return Err(format!(
                "'{}' is already checked out in another worktree at '{}'",
                branch,
                other_worktree_path.display()
            ));
        }
    }

    let output = Command::new("git")
        .args(["checkout", branch])
        .current_dir(workdir)
        .output()
        .map_err(|e| format!("git checkout '{}' failed: {}", branch, e))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        Err(format!(
            "git checkout '{}' exited with {}",
            branch, output.status
        ))
    } else {
        Err(stderr)
    }
}

fn resolve_effective_parent(
    workdir: &std::path::Path,
    recorded_parent: &str,
    trunk: &str,
) -> (String, Option<String>) {
    if local_branch_exists(workdir, recorded_parent) {
        return (recorded_parent.to_string(), None);
    }

    if recorded_parent != trunk && local_branch_exists(workdir, trunk) {
        return (trunk.to_string(), Some(recorded_parent.to_string()));
    }

    (recorded_parent.to_string(), None)
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

    let ci_timer = LiveTimer::maybe_new(!quiet, "Record CI history");

    // Fetch CI statuses for merged branches
    match fetch_ci_statuses(repo, rt, client, stack, &branches_to_check) {
        Ok(statuses) => {
            record_ci_history(repo, &statuses);
            LiveTimer::maybe_finish_timed(ci_timer);
        }
        Err(_) => {
            LiveTimer::maybe_finish_warn(ci_timer, "skipped (couldn't fetch)");
        }
    }
}

fn format_duration(duration: Duration) -> String {
    format!("{:.3}s", duration.as_secs_f64())
}
