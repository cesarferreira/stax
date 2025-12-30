use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{generate_stack_comment, StackPrInfo};
use crate::github::GitHubClient;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Editor, Input, Select};
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
    is_draft: Option<bool>,
    // Track if this is a no-op (already synced)
    needs_push: bool,
    needs_pr_update: bool,
    // Empty branches get pushed but no PR created
    is_empty: bool,
}

#[allow(clippy::too_many_arguments)]
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
    let _ = yes; // Used for future auto-confirm features

    // Track if --draft was explicitly passed (we'll ask interactively if not)
    let draft_flag_set = draft;

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
        println!("{}", "Submitting stack...".bold());
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
                "  {} {} needs restack",
                "!".yellow(),
                b.cyan()
            );
        }
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

    // Empty branches will be pushed but won't get PRs created
    let empty_set: std::collections::HashSet<_> = empty_branches.iter().cloned().collect();

    if !empty_branches.is_empty() && !quiet {
        println!("  {} Empty branches (will push, skip PR):", "!".yellow());
        for b in &empty_branches {
            println!("    {}", b.dimmed());
        }
    }

    // Submit all branches in the stack
    let branches_to_submit = stack_branches.clone();

    if branches_to_submit.is_empty() {
        if !quiet {
            println!("{}", "No branches to submit.".yellow());
        }
        return Ok(());
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    let owner = remote_info.owner().to_string();
    let repo_name = remote_info.repo.clone();

    // Fetch to ensure we have latest remote refs
    if !quiet {
        print!("  Fetching from {}... ", remote_info.name);
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    remote::fetch_remote(repo.workdir()?, &remote_info.name)?;
    if !quiet {
        println!("{}", "done".green());
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
        print!("  Planning PR operations... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    let rt = tokio::runtime::Runtime::new()?;
    let client = rt.block_on(async {
        GitHubClient::new(&owner, &repo_name, remote_info.api_base_url.clone())
    })?;

    let mut plans: Vec<PrPlan> = Vec::new();

    for branch in &branches_to_submit {
        let meta = BranchMetadata::read(repo.inner(), branch)?
            .context(format!("No metadata for branch {}", branch))?;

        let is_empty = empty_set.contains(branch);

        // Check if PR exists (skip for empty branches)
        let existing_pr = if is_empty {
            None
        } else {
            rt.block_on(async { client.find_pr(branch).await })?
        };
        let pr_number = existing_pr.as_ref().map(|p| p.number);

        // Determine the base branch for PR
        let base = meta.parent_branch_name.clone();

        // Check if we actually need to push
        let needs_push = branch_needs_push(repo.workdir()?, &remote_info.name, branch);

        // Check if PR base needs updating (not for empty branches)
        let needs_pr_update = if is_empty {
            false
        } else if let Some(pr) = &existing_pr {
            pr.base != base || needs_push
        } else {
            true // New PR always needs creation
        };

        plans.push(PrPlan {
            branch: branch.clone(),
            parent: base,
            existing_pr: pr_number,
            title: None,
            body: None,
            is_draft: None,
            needs_push,
            needs_pr_update,
            is_empty,
        });
    }
    if !quiet {
        println!("{}", "done".green());
    }

    // Show plan summary (exclude empty branches from PR counts)
    let creates: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_none() && !p.is_empty).collect();
    let updates: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_some() && p.needs_pr_update && !p.is_empty).collect();
    let noops: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_some() && !p.needs_pr_update && !p.needs_push && !p.is_empty).collect();

    if !quiet {
        if !creates.is_empty() {
            println!("  {} {} {} to create", creates.len().to_string().cyan(), "▸".dimmed(), if creates.len() == 1 { "PR" } else { "PRs" });
        }
        if !updates.is_empty() {
            println!("  {} {} {} to update", updates.len().to_string().cyan(), "▸".dimmed(), if updates.len() == 1 { "PR" } else { "PRs" });
        }
        if !noops.is_empty() {
            println!("  {} {} {} already up to date", noops.len().to_string().dimmed(), "▸".dimmed(), if noops.len() == 1 { "PR" } else { "PRs" });
        }
    }

    // Collect PR details for new PRs BEFORE pushing (skip empty branches)
    if !no_pr {
        let pr_template = load_pr_template(repo.workdir()?);
        let new_prs: Vec<_> = plans.iter().filter(|p| p.existing_pr.is_none() && !p.is_empty).collect();
        if !new_prs.is_empty() && !quiet {
            println!();
            println!("{}", "New PR details:".bold());
        }

        for plan in &mut plans {
            if plan.existing_pr.is_some() || plan.is_empty {
                continue;
            }

            let commit_messages =
                collect_commit_messages(repo.workdir()?, &plan.parent, &plan.branch);
            let default_title = default_pr_title(&commit_messages, &plan.branch);
            let default_body =
                build_default_pr_body(pr_template.as_deref(), &plan.branch, &commit_messages);

            if !quiet {
                println!("  {}", plan.branch.cyan());
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

            // Ask about draft vs publish (only if --draft wasn't explicitly set)
            let is_draft = if draft_flag_set {
                draft
            } else if no_prompt {
                false // default to publish in no-prompt mode
            } else {
                let options = vec!["Publish immediately", "Create as draft"];
                let choice = Select::with_theme(&ColorfulTheme::default())
                    .with_prompt("  PR type")
                    .items(&options)
                    .default(0)
                    .interact()?;
                choice == 1
            };

            plan.title = Some(title);
            plan.body = Some(body);
            plan.is_draft = Some(is_draft);
        }
    }

    // Now push branches that need it
    let branches_needing_push: Vec<_> = plans.iter().filter(|p| p.needs_push).collect();

    // Create transaction if we have branches to push
    let mut tx = if !branches_needing_push.is_empty() {
        let mut tx = Transaction::begin(OpKind::Submit, &repo, quiet)?;
        
        // Plan local branches (for backup)
        let branch_names: Vec<String> = branches_needing_push.iter().map(|p| p.branch.clone()).collect();
        tx.plan_branches(&repo, &branch_names)?;
        
        // Plan remote refs (record current remote state before pushing)
        for plan in &branches_needing_push {
            tx.plan_remote_branch(&repo, &remote_info.name, &plan.branch)?;
        }
        
        let summary = PlanSummary {
            branches_to_rebase: 0,
            branches_to_push: branches_needing_push.len(),
            description: vec![format!("Submit {} {}", branches_needing_push.len(), if branches_needing_push.len() == 1 { "branch" } else { "branches" })],
        };
        tx::print_plan(tx.kind(), &summary, quiet);
        tx.set_plan_summary(summary);
        tx.snapshot()?;
        
        Some(tx)
    } else {
        None
    };

    if !branches_needing_push.is_empty() {
        if !quiet {
            println!();
            println!("{}", "Pushing branches...".bold());
        }

        for plan in &branches_needing_push {
            if !quiet {
                print!("  {}... ", plan.branch);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            
            // Get local OID before push (this is what we're pushing)
            let local_oid = repo.branch_commit(&plan.branch).ok();
            
            match push_branch(repo.workdir()?, &remote_info.name, &plan.branch) {
                Ok(()) => {
                    // Record after-OIDs
                    if let Some(ref mut tx) = tx {
                        let _ = tx.record_after(&repo, &plan.branch);
                        if let Some(oid) = &local_oid {
                            tx.record_remote_after(&remote_info.name, &plan.branch, oid);
                        }
                    }
                    if !quiet {
                        println!("{}", "done".green());
                    }
                }
                Err(e) => {
                    if let Some(tx) = tx {
                        tx.finish_err(
                            &format!("Push failed: {}", e),
                            Some("push"),
                            Some(&plan.branch),
                        )?;
                    }
                    return Err(e);
                }
            }
        }
    }

    if no_pr {
        // Finish transaction successfully
        if let Some(tx) = tx {
            tx.finish_ok()?;
        }
        if !quiet {
            println!();
            println!("{}", "✓ Branches pushed successfully!".green().bold());
        }
        return Ok(());
    }

    // Check if anything needs to be done (exclude empty branches)
    let any_pr_work = plans.iter().any(|p| !p.is_empty && (p.existing_pr.is_none() || p.needs_pr_update));

    if !any_pr_work && branches_needing_push.is_empty() {
        if !quiet {
            println!();
            println!("{}", "✓ Stack already up to date!".green().bold());
        }
        return Ok(());
    }

    // Create/update PRs
    if any_pr_work && !quiet {
        println!();
        println!("{}", "Processing PRs...".bold());
    }

    rt.block_on(async {
        let mut pr_infos: Vec<StackPrInfo> = Vec::new();

        for plan in &plans {
            // Skip empty branches for PR operations
            if plan.is_empty {
                continue;
            }

            let meta = BranchMetadata::read(repo.inner(), &plan.branch)?
                .context(format!("No metadata for branch {}", plan.branch))?;

            if plan.existing_pr.is_none() {
                // Create new PR
                let title = plan.title.as_ref().unwrap();
                let body = plan.body.as_ref().unwrap();
                let is_draft = plan.is_draft.unwrap_or(draft);

                if !quiet {
                    print!("  Creating {}... ", plan.branch);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                }

                let pr = client
                    .create_pr(&plan.branch, &plan.parent, title, body, is_draft)
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
                    println!("{} {}", "created".green(), format!("#{}", pr.number).dimmed());
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
                });
            } else if plan.needs_pr_update {
                // Update existing PR (only if needed)
                let pr_number = plan.existing_pr.unwrap();
                if !quiet {
                    print!("  Updating {} #{}... ", plan.branch, pr_number);
                    std::io::Write::flush(&mut std::io::stdout()).ok();
                }

                // Update base if needed
                client.update_pr_base(pr_number, &plan.parent).await?;

                apply_pr_metadata(&client, pr_number, &reviewers, &labels, &assignees).await?;

                if !quiet {
                    println!("{}", "done".green());
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
                });
            } else {
                // No-op - just add to pr_infos for summary
                pr_infos.push(StackPrInfo {
                    branch: plan.branch.clone(),
                    pr_number: plan.existing_pr,
                });
            }
        }

        // Update stack comment on ALL PRs in the stack
        let prs_with_numbers: Vec<_> = pr_infos
            .iter()
            .filter_map(|p| p.pr_number.map(|num| (num, p.branch.clone())))
            .collect();

        for (pr_number, _branch) in &prs_with_numbers {
            if !quiet {
                print!("  Updating stack comment on #{}... ", pr_number);
                std::io::Write::flush(&mut std::io::stdout()).ok();
            }
            let stack_comment = generate_stack_comment(
                &pr_infos,
                *pr_number,
                &remote_info,
                &stack.trunk,
            );
            client.update_stack_comment(*pr_number, &stack_comment).await?;
            if !quiet {
                println!("{}", "done".green());
            }
        }

        if !quiet {
            println!();
            println!("{}", "✓ Stack submitted!".green().bold());

            // Print PR URLs
            if !pr_infos.is_empty() {
                for pr_info in &pr_infos {
                    if let Some(num) = pr_info.pr_number {
                        println!(
                            "  {} {}",
                            "✓".green(),
                            remote_info.pr_url(num)
                        );
                    }
                }
            }
        }

        Ok::<(), anyhow::Error>(())
    })?;

    // Finish transaction successfully
    if let Some(tx) = tx {
        tx.finish_ok()?;
    }

    Ok(())
}

fn push_branch(workdir: &std::path::Path, remote: &str, branch: &str) -> Result<()> {
    let status = Command::new("git")
        .args(["push", "-f", "-u", remote, branch])
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

/// Check if a branch needs to be pushed (local differs from remote)
fn branch_needs_push(workdir: &Path, remote: &str, branch: &str) -> bool {
    // Get local commit
    let local = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    // Get remote commit
    let remote_ref = format!("{}/{}", remote, branch);
    let remote_commit = Command::new("git")
        .args(["rev-parse", &remote_ref])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    match (local, remote_commit) {
        (Some(l), Some(r)) => l != r, // Need push if different
        (Some(_), None) => true,       // Branch not on remote yet
        _ => true,                     // Default to push if unsure
    }
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
