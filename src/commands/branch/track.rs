use crate::config::Config;
use crate::engine::{BranchMetadata, PrInfo};
use crate::git::GitRepo;
use crate::github::client::GitHubClient;
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};
use std::process::Command;

pub fn run(parent: Option<String>, all_prs: bool) -> Result<()> {
    if all_prs {
        return run_track_all_prs();
    }
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let config = Config::load()?;
    let trunk = repo.trunk_branch()?;

    // Can't track trunk
    if current == trunk {
        println!(
            "{} is the trunk branch and cannot be tracked.",
            current.yellow()
        );
        return Ok(());
    }

    // Check if already tracked
    if let Some(existing) = BranchMetadata::read(repo.inner(), &current)? {
        println!(
            "Branch '{}' is already tracked with parent '{}'.",
            current.yellow(),
            existing.parent_branch_name.blue()
        );
        println!("Use {} to update.", "stax branch reparent".cyan());
        return Ok(());
    }

    // Determine parent
    let parent_branch = match parent {
        Some(p) => {
            // Validate the branch exists
            if repo.branch_commit(&p).is_err() {
                anyhow::bail!("Branch '{}' does not exist", p);
            }
            p
        }
        None => {
            // Build list of potential parents
            let mut branches = repo.list_branches()?;
            branches.retain(|b| b != &current);
            branches.sort();

            // Put trunk first as the recommended default
            if let Some(pos) = branches.iter().position(|b| b == &trunk) {
                branches.remove(pos);
                branches.insert(0, trunk.clone());
            }

            if branches.is_empty() {
                anyhow::bail!("No branches available to be parent");
            }

            // Build display with recommendation hint
            let items: Vec<String> = branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if i == 0 {
                        format!("{} (recommended)", b)
                    } else {
                        b.clone()
                    }
                })
                .collect();

            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Select parent branch for '{}'", current))
                .items(&items)
                .default(0)
                .interact()?;

            branches[selection].clone()
        }
    };

    let parent_rev = repo.branch_commit(&parent_branch)?;

    // Create metadata
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &current)?;

    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name()) {
        if !remote_branches.contains(&parent_branch) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }

    println!(
        "✓ Tracking '{}' with parent '{}'",
        current.green(),
        parent_branch.blue()
    );

    Ok(())
}

/// Track all open PRs authored by the current user
fn run_track_all_prs() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let trunk = repo.trunk_branch()?;
    let workdir = repo.workdir()?;
    let remote_name = config.remote_name();

    // Get remote info for GitHub API
    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    // Create GitHub client
    let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
    let client = rt.block_on(async {
        GitHubClient::new(remote_info.owner(), &remote_info.repo, remote_info.api_base_url.clone())
    })?;

    // Get current user
    let username = rt
        .block_on(async { client.get_current_user().await })
        .context("Failed to get current GitHub user")?;

    // Fetch all open PRs
    let open_prs = rt
        .block_on(async { client.get_user_open_prs(&username).await })
        .context("Failed to fetch open PRs")?;

    if open_prs.is_empty() {
        println!("No open PRs found for user '{}' in {}/{}.",
            username.cyan(),
            remote_info.owner().dimmed(),
            remote_info.repo.dimmed()
        );
        println!("{}", "Tip: This only finds PRs in the current repository.".dimmed());
        return Ok(());
    }

    println!(
        "Found {} open PR(s) by {}:\n",
        open_prs.len().to_string().cyan(),
        username.cyan()
    );

    let mut tracked_count = 0;
    let mut skipped_count = 0;
    let mut fetched_count = 0;

    for pr in open_prs {
        // Skip if already tracked
        if BranchMetadata::read(repo.inner(), &pr.head_branch)?.is_some() {
            println!(
                "  {} {} (already tracked)",
                "▸".dimmed(),
                pr.head_branch.dimmed()
            );
            skipped_count += 1;
            continue;
        }

        // Check if branch exists locally
        let branch_exists = repo.branch_commit(&pr.head_branch).is_ok();

        if !branch_exists {
            // Fetch branch from remote
            print!("  {} Fetching {}...", "↓".blue(), pr.head_branch.cyan());
            std::io::Write::flush(&mut std::io::stdout()).ok();

            match fetch_branch_from_remote(workdir, remote_name, &pr.head_branch) {
                Ok(_) => {
                    println!(" {}", "done".green());
                    fetched_count += 1;
                }
                Err(e) => {
                    println!(" {}", "failed".red());
                    eprintln!("    Error: {}", e);
                    continue;
                }
            }
        }

        // Validate parent branch exists
        let parent_branch = if repo.branch_commit(&pr.base_branch).is_ok() {
            pr.base_branch.clone()
        } else {
            // Fall back to trunk if base doesn't exist locally
            trunk.clone()
        };

        let parent_rev = match repo.branch_commit(&parent_branch) {
            Ok(rev) => rev,
            Err(_) => {
                eprintln!(
                    "  {} Could not get parent revision for '{}'",
                    "✗".red(),
                    pr.head_branch
                );
                continue;
            }
        };

        // Create metadata with PR info
        let meta = BranchMetadata {
            parent_branch_name: parent_branch.clone(),
            parent_branch_revision: parent_rev,
            pr_info: Some(PrInfo {
                number: pr.number,
                state: pr.state.to_uppercase(),
                is_draft: Some(pr.is_draft),
            }),
        };

        meta.write(repo.inner(), &pr.head_branch)?;

        let draft_indicator = if pr.is_draft { " (draft)" } else { "" };
        println!(
            "  {} Tracked '{}' (PR #{}{}) with parent '{}'",
            "✓".green(),
            pr.head_branch.green(),
            pr.number.to_string().yellow(),
            draft_indicator.dimmed(),
            parent_branch.blue()
        );
        tracked_count += 1;
    }

    println!();
    println!(
        "Tracked {} branch(es), fetched {}, skipped {} (already tracked).",
        tracked_count.to_string().green(),
        fetched_count.to_string().blue(),
        skipped_count.to_string().dimmed()
    );

    Ok(())
}

/// Fetch a single branch from remote and create local tracking branch
fn fetch_branch_from_remote(
    workdir: &std::path::Path,
    remote: &str,
    branch: &str,
) -> Result<()> {
    let status = Command::new("git")
        .args(["fetch", remote, &format!("{}:{}", branch, branch)])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git fetch")?;

    if !status.success() {
        anyhow::bail!(
            "Failed to fetch branch '{}' from remote '{}'",
            branch,
            remote
        );
    }

    Ok(())
}
