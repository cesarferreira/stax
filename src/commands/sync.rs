use crate::cache::CiCache;
use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::github::GitHubClient;
use crate::remote::{Provider, RemoteInfo};
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

    // 2. Update trunk branch
    if !quiet {
        print!("  Updating {}... ", stack.trunk.cyan());
        let _ = std::io::stdout().flush();
    }

    // Check if we're on trunk
    let was_on_trunk = current == stack.trunk;

    if was_on_trunk {
        // Pull directly
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
                .args(["reset", "--hard", &format!("{}/{}", remote_name, stack.trunk)])
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
        // Update trunk without switching to it
        let output = Command::new("git")
            .args([
                "fetch",
                &remote_name,
                &format!("{}:{}", stack.trunk, stack.trunk),
            ])
            .current_dir(workdir)
            .output()
            .context("Failed to update trunk")?;

        if !quiet {
            if output.status.success() {
                println!("{}", "done".green());
            } else {
                println!("{}", "failed (may need manual update)".yellow());
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
    }

    // 3. Delete merged branches
    if delete_merged {
        let merged = find_merged_branches(workdir, &stack, &remote_name)?;

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
                                "deleted (local only)".green()
                            );
                        } else if remote_deleted {
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "deleted (remote only)".green()
                            );
                        } else {
                            // Branch was already deleted (orphaned), just cleaned up metadata
                            println!(
                                "    {} {}",
                                branch.bright_black(),
                                "cleaned up (already deleted)".green()
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

    // Method 2: Find orphaned branches (tracked but no longer exist locally or remotely)
    // Get list of all local branches
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

    // Check each tracked branch
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

    if remote_info.provider != Provider::GitHub {
        return;
    }

    if Config::github_token().is_none() {
        return;
    }

    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(_) => return,
    };

    let client = match rt.block_on(async {
        GitHubClient::new(remote_info.owner(), &remote_info.repo, remote_info.api_base_url.clone())
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
