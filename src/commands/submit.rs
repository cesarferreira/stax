use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{generate_stack_comment, StackPrInfo};
use crate::github::GitHubClient;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

pub fn run(draft: bool, no_pr: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get branches in current stack (excluding trunk)
    let stack_branches: Vec<String> = stack
        .current_stack(&current)
        .into_iter()
        .filter(|b| b != &stack.trunk)
        .collect();

    if stack_branches.is_empty() {
        println!("{}", "No tracked branches to submit.".yellow());
        return Ok(());
    }

    // Check for needs restack
    let needs_restack: Vec<_> = stack_branches
        .iter()
        .filter(|b| {
            stack
                .branches
                .get(*b)
                .map(|br| br.needs_restack)
                .unwrap_or(false)
        })
        .collect();

    if !needs_restack.is_empty() {
        println!(
            "{}",
            "⚠ Some branches need restacking before submit.".yellow()
        );
        println!("Run {} first.", "stax rs".cyan());
        return Ok(());
    }

    // Get remote URL for GitHub
    let remote_url = get_remote_url(repo.workdir()?)?;
    let (owner, repo_name) = GitHubClient::from_remote(&remote_url)?;

    println!(
        "Submitting {} branch(es) to {}/{}...",
        stack_branches.len().to_string().cyan(),
        owner,
        repo_name
    );
    println!();

    // Push all branches
    for branch in &stack_branches {
        print!("  Pushing {}... ", branch.white());
        push_branch(repo.workdir()?, branch)?;
        println!("{}", "✓".green());
    }

    if no_pr {
        println!();
        println!("{}", "✓ Branches pushed (--no-pr, skipping PR creation)".green());
        return Ok(());
    }

    // Create/update PRs
    println!();
    println!("Creating/updating PRs...");

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        let client = GitHubClient::new(&owner, &repo_name)?;

        let mut pr_infos: Vec<StackPrInfo> = Vec::new();

        for branch in &stack_branches {
            let meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;

            // Check if PR exists
            let existing_pr = client.find_pr(branch).await?;

            let pr = if let Some(pr) = existing_pr {
                print!("  Updating PR #{} for {}... ", pr.number, branch.white());

                // Update base if needed
                client.update_pr_base(pr.number, &meta.parent_branch_name).await?;

                println!("{}", "✓".green());
                pr
            } else {
                print!("  Creating PR for {}... ", branch.white());

                // Create new PR
                let title = branch.replace('-', " ").replace('_', " ");
                let body = format!("Stack branch: `{}`\n\nParent: `{}`", branch, meta.parent_branch_name);

                let pr = client
                    .create_pr(branch, &meta.parent_branch_name, &title, &body, draft)
                    .await?;

                println!("{} {}", "✓".green(), format!("#{}", pr.number).dimmed());
                pr
            };

            // Update metadata with PR info
            let updated_meta = BranchMetadata {
                pr_info: Some(crate::engine::metadata::PrInfo {
                    number: pr.number,
                    state: pr.state.clone(),
                    is_draft: Some(pr.is_draft),
                }),
                ..meta
            };
            updated_meta.write(repo.inner(), branch)?;

            pr_infos.push(StackPrInfo {
                branch: branch.clone(),
                pr_number: Some(pr.number),
                state: Some(pr.state.clone()),
                is_draft: pr.is_draft,
            });
        }

        // Update stack comments on all PRs
        println!();
        println!("Updating stack comments...");

        for pr_info in &pr_infos {
            if let Some(num) = pr_info.pr_number {
                print!("  PR #{}... ", num);
                let stack_comment = generate_stack_comment(&pr_infos, num, &owner, &repo_name);
                client.update_stack_comment(num, &stack_comment).await?;
                println!("{}", "✓".green());
            }
        }

        println!();
        println!("{}", "✓ Stack submitted successfully!".green());

        // Print PR URLs
        println!();
        for pr_info in &pr_infos {
            if let Some(num) = pr_info.pr_number {
                println!(
                    "  {} → https://github.com/{}/{}/pull/{}",
                    pr_info.branch.white(),
                    owner,
                    repo_name,
                    num
                );
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn get_remote_url(workdir: &std::path::Path) -> Result<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(workdir)
        .output()
        .context("Failed to get remote URL")?;

    if !output.status.success() {
        anyhow::bail!(
            "No git remote 'origin' found.\n\n\
             To fix this, add a GitHub remote:\n\n  \
             git remote add origin git@github.com:owner/repo.git\n\n\
             Or:\n\n  \
             git remote add origin https://github.com/owner/repo.git"
        );
    }

    let url = String::from_utf8(output.stdout)?.trim().to_string();

    if url.is_empty() {
        anyhow::bail!(
            "Git remote 'origin' has no URL configured.\n\n\
             To fix this, set the remote URL:\n\n  \
             git remote set-url origin git@github.com:owner/repo.git"
        );
    }

    Ok(url)
}

fn push_branch(workdir: &std::path::Path, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["push", "-f", "origin", branch])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to push branch")?;

    if !status.success() {
        anyhow::bail!("Failed to push branch {}", branch);
    }
    Ok(())
}
