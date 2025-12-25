use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::process::Command;

/// Sync repo: pull trunk from remote, delete merged branches, optionally restack
pub fn run(
    restack: bool,
    delete_merged: bool,
    force: bool,
    safe: bool,
    r#continue: bool,
    quiet: bool,
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
    }
    let status = Command::new("git")
        .args(["fetch", &remote_name])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to fetch")?;

    if !quiet {
        if status.success() {
            println!("{}", "done".green());
        } else {
            println!("{}", "failed".red());
        }
    }

    // 2. Update trunk branch
    if !quiet {
        print!("  Updating {}... ", stack.trunk.cyan());
    }

    // Check if we're on trunk
    let was_on_trunk = current == stack.trunk;

    if was_on_trunk {
        // Pull directly
        let status = Command::new("git")
            .args(["pull", "--ff-only", &remote_name, &stack.trunk])
            .current_dir(workdir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to pull trunk")?;

        if status.success() {
            if !quiet {
                println!("{}", "done".green());
            }
        } else if safe {
            if !quiet {
                println!("{}", "failed (safe mode, no reset)".yellow());
            }
        } else {
            // Try reset to remote
            let status = Command::new("git")
                .args(["reset", "--hard", &format!("{}/{}", remote_name, stack.trunk)])
                .current_dir(workdir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .context("Failed to reset trunk")?;

            if !quiet {
                if status.success() {
                    println!("{}", "reset to remote".yellow());
                } else {
                    println!("{}", "failed".red());
                }
            }
        }
    } else {
        // Update trunk without switching to it
        let status = Command::new("git")
            .args([
                "fetch",
                &remote_name,
                &format!("{}:{}", stack.trunk, stack.trunk),
            ])
            .current_dir(workdir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to update trunk")?;

        if !quiet {
            if status.success() {
                println!("{}", "done".green());
            } else {
                println!("{}", "failed (may need manual update)".yellow());
            }
        }
    }

    // 3. Delete merged branches
    if delete_merged {
        let merged = find_merged_branches(workdir, &stack)?;

        if !merged.is_empty() {
            if !quiet {
                println!("  Found {} merged branch(es):", merged.len().to_string().cyan());
                for branch in &merged {
                    println!("    {} {}", "▸".bright_black(), branch);
                }
                println!();
            }

            for branch in &merged {
                let confirm = if auto_confirm {
                    true
                } else if quiet {
                    false
                } else {
                    Confirm::with_theme(&ColorfulTheme::default())
                        .with_prompt(format!("Delete '{}'?", branch))
                        .default(true)
                        .interact()?
                };

                if confirm {
                    // Delete local branch (force delete since we confirmed)
                    let local_status = Command::new("git")
                        .args(["branch", "-D", branch])
                        .current_dir(workdir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();

                    let local_deleted = local_status.map(|s| s.success()).unwrap_or(false);

                    // Delete remote branch
                    let remote_status = Command::new("git")
                        .args(["push", &remote_name, "--delete", branch])
                        .current_dir(workdir)
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();

                    let remote_deleted = remote_status.map(|s| s.success()).unwrap_or(false);

                    // Delete metadata
                    let _ = crate::git::refs::delete_metadata(repo.inner(), branch);

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
                                "deleted (local only)".yellow()
                            );
                        } else if remote_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (remote only)".yellow()
                            );
                        } else {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "failed to delete".red()
                            );
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

    // 4. Optionally restack
    if restack {
        if !quiet {
            println!();
            println!("{}", "Restacking...".bold());
        }

        let needs_restack = stack.needs_restack();

        if needs_restack.is_empty() {
            if !quiet {
                println!("  {}", "All branches up to date.".dimmed());
            }
        } else {
            let mut summary: Vec<(String, String)> = Vec::new();

            for branch in &needs_restack {
                if !quiet {
                    print!("  Restacking {}... ", branch.cyan());
                }

                repo.checkout(branch)?;

                let meta = match BranchMetadata::read(repo.inner(), branch)? {
                    Some(meta) => meta,
                    None => continue,
                };

                match repo.rebase(&meta.parent_branch_name)? {
                    RebaseResult::Success => {
                        let parent_commit = repo.branch_commit(&meta.parent_branch_name)?;
                        let updated_meta = BranchMetadata {
                            parent_branch_revision: parent_commit,
                            ..meta
                        };
                        updated_meta.write(repo.inner(), branch)?;
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
                        return Ok(());
                    }
                }
            }

            repo.checkout(&current)?;

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

    if !quiet {
        println!();
        println!("{}", "Sync complete!".green().bold());
    }

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
