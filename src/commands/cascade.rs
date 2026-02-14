use crate::commands;
use crate::config::Config;
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::io::Write;
use std::process::Command;

pub fn run(no_submit: bool, no_pr: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;

    println!("{}", "Cascading stack...".bold());

    // Fetch from remote and update local trunk before restacking.
    // Without this, restack rebases onto a stale local trunk, causing
    // PRs to include commits that are already on the remote trunk.
    sync_trunk(&repo)?;

    commands::navigate::bottom()?;
    commands::restack::run(false, false, true, false)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    commands::upstack::restack::run(false)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if !no_submit {
        commands::submit::run(
            false,  // draft
            no_pr,  // no_pr
            false,  // _force
            true,   // yes
            true,   // no_prompt
            vec![], // reviewers
            vec![], // labels
            vec![], // assignees
            false,  // quiet
            false,  // verbose
            None,   // template
            false,  // no_template
            false,  // edit
        )?;
    }

    if !repo.rebase_in_progress()? && repo.current_branch()? != original {
        repo.checkout(&original)?;
    }

    Ok(())
}

/// Fetch from remote and fast-forward local trunk to match.
/// This prevents rebasing onto a stale local trunk, which would cause PRs
/// to include commits already present on the remote.
fn sync_trunk(repo: &GitRepo) -> Result<()> {
    let config = Config::load()?;
    let remote_name = config.remote_name().to_string();
    let stack = Stack::load(repo)?;
    let workdir = repo.workdir()?;
    let current = repo.current_branch()?;

    // 1. Fetch latest refs from remote
    print!("  Fetching from {}... ", remote_name);
    std::io::stdout().flush().ok();

    let fetch_output = Command::new("git")
        .args(["fetch", &remote_name])
        .current_dir(workdir)
        .output()
        .context("Failed to fetch from remote")?;

    if fetch_output.status.success() {
        println!("{}", "done".green());
    } else {
        println!("{}", "warning".yellow());
        // Continue anyway - local trunk update may still work if refs were partially updated
    }

    // 2. Update local trunk branch to match remote
    print!("  Updating {}... ", stack.trunk.cyan());
    std::io::stdout().flush().ok();

    if current == stack.trunk {
        // We're on trunk - pull directly
        let output = Command::new("git")
            .args(["pull", "--ff-only", &remote_name, &stack.trunk])
            .current_dir(workdir)
            .output()
            .context("Failed to pull trunk")?;

        if output.status.success() {
            println!("{}", "done".green());
        } else {
            // pull --ff-only failed (trunk may have diverged). Stash any local
            // changes before resetting so they are not destroyed.
            let stashed = if repo.is_dirty_at(workdir)? {
                repo.stash_push_at(workdir)?
            } else {
                false
            };

            let reset_output = Command::new("git")
                .args([
                    "reset",
                    "--hard",
                    &format!("{}/{}", remote_name, stack.trunk),
                ])
                .current_dir(workdir)
                .output()
                .context("Failed to reset trunk")?;

            if reset_output.status.success() {
                println!("{}", "reset to remote".yellow());
            } else {
                println!("{}", "failed".yellow());
            }

            if stashed {
                if let Err(e) = repo.stash_pop_at(workdir) {
                    println!(
                        "  {} Auto-stash pop failed ({}). Your changes are safe in 'git stash list'.",
                        "warning:".yellow(),
                        e
                    );
                }
            }
        }
    } else if let Some(trunk_worktree_path) = repo.branch_worktree_path(&stack.trunk)? {
        // Trunk is checked out in another worktree - pull there
        let output = Command::new("git")
            .args(["pull", "--ff-only", &remote_name, &stack.trunk])
            .current_dir(&trunk_worktree_path)
            .output()
            .context("Failed to pull trunk in its worktree")?;

        if output.status.success() {
            println!("{}", "done".green());
        } else {
            let stashed = if repo.is_dirty_at(&trunk_worktree_path)? {
                repo.stash_push_at(&trunk_worktree_path)?
            } else {
                false
            };

            let reset_output = Command::new("git")
                .args([
                    "reset",
                    "--hard",
                    &format!("{}/{}", remote_name, stack.trunk),
                ])
                .current_dir(&trunk_worktree_path)
                .output()
                .context("Failed to reset trunk in its worktree")?;

            if reset_output.status.success() {
                println!("{}", "reset to remote".yellow());
            } else {
                println!("{}", "failed".yellow());
            }

            if stashed {
                if let Err(e) = repo.stash_pop_at(&trunk_worktree_path) {
                    println!(
                        "  {} Auto-stash pop failed in '{}' ({}). Your changes are safe in 'git stash list'.",
                        "warning:".yellow(),
                        trunk_worktree_path.display(),
                        e
                    );
                }
            }
        }
    } else {
        // Trunk isn't checked out anywhere - update via refspec fetch
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
            println!("{}", "done".green());
        } else {
            // Refspec fetch can fail if local trunk has diverged; this is non-fatal
            // since the rebase will still use origin/<trunk> indirectly through
            // the metadata's parent_branch_revision
            println!("{}", "skipped (local trunk may be stale)".yellow());
        }
    }

    Ok(())
}
