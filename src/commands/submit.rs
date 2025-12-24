use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{generate_stack_comment, StackPrInfo};
use crate::github::GitHubClient;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::process::Command;

struct PrPlan {
    branch: String,
    parent: String,
    existing_pr: Option<u64>,
    // For new PRs, we'll collect these upfront
    title: Option<String>,
    body: Option<String>,
}

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

    // Validation phase
    println!(
        "{}",
        "Validating that this stack is ready to submit...".yellow()
    );
    println!();

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
            "WARNING: Some branches need restacking before submit:".red()
        );
        for b in &needs_restack {
            println!("  {} {}", "▸".red(), b);
        }
        println!();
        println!("Run {} first.", "stax rs --restack".cyan());
        return Ok(());
    }

    // Check for branches with no changes (empty branches)
    let empty_branches: Vec<_> = stack_branches
        .iter()
        .filter(|b| {
            if let Some(branch_info) = stack.branches.get(*b) {
                if let Some(parent) = &branch_info.parent {
                    if let Ok(branch_commit) = repo.branch_commit(b) {
                        if let Ok(parent_commit) = repo.branch_commit(parent) {
                            return branch_commit == parent_commit;
                        }
                    }
                }
            }
            false
        })
        .collect();

    if !empty_branches.is_empty() {
        println!(
            "{}",
            "WARNING: The following branches have no changes:".yellow()
        );
        for b in &empty_branches {
            println!("  {} {}", "▸".yellow(), b);
        }
        println!();
        println!(
            "{}",
            "GitHub will reject PRs for branches with no commits.".yellow()
        );

        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Skip empty branches and continue?")
            .default(true)
            .interact()?;

        if !proceed {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
        println!();
    }

    // Filter out empty branches
    let empty_set: std::collections::HashSet<_> = empty_branches.iter().cloned().collect();
    let branches_to_submit: Vec<_> = stack_branches
        .iter()
        .filter(|b| !empty_set.contains(b))
        .cloned()
        .collect();

    if branches_to_submit.is_empty() {
        println!("{}", "No branches with changes to submit.".yellow());
        return Ok(());
    }

    // Get remote URL for GitHub
    let remote_url = get_remote_url(repo.workdir()?)?;
    let (owner, repo_name) = GitHubClient::from_remote(&remote_url)?;

    // Fetch to ensure we have latest remote refs
    print!("Fetching from origin... ");
    fetch_origin(repo.workdir()?)?;
    println!("{}", "✓".green());

    // Check which branches exist on remote
    let remote_branches = get_remote_branches(repo.workdir()?)?;

    // Verify trunk exists on remote
    if !remote_branches.contains(&stack.trunk) {
        anyhow::bail!(
            "Base branch '{}' does not exist on the remote.\n\n\
             This can happen if:\n  \
             - This is a new repository that hasn't been pushed yet\n  \
             - The default branch has a different name on GitHub\n\n\
             To fix this, push your base branch first:\n  \
             git push -u origin {}",
            stack.trunk,
            stack.trunk
        );
    }

    // Build plan - determine which PRs need create vs update
    println!(
        "{}",
        "Preparing to submit PRs for the following branches...".yellow()
    );

    let rt = tokio::runtime::Runtime::new()?;
    let client = rt.block_on(async { GitHubClient::new(&owner, &repo_name) })?;

    let mut plans: Vec<PrPlan> = Vec::new();

    for branch in &branches_to_submit {
        let meta = BranchMetadata::read(repo.inner(), branch)?
            .context(format!("No metadata for branch {}", branch))?;

        // Check if PR exists
        let existing_pr = rt.block_on(async { client.find_pr(branch).await })?;
        let pr_number = existing_pr.as_ref().map(|p| p.number);

        // Determine the base branch for PR
        let base = if remote_branches.contains(&meta.parent_branch_name) {
            meta.parent_branch_name.clone()
        } else {
            // Parent not on remote - will be pushed, use it anyway
            meta.parent_branch_name.clone()
        };

        let action = if existing_pr.is_none() {
            "(Create)".green()
        } else {
            format!("(Update #{})", pr_number.unwrap()).blue()
        };
        println!("  {} {} {}", "▸".white(), branch, action);

        plans.push(PrPlan {
            branch: branch.clone(),
            parent: base,
            existing_pr: pr_number,
            title: None,
            body: None,
        });
    }
    println!();

    // Collect PR details for new PRs BEFORE pushing
    if !no_pr {
        let new_prs: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_none()).collect();
        if !new_prs.is_empty() {
            println!("{}", "Enter details for new PRs:".yellow());
            println!();

            for plan in &mut plans {
                if plan.existing_pr.is_none() {
                    let default_title = plan
                        .branch
                        .split('/')
                        .last()
                        .unwrap_or(&plan.branch)
                        .replace('-', " ")
                        .replace('_', " ");

                    println!("  {} {}", "▸".cyan(), plan.branch.cyan());

                    let title: String = Input::with_theme(&ColorfulTheme::default())
                        .with_prompt("  Title")
                        .default(default_title)
                        .interact_text()?;

                    let body_options = vec!["Skip (leave empty)", "Enter description"];
                    let body_choice = Select::with_theme(&ColorfulTheme::default())
                        .with_prompt("  Body")
                        .items(&body_options)
                        .default(0)
                        .interact()?;

                    let body = if body_choice == 1 {
                        Input::with_theme(&ColorfulTheme::default())
                            .with_prompt("  Description")
                            .allow_empty(true)
                            .interact_text()?
                    } else {
                        String::new()
                    };

                    plan.title = Some(title);
                    plan.body = Some(body);
                    println!();
                }
            }
        }
    }

    // Now push all branches
    println!(
        "Pushing {} branch(es) to {}/{}...",
        branches_to_submit.len().to_string().cyan(),
        owner,
        repo_name
    );

    for branch in &branches_to_submit {
        print!("  Pushing {}... ", branch.white());
        push_branch(repo.workdir()?, branch)?;
        println!("{}", "✓".green());
    }

    if no_pr {
        println!();
        println!(
            "{}",
            "✓ Branches pushed (--no-pr, skipping PR creation)".green()
        );
        return Ok(());
    }

    // Create/update PRs
    println!();
    println!("Creating/updating PRs...");

    rt.block_on(async {
        let mut pr_infos: Vec<StackPrInfo> = Vec::new();

        for plan in &plans {
            let meta = BranchMetadata::read(repo.inner(), &plan.branch)?
                .context(format!("No metadata for branch {}", plan.branch))?;

            if plan.existing_pr.is_none() {
                // Create new PR
                let title = plan.title.as_ref().unwrap();
                let body = plan.body.as_ref().unwrap();

                print!("  Creating PR for {}... ", plan.branch.white());

                let pr = client
                    .create_pr(&plan.branch, &plan.parent, title, body, draft)
                    .await
                    .context(format!(
                        "Failed to create PR for '{}' with base '{}'\n\
                         This may happen if:\n  \
                         - The base branch '{}' doesn't exist on GitHub\n  \
                         - The branch has no commits different from base\n  \
                         Try: git log {}..{} to see the commits",
                        plan.branch, plan.parent, plan.parent, plan.parent, plan.branch
                    ))?;

                println!("{} {}", "✓".green(), format!("#{}", pr.number).dimmed());

                // Update metadata with PR info
                let updated_meta = BranchMetadata {
                    pr_info: Some(crate::engine::metadata::PrInfo {
                        number: pr.number,
                        state: pr.state.clone(),
                        is_draft: Some(pr.is_draft),
                    }),
                    ..meta
                };
                updated_meta.write(repo.inner(), &plan.branch)?;

                pr_infos.push(StackPrInfo {
                    branch: plan.branch.clone(),
                    pr_number: Some(pr.number),
                    state: Some(pr.state.clone()),
                    is_draft: pr.is_draft,
                });
            } else {
                // Update existing PR
                let pr_number = plan.existing_pr.unwrap();
                print!(
                    "  Updating PR #{} for {}... ",
                    pr_number,
                    plan.branch.white()
                );

                // Update base if needed
                client.update_pr_base(pr_number, &plan.parent).await?;

                println!("{}", "✓".green());

                // Get current PR state
                let pr = client.get_pr(pr_number).await?;

                pr_infos.push(StackPrInfo {
                    branch: plan.branch.clone(),
                    pr_number: Some(pr.number),
                    state: Some(pr.state.clone()),
                    is_draft: pr.is_draft,
                });
            }
        }

        // Update stack comments on all PRs
        if !pr_infos.is_empty() {
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
        }

        println!();
        println!("{}", "✓ Stack submitted successfully!".green());

        // Print PR URLs
        if !pr_infos.is_empty() {
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

fn get_remote_branches(workdir: &std::path::Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output()
        .context("Failed to list remote branches")?;

    let branches: Vec<String> = String::from_utf8(output.stdout)?
        .lines()
        .map(|s| s.trim().strip_prefix("origin/").unwrap_or(s).to_string())
        .collect();

    Ok(branches)
}

fn fetch_origin(workdir: &std::path::Path) -> Result<()> {
    let status = Command::new("git")
        .args(["fetch", "origin"])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to fetch from origin")?;

    if !status.success() {
        anyhow::bail!("Failed to fetch from origin");
    }
    Ok(())
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
