use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{generate_stack_comment, StackPrInfo};
use crate::github::GitHubClient;
use crate::remote::{self, Provider, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Editor, Input, Select};
use std::fs;
use std::path::Path;
use std::process::Command;

struct PrPlan {
    branch: String,
    parent: String,
    existing_pr: Option<u64>,
    // For new PRs, we'll collect these upfront
    title: Option<String>,
    body: Option<String>,
}

pub fn run(
    draft: bool,
    no_pr: bool,
    _force: bool, // kept for CLI compatibility
    yes: bool,
    no_prompt: bool,
    reviewers: Vec<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    quiet: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let auto_confirm = yes || no_prompt;

    // Get branches in current stack (excluding trunk)
    let stack_branches: Vec<String> = stack
        .current_stack(&current)
        .into_iter()
        .filter(|b| b != &stack.trunk)
        .collect();

    if stack_branches.is_empty() {
        if !quiet {
            println!("{}", "No tracked branches to submit.".yellow());
        }
        return Ok(());
    }

    // Validation phase
    if !quiet {
        println!(
            "{}",
            "Validating that this stack is ready to submit...".yellow()
        );
        println!();
    }

    // Check for needs restack - show warning but continue (like fp)
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

    if !needs_restack.is_empty() && !quiet {
        for b in &needs_restack {
            println!(
                "{}",
                format!(
                    "Note: {} has fallen behind its parent. You may encounter conflicts if you attempt to merge it.",
                    b
                ).yellow()
            );
        }
        println!();
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
        if !quiet {
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
        }

        let proceed = if auto_confirm {
            true
        } else {
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Skip empty branches and continue?")
                .default(true)
                .interact()?
        };

        if !proceed {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
        if !quiet {
            println!();
        }
    }

    // Filter out empty branches
    let empty_set: std::collections::HashSet<_> = empty_branches.iter().cloned().collect();
    let branches_to_submit: Vec<_> = stack_branches
        .iter()
        .filter(|b| !empty_set.contains(b))
        .cloned()
        .collect();

    if branches_to_submit.is_empty() {
        if !quiet {
            println!("{}", "No branches with changes to submit.".yellow());
        }
        return Ok(());
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;
    if remote_info.provider != Provider::GitHub && !no_pr {
        anyhow::bail!(
            "PR creation is only supported for GitHub remotes.\n\n\
             Current provider: {}\n\
             You can still push branches with:\n  \
             stax submit --no-pr",
            config.remote_provider()
        );
    }

    let owner = remote_info.owner().to_string();
    let repo_name = remote_info.repo.clone();

    // Fetch to ensure we have latest remote refs
    if !quiet {
        print!("Fetching from {}... ", remote_info.name);
    }
    remote::fetch_remote(repo.workdir()?, &remote_info.name)?;
    if !quiet {
        println!("{}", "✓".green());
    }

    // Check which branches exist on remote
    let remote_branches = remote::get_remote_branches(repo.workdir()?, &remote_info.name)?;

    // Verify trunk exists on remote
    if !remote_branches.contains(&stack.trunk) {
        anyhow::bail!(
            "Base branch '{}' does not exist on the remote.\n\n\
             This can happen if:\n  \
             - This is a new repository that hasn't been pushed yet\n  \
             - The default branch has a different name on GitHub\n\n\
             To fix this, push your base branch first:\n  \
             git push -u {} {}",
            stack.trunk,
            remote_info.name,
            stack.trunk
        );
    }

    // Build plan - determine which PRs need create vs update
    if !quiet {
        println!(
            "{}",
            "Preparing to submit PRs for the following branches...".yellow()
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    let client = rt.block_on(async {
        GitHubClient::new(&owner, &repo_name, remote_info.api_base_url.clone())
    })?;

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
        if !quiet {
            println!("  {} {} {}", "▸".white(), branch, action);
        }

        plans.push(PrPlan {
            branch: branch.clone(),
            parent: base,
            existing_pr: pr_number,
            title: None,
            body: None,
        });
    }
    if !quiet {
        println!();
    }

    // Collect PR details for new PRs BEFORE pushing
    if !no_pr {
        let pr_template = load_pr_template(repo.workdir()?);
        let new_prs: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_none()).collect();
        if !new_prs.is_empty() && !quiet {
            println!("{}", "Enter details for new PRs:".yellow());
            println!();
        }

        for plan in &mut plans {
            if plan.existing_pr.is_some() {
                continue;
            }

            let commit_messages =
                collect_commit_messages(repo.workdir()?, &plan.parent, &plan.branch);
            let default_title = default_pr_title(&commit_messages, &plan.branch);
            let default_body =
                build_default_pr_body(pr_template.as_deref(), &plan.branch, &commit_messages);

            if !quiet {
                println!("  {} {}", "▸".cyan(), plan.branch.cyan());
            }

            let title = if no_prompt {
                default_title
            } else {
                Input::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Title")
                    .default(default_title)
                    .interact_text()?
            };

            let body = if no_prompt {
                default_body
            } else {
                let options = if default_body.trim().is_empty() {
                    vec!["Edit", "Skip (leave empty)"]
                } else {
                    vec!["Use default", "Edit", "Skip (leave empty)"]
                };

                let choice = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("  Body")
                    .items(&options)
                    .default(0)
                    .interact()?;

                match options[choice] {
                    "Use default" => default_body,
                    "Edit" => Editor::new()
                        .edit(&default_body)?
                        .unwrap_or(default_body),
                    _ => String::new(),
                }
            };

            plan.title = Some(title);
            plan.body = Some(body);

            if !quiet {
                println!();
            }
        }
    }

    // Now push all branches
    if !quiet {
        println!(
            "Pushing {} branch(es) to {} ({}/{})...",
            branches_to_submit.len().to_string().cyan(),
            remote_info.name,
            owner,
            repo_name
        );
    }

    for branch in &branches_to_submit {
        if !quiet {
            print!("  Pushing {}... ", branch.white());
        }
        push_branch(repo.workdir()?, &remote_info.name, branch)?;
        if !quiet {
            println!("{}", "✓".green());
        }
    }

    if no_pr {
        if !quiet {
            println!();
            println!(
                "{}",
                "✓ Branches pushed (--no-pr, skipping PR creation)".green()
            );
        }
        return Ok(());
    }

    // Create/update PRs
    if !quiet {
        println!();
        println!("Creating/updating PRs...");
    }

    rt.block_on(async {
        let mut pr_infos: Vec<StackPrInfo> = Vec::new();

        for plan in &plans {
            let meta = BranchMetadata::read(repo.inner(), &plan.branch)?
                .context(format!("No metadata for branch {}", plan.branch))?;

            if plan.existing_pr.is_none() {
                // Create new PR
                let title = plan.title.as_ref().unwrap();
                let body = plan.body.as_ref().unwrap();

                if !quiet {
                    print!("  Creating PR for {}... ", plan.branch.white());
                }

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

                if !quiet {
                    println!("{} {}", "✓".green(), format!("#{}", pr.number).dimmed());
                }

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

                apply_pr_metadata(&client, pr.number, &reviewers, &labels, &assignees).await?;

                pr_infos.push(StackPrInfo {
                    branch: plan.branch.clone(),
                    pr_number: Some(pr.number),
                    pr_title: Some(title.clone()),
                });
            } else {
                // Update existing PR
                let pr_number = plan.existing_pr.unwrap();
                if !quiet {
                    print!(
                        "  Updating PR #{} for {}... ",
                        pr_number,
                        plan.branch.white()
                    );
                }

                // Update base if needed
                client.update_pr_base(pr_number, &plan.parent).await?;

                apply_pr_metadata(&client, pr_number, &reviewers, &labels, &assignees).await?;

                if !quiet {
                    println!("{}", "✓".green());
                }

                // Get current PR state
                let pr = client.get_pr(pr_number).await?;

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
                    pr_title: Some(pr.title.clone()),
                });
            }
        }

        // Update a single stack summary comment
        if !pr_infos.is_empty() {
            let summary_pr = pr_infos
                .iter()
                .find(|p| p.branch == current && p.pr_number.is_some())
                .and_then(|p| p.pr_number)
                .or_else(|| pr_infos.iter().rev().find_map(|p| p.pr_number));

            if let Some(num) = summary_pr {
                if !quiet {
                    println!();
                    println!("Updating stack summary...");
                    print!("  {} #{}... ", remote_info.provider.pr_label(), num);
                }
                let stack_comment = generate_stack_comment(
                    &pr_infos,
                    num,
                    &remote_info,
                    &stack.trunk,
                );
                client.update_stack_comment(num, &stack_comment).await?;
                if !quiet {
                    println!("{}", "✓".green());
                }
            }
        }

        if !quiet {
            println!();
            println!("{}", "✓ Stack submitted successfully!".green());
        }

        // Print PR URLs
        if !pr_infos.is_empty() && !quiet {
            println!();
            for pr_info in &pr_infos {
                if let Some(num) = pr_info.pr_number {
                    println!(
                        "  {} → {}",
                        pr_info.branch.white(),
                        remote_info.pr_url(num)
                    );
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    Ok(())
}

fn push_branch(workdir: &std::path::Path, remote: &str, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["push", "-f", remote, branch])
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

fn collect_commit_messages(workdir: &Path, parent: &str, branch: &str) -> Vec<String> {
    let output = Command::new("git")
        .args([
            "log",
            "--reverse",
            "--format=%s",
            &format!("{}..{}", parent, branch),
        ])
        .current_dir(workdir)
        .output();

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .filter(|line| !line.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn default_pr_title(commit_messages: &[String], branch: &str) -> String {
    if let Some(first) = commit_messages.first() {
        return first.clone();
    }

    branch
        .split('/')
        .next_back()
        .unwrap_or(branch)
        .replace(['-', '_'], " ")
}

fn build_default_pr_body(
    template: Option<&str>,
    branch: &str,
    commit_messages: &[String],
) -> String {
    let commits_text = render_commit_list(commit_messages);

    let mut body = if let Some(template) = template {
        template.to_string()
    } else if commits_text.is_empty() {
        String::new()
    } else {
        format!("## Summary\n\n{}", commits_text)
    };

    if !body.is_empty() {
        body = body.replace("{{BRANCH}}", branch);
        body = body.replace("{{COMMITS}}", &commits_text);
    }

    body
}

fn render_commit_list(commit_messages: &[String]) -> String {
    if commit_messages.is_empty() {
        return String::new();
    }

    commit_messages
        .iter()
        .map(|msg| format!("- {}", msg))
        .collect::<Vec<_>>()
        .join("\n")
}

fn load_pr_template(workdir: &Path) -> Option<String> {
    let candidates = [
        ".github/pull_request_template.md",
        ".github/PULL_REQUEST_TEMPLATE.md",
        "PULL_REQUEST_TEMPLATE.md",
        "pull_request_template.md",
    ];

    for candidate in &candidates {
        let path = workdir.join(candidate);
        if path.is_file() {
            if let Ok(content) = fs::read_to_string(path) {
                return Some(content);
            }
        }
    }

    let dir = workdir.join(".github").join("pull_request_template");
    if dir.is_dir() {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|ext| ext == "md")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|entry| entry.path());
        if let Some(entry) = entries.first() {
            if let Ok(content) = fs::read_to_string(entry.path()) {
                return Some(content);
            }
        }
    }

    None
}

async fn apply_pr_metadata(
    client: &GitHubClient,
    pr_number: u64,
    reviewers: &[String],
    labels: &[String],
    assignees: &[String],
) -> Result<()> {
    if !reviewers.is_empty() {
        client.request_reviewers(pr_number, reviewers).await?;
    }

    if !labels.is_empty() {
        client.add_labels(pr_number, labels).await?;
    }

    if !assignees.is_empty() {
        client.add_assignees(pr_number, assignees).await?;
    }

    Ok(())
}
