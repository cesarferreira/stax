use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
pub fn run(restack: bool, delete_merged: bool, _force: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?;

    println!("{}", "Syncing repository...".bold());

    // 1. Fetch from remote
    print!("  Fetching from origin... ");
    let status = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(&workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to fetch")?;

    if status.success() {
        println!("{}", "done".green());
    } else {
        println!("{}", "failed".red());
    }

    // 2. Update trunk branch
    print!("  Updating {}... ", stack.trunk.cyan());

    // Check if we're on trunk
    let was_on_trunk = current == stack.trunk;

    if was_on_trunk {
        // Pull directly
        let status = Command::new("git")
            .args(["pull", "--ff-only", "origin", &stack.trunk])
            .current_dir(&workdir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to pull trunk")?;

        if status.success() {
            println!("{}", "done".green());
        } else {
            // Try reset to origin
            let status = Command::new("git")
                .args(["reset", "--hard", &format!("origin/{}", stack.trunk)])
                .current_dir(&workdir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .context("Failed to reset trunk")?;

            if status.success() {
                println!("{}", "reset to origin".yellow());
            } else {
                println!("{}", "failed".red());
            }
        }
    } else {
        // Update trunk without switching to it
        let status = Command::new("git")
            .args(["fetch", "origin", &format!("{}:{}", stack.trunk, stack.trunk)])
            .current_dir(&workdir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to update trunk")?;

        if status.success() {
            println!("{}", "done".green());
        } else {
            println!("{}", "failed (may need manual update)".yellow());
        }
    }

    // 3. Delete merged branches
    if delete_merged {
        let merged = find_merged_branches(&workdir, &stack)?;

        if !merged.is_empty() {
            println!("  Deleting merged branches:");
            for branch in &merged {
                print!("    {} ", branch.bright_black());

                // Delete local branch
                let status = Command::new("git")
                    .args(["branch", "-d", branch])
                    .current_dir(&workdir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                if status.map(|s| s.success()).unwrap_or(false) {
                    // Also delete metadata
                    let _ = crate::git::refs::delete_metadata(repo.inner(), branch);
                    println!("{}", "deleted".green());
                } else {
                    println!("{}", "skipped (not fully merged)".yellow());
                }
            }
        } else {
            println!("  {}", "No merged branches to delete.".dimmed());
        }
    }

    // 4. Optionally restack
    if restack {
        println!();
        println!("{}", "Restacking...".bold());

        // Find branches that need restack
        let needs_restack = stack.needs_restack();

        if needs_restack.is_empty() {
            println!("  {}", "All branches up to date.".dimmed());
        } else {
            for branch in &needs_restack {
                print!("  Restacking {}... ", branch.cyan());

                // Checkout and rebase
                let checkout = Command::new("git")
                    .args(["checkout", branch])
                    .current_dir(&workdir)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();

                if !checkout.map(|s| s.success()).unwrap_or(false) {
                    println!("{}", "failed to checkout".red());
                    continue;
                }

                // Get parent
                if let Some(info) = stack.branches.get(branch) {
                    if let Some(parent) = &info.parent {
                        let rebase = Command::new("git")
                            .args(["rebase", parent])
                            .current_dir(&workdir)
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status();

                        if rebase.map(|s| s.success()).unwrap_or(false) {
                            println!("{}", "done".green());
                        } else {
                            println!("{}", "conflicts - run 'stax continue' after resolving".yellow());
                            return Ok(());
                        }
                    }
                }
            }

            // Return to original branch
            let _ = Command::new("git")
                .args(["checkout", &current])
                .current_dir(&workdir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    println!();
    println!("{}", "Sync complete!".green().bold());

    Ok(())
}

/// Find branches that have been merged into trunk
fn find_merged_branches(workdir: &std::path::Path, stack: &Stack) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["branch", "--merged", &stack.trunk])
        .current_dir(workdir)
        .output()
        .context("Failed to list merged branches")?;

    let merged_output = String::from_utf8_lossy(&output.stdout);
    let mut merged = Vec::new();

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

    Ok(merged)
}
