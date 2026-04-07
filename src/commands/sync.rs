use crate::commands::ci::{fetch_ci_statuses, record_ci_history};
use crate::commands::restack_conflict::{print_restack_conflict, RestackConflictContext};
use crate::commands::restack_parent::normalize_scope_parents_for_restack;
use crate::commands::worktree::{
    remove::remove_worktree_with_hooks,
    shared::{compute_worktree_details, worktree_removal_blockers},
};
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::forge::ForgeClient;
use crate::git::repo::BranchDeleteResolution;
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct SyncStats {
    trunk: Option<TrunkSummary>,
    merged_branches_cleaned: usize,
    restacked_branches: usize,
}

#[derive(Debug)]
enum TrunkSummary {
    UpToDate {
        branch: String,
    },
    Pulled {
        branch: String,
        commits: usize,
        additions: usize,
        deletions: usize,
    },
    Updated {
        branch: String,
    },
}

#[derive(Debug, Clone)]
struct BlockingWorktreeCleanup {
    resolution: BranchDeleteResolution,
    blockers: Vec<&'static str>,
}

impl BlockingWorktreeCleanup {
    fn can_remove_during_sync(&self) -> bool {
        !self.resolution.worktree.is_main && self.blockers.is_empty()
    }

    fn blocker_summary(&self) -> Option<String> {
        if self.resolution.worktree.is_main {
            return Some("it is the main worktree".to_string());
        }

        if self.blockers.is_empty() {
            return None;
        }

        let reasons = self
            .blockers
            .iter()
            .map(|blocker| match *blocker {
                "current" => "it is the current worktree",
                "dirty" => "it has uncommitted changes",
                "locked" => "it is locked",
                "rebase" => "a rebase is in progress",
                "merge" => "a merge is in progress",
                "conflicts" => "it has unresolved conflicts",
                other => other,
            })
            .collect::<Vec<_>>();

        Some(reasons.join(", "))
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct LocalBranchDeleteOutcome {
    deleted: bool,
    worktree_blocked: bool,
}

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
#[allow(clippy::too_many_arguments)]
pub fn run(
    restack: bool,
    #[allow(unused_variables)] prune: bool,
    full: bool,
    delete_merged: bool,
    delete_upstream_gone: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
    verbose: bool,
    mut auto_stash_pop: bool,
) -> Result<()> {
    let sync_started_at = Instant::now();
    let mut step_timings: Vec<(String, Duration)> = Vec::new();
    let mut stats = SyncStats::default();

    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let reopen_repo_path = repo.git_dir()?.to_path_buf();
    let trunk_branch = stack.trunk.clone();
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
            auto_stash_pop = true;
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
    // Default: trunk-only fetch + `ls-remote --heads` in parallel (fast on large repos).
    // `--full`: classic `fetch --prune --no-tags` for all remote-tracking refs.
    let fetch_timer = LiveTimer::maybe_new(!quiet, &format!("Fetch {}", remote_name));

    let fetch_started_at = Instant::now();
    let output;
    // Remote branch names for merged detection (`None` when `--no-delete`: trunk-only fetch).
    let remote_branches_for_merged: Option<HashSet<String>>;

    if full {
        let fetch_args: Vec<&str> = vec!["fetch", "--prune", "--no-tags", remote_name.as_str()];
        output = Command::new("git")
            .args(&fetch_args)
            .current_dir(&workdir)
            .output()
            .context("Failed to fetch")?;
        remote_branches_for_merged = if delete_merged {
            Some(
                repo.remote_branch_names(&remote_name)
                    .context("Failed to read remote-tracking branches after fetch")?,
            )
        } else {
            None
        };
    } else if delete_merged {
        let workdir_fetch = workdir.clone();
        let remote_fetch = remote_name.clone();
        let trunk = stack.trunk.clone();
        let workdir_ls = workdir.clone();
        let remote_ls = remote_name.clone();

        let fetch_handle = std::thread::spawn(move || {
            Command::new("git")
                .args(["fetch", "--no-tags", remote_fetch.as_str(), trunk.as_str()])
                .current_dir(&workdir_fetch)
                .output()
        });

        let ls_handle =
            std::thread::spawn(move || remote::ls_remote_heads(&workdir_ls, &remote_ls));

        output = fetch_handle
            .join()
            .map_err(|_| anyhow::anyhow!("fetch thread panicked"))?
            .context("Failed to fetch")?;

        let heads = ls_handle
            .join()
            .map_err(|_| anyhow::anyhow!("git ls-remote thread panicked"))??;
        if output.status.success() {
            prune_stale_remote_tracking_refs(&workdir, remote_name.as_str(), &stack, &heads);
        }
        remote_branches_for_merged = Some(heads);
    } else {
        output = Command::new("git")
            .args([
                "fetch",
                "--no-tags",
                remote_name.as_str(),
                stack.trunk.as_str(),
            ])
            .current_dir(&workdir)
            .output()
            .context("Failed to fetch")?;
        remote_branches_for_merged = None;
    }

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

    let local_trunk_before_sync = resolve_ref_oid(&workdir, &stack.trunk);
    let remote_trunk_after_fetch = resolve_ref_oid(&workdir, &remote_trunk_ref);

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
        let merged = find_merged_branches(
            &repo,
            &workdir,
            &stack,
            &remote_name,
            remote_branches_for_merged
                .as_ref()
                .expect("remote branch list when deleting merged branches"),
        )?;
        step_timings.push((
            "detect merged branches".to_string(),
            detect_merged_started_at.elapsed(),
        ));
        LiveTimer::maybe_finish_timed(detect_timer);

        let delete_merged_started_at = Instant::now();
        drop(repo);
        let repo = GitRepo::open_from_path(&reopen_repo_path)?;

        // Lazy-initialize forge client for updating PR bases (only if needed)
        let forge_client: Option<(tokio::runtime::Runtime, ForgeClient)> = {
            let remote_info = RemoteInfo::from_repo(&repo, &config).ok();

            if let Some(info) = remote_info {
                tokio::runtime::Runtime::new().ok().and_then(|rt| {
                    let _enter = rt.enter();
                    ForgeClient::new(&info).ok().map(|client| (rt, client))
                })
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
            if let Some((ref rt, ref client)) = forge_client {
                record_ci_history_for_merged(&repo, rt, client, &merged, &stack, quiet);
            }

            for branch in &merged {
                let is_current_branch = branch == &current;
                let blocking_worktree_cleanup = if is_current_branch {
                    None
                } else {
                    plan_blocking_worktree_cleanup(&repo, branch)?
                };

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

                let prompt = sync_delete_prompt(
                    branch,
                    if is_current_branch {
                        Some(parent_branch.as_str())
                    } else {
                        None
                    },
                    None,
                    blocking_worktree_cleanup.as_ref(),
                );

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
                            // Only use the merged branch's current tip when it is
                            // actually in the child's ancestry.  If the parent was
                            // rebased before deletion its tip may have moved out of
                            // the child's commit graph (#120).
                            let old_parent_boundary = merged_branch_tip
                                .clone()
                                .filter(|tip| repo.is_ancestor(tip, child).unwrap_or(false))
                                .unwrap_or_else(|| child_meta.parent_branch_revision.clone());

                            let updated_meta = BranchMetadata {
                                parent_branch_name: parent_branch.clone(),
                                parent_branch_revision: old_parent_boundary,
                                ..child_meta.clone()
                            };
                            updated_meta.write(repo.inner(), child)?;

                            // Update PR base on the forge if this branch has a PR
                            if let Some(pr_info) = &child_meta.pr_info {
                                if let Some((rt, client)) = &forge_client {
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

                    let local_delete = delete_local_branch_for_sync(
                        &repo,
                        &config,
                        &workdir,
                        branch,
                        blocking_worktree_cleanup.as_ref(),
                        quiet,
                    )?;
                    let local_deleted = local_delete.deleted;
                    let local_worktree_blocked = local_delete.worktree_blocked;

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

                    if metadata_deleted {
                        stats.merged_branches_cleaned += 1;
                    }

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
                                print_blocked_branch_delete_recovery(
                                    branch,
                                    blocking_worktree_cleanup.as_ref(),
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
                let blocking_worktree_cleanup = if is_current_branch {
                    None
                } else {
                    plan_blocking_worktree_cleanup(&repo, branch)?
                };
                let fallback_parent = &stack.trunk;
                let prompt = sync_delete_prompt(
                    branch,
                    if is_current_branch {
                        Some(fallback_parent.as_str())
                    } else {
                        None
                    },
                    Some("upstream gone"),
                    blocking_worktree_cleanup.as_ref(),
                );

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

                let local_delete = delete_local_branch_for_sync(
                    &repo,
                    &config,
                    &workdir,
                    branch,
                    blocking_worktree_cleanup.as_ref(),
                    quiet,
                )?;
                let local_deleted = local_delete.deleted;
                let local_worktree_blocked = local_delete.worktree_blocked;

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
                        print_blocked_branch_delete_recovery(
                            branch,
                            blocking_worktree_cleanup.as_ref(),
                        );
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
            .iter()
            .filter(|branch| {
                restack_stack
                    .branches
                    .get(branch.as_str())
                    .map(|br| br.needs_restack)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();

        if branches_to_restack.is_empty() {
            if !quiet {
                println!("  {}", "All branches up to date.".dimmed());
            }
        } else {
            // Begin transaction for restack phase
            let mut tx = Transaction::begin(OpKind::SyncRestack, &repo, quiet)?;
            tx.plan_branches(&repo, &scope_order)?;
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
            let mut restacked_branches = 0usize;

            for (index, branch) in scope_order.iter().enumerate() {
                let live_stack = Stack::load(&repo)?;
                let needs_restack = live_stack
                    .branches
                    .get(branch.as_str())
                    .map(|br| br.needs_restack)
                    .unwrap_or(false);
                if !needs_restack {
                    continue;
                }

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
                        restacked_branches += 1;
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
                                remaining_branches: scope_order.len().saturating_sub(index + 1),
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
            stats.restacked_branches = restacked_branches;

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

    stats.trunk = summarize_trunk_sync(
        &workdir,
        &trunk_branch,
        local_trunk_before_sync.as_deref(),
        remote_trunk_after_fetch.as_deref(),
    );

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
        println!(
            "{} {}",
            "Sync complete!".green().bold(),
            render_sync_footer(&stats, sync_started_at.elapsed())
        );
    }

    Ok(())
}

/// Drop stale `refs/remotes/<remote>/<branch>` for stax-tracked branches that no longer exist on the remote.
fn prune_stale_remote_tracking_refs(
    workdir: &Path,
    remote_name: &str,
    stack: &Stack,
    remote_branches: &HashSet<String>,
) {
    for branch in stack.branches.keys() {
        if branch == &stack.trunk {
            continue;
        }
        if remote_branches.contains(branch.as_str()) {
            continue;
        }
        let refname = format!("refs/remotes/{}/{}", remote_name, branch);
        let _ = Command::new("git")
            .args(["update-ref", "-d", &refname])
            .current_dir(workdir)
            .status();
    }
}

fn find_merged_branches(
    repo: &GitRepo,
    workdir: &std::path::Path,
    stack: &Stack,
    remote_name: &str,
    remote_branches: &HashSet<String>,
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

    // Method 2: Check PR state from metadata.
    // Only an explicitly merged PR is a strong enough signal for cleanup here.
    // Closed-but-unmerged PRs must be preserved unless some other merge/deletion
    // heuristic below proves the branch is safe to clean up.
    for (branch, info) in &stack.branches {
        // Skip trunk
        if branch == &stack.trunk {
            continue;
        }

        // Skip if already in merged list
        if merged.contains(branch) {
            continue;
        }

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
    let trunk = stack.trunk.as_str();
    let mut need_patch_id: Vec<(String, String)> = Vec::new();

    for branch in stack.branches.keys() {
        if branch == &stack.trunk || merged.contains(branch) {
            continue;
        }
        // Remote still exists -> not merged via squash-delete; skip expensive check.
        if remote_branches.contains(branch.as_str()) {
            continue;
        }
        match repo.is_branch_merged_cheap(branch) {
            Ok(Some(())) => merged.push(branch.clone()),
            Ok(None) => {
                if let Ok(mb) = repo.merge_base(trunk, branch) {
                    need_patch_id.push((branch.clone(), mb));
                }
            }
            Err(_) => {}
        }
    }

    if !need_patch_id.is_empty() {
        let mut by_merge_base: HashMap<String, Vec<String>> = HashMap::new();
        for (branch, mb) in need_patch_id {
            by_merge_base.entry(mb).or_default().push(branch);
        }

        for (merge_base, branches) in by_merge_base {
            let trunk_range = format!("{}..{}", merge_base, trunk);
            let trunk_count = match repo.rev_list_count(workdir, &trunk_range) {
                Ok(c) => c,
                Err(_) => {
                    for branch in branches {
                        if repo
                            .is_branch_merged_equivalent_to_trunk(&branch)
                            .unwrap_or(false)
                        {
                            merged.push(branch);
                        }
                    }
                    continue;
                }
            };

            if trunk_count > GitRepo::PATCH_ID_TRUNK_COMMIT_CAP {
                for branch in branches {
                    if repo
                        .is_branch_merged_equivalent_to_trunk(&branch)
                        .unwrap_or(false)
                    {
                        merged.push(branch);
                    }
                }
                continue;
            }

            let trunk_patch_ids = match repo.patch_ids_for_range(workdir, &trunk_range) {
                Ok(ids) => ids,
                Err(_) => {
                    for branch in branches {
                        if repo
                            .is_branch_merged_equivalent_to_trunk(&branch)
                            .unwrap_or(false)
                        {
                            merged.push(branch);
                        }
                    }
                    continue;
                }
            };

            for branch in branches {
                let branch_range = format!("{}..{}", merge_base, branch);
                let branch_patch_ids = match repo.patch_ids_for_range(workdir, &branch_range) {
                    Ok(ids) => ids,
                    Err(_) => continue,
                };
                if branch_patch_ids.is_empty() || branch_patch_ids.is_subset(&trunk_patch_ids) {
                    merged.push(branch);
                }
            }
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

fn plan_blocking_worktree_cleanup(
    repo: &GitRepo,
    branch: &str,
) -> Result<Option<BlockingWorktreeCleanup>> {
    let Some(resolution) = repo.branch_delete_resolution(branch)? else {
        return Ok(None);
    };

    if resolution.worktree.is_main {
        return Ok(Some(BlockingWorktreeCleanup {
            resolution,
            blockers: Vec::new(),
        }));
    }

    let details = compute_worktree_details(repo, resolution.worktree.clone())?;
    Ok(Some(BlockingWorktreeCleanup {
        resolution,
        blockers: worktree_removal_blockers(&details),
    }))
}

fn delete_local_branch_for_sync(
    repo: &GitRepo,
    config: &Config,
    workdir: &std::path::Path,
    branch: &str,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
    quiet: bool,
) -> Result<LocalBranchDeleteOutcome> {
    let mut outcome = attempt_local_branch_delete(workdir, branch);
    if outcome.deleted || !outcome.worktree_blocked {
        return Ok(outcome);
    }

    let Some(cleanup) = blocking_worktree_cleanup else {
        return Ok(outcome);
    };

    if !cleanup.can_remove_during_sync() {
        return Ok(outcome);
    }

    let removed_worktree =
        remove_worktree_with_hooks(repo, config, &cleanup.resolution.worktree, false);
    match removed_worktree {
        Ok(display_name) => {
            if !quiet {
                println!(
                    "    {} removed linked worktree {}",
                    "→".cyan(),
                    display_name.cyan()
                );
            }
            outcome = attempt_local_branch_delete(workdir, branch);
            Ok(outcome)
        }
        Err(error) => {
            if !quiet {
                println!(
                    "    {} {}",
                    "↷".yellow(),
                    format!(
                        "couldn't remove linked worktree '{}': {}",
                        cleanup.resolution.worktree.name, error
                    )
                    .dimmed()
                );
            }
            Ok(outcome)
        }
    }
}

fn attempt_local_branch_delete(
    workdir: &std::path::Path,
    branch: &str,
) -> LocalBranchDeleteOutcome {
    let local_output = Command::new("git")
        .args(["branch", "-D", branch])
        .current_dir(workdir)
        .output();

    match local_output {
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            LocalBranchDeleteOutcome {
                deleted: out.status.success(),
                worktree_blocked: stderr.contains("used by worktree"),
            }
        }
        Err(_) => LocalBranchDeleteOutcome::default(),
    }
}

fn sync_delete_prompt(
    branch: &str,
    checkout_target: Option<&str>,
    reason: Option<&str>,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
) -> String {
    if let Some(target) = checkout_target {
        if let Some(reason) = reason {
            return format!("Delete '{}' ({reason}) and checkout '{}'?", branch, target);
        }

        return format!("Delete '{}' and checkout '{}'?", branch, target);
    }

    if let Some(cleanup) = blocking_worktree_cleanup {
        if cleanup.can_remove_during_sync() {
            if let Some(reason) = reason {
                return format!(
                    "Delete '{}' ({reason}) and remove linked worktree '{}'?",
                    branch, cleanup.resolution.worktree.name
                );
            }

            return format!(
                "Delete '{}' and remove linked worktree '{}'?",
                branch, cleanup.resolution.worktree.name
            );
        }

        if let Some(reason) = reason {
            return format!(
                "Delete '{}' ({reason}; keep linked worktree '{}')?",
                branch, cleanup.resolution.worktree.name
            );
        }

        return format!(
            "Delete '{}' (keep linked worktree '{}')?",
            branch, cleanup.resolution.worktree.name
        );
    }

    if let Some(reason) = reason {
        format!("Delete '{}' ({reason})?", branch)
    } else {
        format!("Delete '{}'?", branch)
    }
}

fn print_blocked_branch_delete_recovery(
    branch: &str,
    blocking_worktree_cleanup: Option<&BlockingWorktreeCleanup>,
) {
    println!(
        "    {} {}",
        branch.bright_black(),
        "not deleted locally (checked out in another worktree)".yellow()
    );
    if let Some(cleanup) = blocking_worktree_cleanup {
        if let Some(reason) = cleanup.blocker_summary() {
            println!(
                "    {} {}",
                "↷".yellow(),
                format!(
                    "sync kept linked worktree '{}' because {}",
                    cleanup.resolution.worktree.name, reason
                )
                .dimmed()
            );
        }

        let resolution = &cleanup.resolution;
        if let Some(remove_cmd) = resolution.remove_worktree_and_branch_cmd() {
            println!(
                "    {} {}",
                "↷".yellow(),
                "Run to remove that worktree and delete the branch:".dimmed()
            );
            println!("      {}", remove_cmd.cyan());
        }
        println!(
            "    {} {}",
            "↷".yellow(),
            if resolution.worktree.is_main {
                "Run to free the branch in the main worktree:".dimmed()
            } else {
                "Or keep the worktree and free the branch:".dimmed()
            }
        );
        println!("      {}", resolution.switch_branch_cmd().cyan());
    }
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
    client: &ForgeClient,
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

fn render_sync_footer(stats: &SyncStats, total_duration: Duration) -> String {
    let mut parts = Vec::new();

    if let Some(trunk) = &stats.trunk {
        match trunk {
            TrunkSummary::UpToDate { branch } => {
                parts.push(format!(
                    "{} {}",
                    branch.cyan().bold(),
                    "up to date".dimmed()
                ));
            }
            TrunkSummary::Pulled {
                branch,
                commits,
                additions,
                deletions,
            } => {
                parts.push(format!(
                    "{} {}",
                    branch.cyan().bold(),
                    format!(
                        "+{} commit{}",
                        commits,
                        if *commits == 1 { "" } else { "s" }
                    )
                    .green()
                ));
                parts.push(format!(
                    "{} {}",
                    format!("+{}", additions).green(),
                    format!("-{}", deletions).red()
                ));
            }
            TrunkSummary::Updated { branch } => {
                parts.push(format!("{} {}", branch.cyan().bold(), "updated".yellow()));
            }
        }
    }

    if stats.merged_branches_cleaned > 0 {
        parts.push(format!(
            "{} {} {}",
            "cleaned".dimmed(),
            stats.merged_branches_cleaned.to_string().cyan().bold(),
            "merged".dimmed()
        ));
    }

    if stats.restacked_branches > 0 {
        parts.push(format!(
            "{} {}",
            "restacked".dimmed(),
            stats.restacked_branches.to_string().cyan().bold()
        ));
    }

    parts.push(format!("{}", format_duration(total_duration).cyan()));
    parts.join(&format!("{}", " | ".dimmed()))
}

fn summarize_trunk_sync(
    workdir: &Path,
    branch: &str,
    local_before: Option<&str>,
    remote_after_fetch: Option<&str>,
) -> Option<TrunkSummary> {
    let local_after = resolve_ref_oid(workdir, branch)?;

    if let Some(remote_after_fetch) = remote_after_fetch {
        if local_after == remote_after_fetch {
            if let Some(local_before) = local_before {
                if local_before == local_after {
                    return Some(TrunkSummary::UpToDate {
                        branch: branch.to_string(),
                    });
                }

                if is_ancestor(workdir, local_before, &local_after) {
                    let commits = count_commits_between(workdir, local_before, &local_after)?;
                    let (additions, deletions) =
                        diff_line_stats_between(workdir, local_before, &local_after)?;
                    return Some(TrunkSummary::Pulled {
                        branch: branch.to_string(),
                        commits,
                        additions,
                        deletions,
                    });
                }
            }

            return Some(TrunkSummary::Updated {
                branch: branch.to_string(),
            });
        }
    }

    None
}

fn resolve_ref_oid(workdir: &Path, reference: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", reference])
        .current_dir(workdir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_ancestor(workdir: &Path, ancestor: &str, descendant: &str) -> bool {
    Command::new("git")
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .current_dir(workdir)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn count_commits_between(workdir: &Path, base: &str, head: &str) -> Option<usize> {
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", base, head)])
        .current_dir(workdir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

fn diff_line_stats_between(workdir: &Path, base: &str, head: &str) -> Option<(usize, usize)> {
    let output = Command::new("git")
        .args(["diff", "--numstat", &format!("{}..{}", base, head)])
        .current_dir(workdir)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut additions = 0usize;
    let mut deletions = 0usize;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 2 {
            if let Ok(add) = parts[0].parse::<usize>() {
                additions += add;
            }
            if let Ok(del) = parts[1].parse::<usize>() {
                deletions += del;
            }
        }
    }

    Some((additions, deletions))
}

#[cfg(test)]
mod tests {
    use super::*;
    use colored::control;
    use std::path::PathBuf;

    fn linked_worktree_cleanup(blockers: &[&'static str]) -> BlockingWorktreeCleanup {
        BlockingWorktreeCleanup {
            resolution: crate::git::repo::BranchDeleteResolution {
                worktree: crate::git::repo::WorktreeInfo {
                    name: "review-pass".to_string(),
                    path: PathBuf::from("/tmp/review-pass"),
                    branch: Some("cesar/review-pass".to_string()),
                    is_main: false,
                    is_current: false,
                    is_locked: false,
                    lock_reason: None,
                    is_prunable: false,
                    prunable_reason: None,
                },
                remove_worktree_selector: "cesar/review-pass".to_string(),
                switch_target: crate::git::repo::BranchDeleteSwitchTarget::Detach,
            },
            blockers: blockers.to_vec(),
        }
    }

    #[test]
    fn render_sync_footer_is_colored_and_compact() {
        control::set_override(true);

        let footer = render_sync_footer(
            &SyncStats {
                trunk: Some(TrunkSummary::Pulled {
                    branch: "main".to_string(),
                    commits: 1,
                    additions: 434,
                    deletions: 22,
                }),
                merged_branches_cleaned: 2,
                restacked_branches: 1,
            },
            Duration::from_millis(14_022),
        );

        control::unset_override();

        assert!(footer.contains("main"));
        assert!(footer.contains("+1 commit"));
        assert!(footer.contains("+434"));
        assert!(footer.contains("-22"));
        assert!(footer.contains("cleaned"));
        assert!(footer.contains("restacked"));
        assert!(footer.contains("14.022s"));
        assert!(footer.contains('\u{1b}'));
    }

    #[test]
    fn render_sync_footer_handles_up_to_date_branch() {
        control::set_override(true);

        let footer = render_sync_footer(
            &SyncStats {
                trunk: Some(TrunkSummary::UpToDate {
                    branch: "main".to_string(),
                }),
                merged_branches_cleaned: 0,
                restacked_branches: 0,
            },
            Duration::from_secs(2),
        );

        control::unset_override();

        assert!(footer.contains("main"));
        assert!(footer.contains("up to date"));
        assert!(footer.contains("2.000s"));
        assert!(footer.contains('\u{1b}'));
    }

    #[test]
    fn sync_delete_prompt_mentions_removed_linked_worktree_for_safe_non_current_branch() {
        let prompt = sync_delete_prompt(
            "cesar/review-pass",
            None,
            None,
            Some(&linked_worktree_cleanup(&[])),
        );

        assert_eq!(
            prompt,
            "Delete 'cesar/review-pass' and remove linked worktree 'review-pass'?"
        );
    }

    #[test]
    fn sync_delete_prompt_prefers_checkout_wording_for_current_branch() {
        let prompt = sync_delete_prompt(
            "cesar/review-pass",
            Some("main"),
            Some("upstream gone"),
            Some(&linked_worktree_cleanup(&[])),
        );

        assert_eq!(
            prompt,
            "Delete 'cesar/review-pass' (upstream gone) and checkout 'main'?"
        );
    }

    #[test]
    fn sync_delete_prompt_mentions_kept_linked_worktree_when_unsafe() {
        let prompt = sync_delete_prompt(
            "cesar/review-pass",
            None,
            Some("upstream gone"),
            Some(&linked_worktree_cleanup(&["dirty"])),
        );

        assert_eq!(
            prompt,
            "Delete 'cesar/review-pass' (upstream gone; keep linked worktree 'review-pass')?"
        );
    }
}
