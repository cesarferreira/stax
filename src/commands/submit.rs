use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::github::pr::{generate_stack_comment, PrInfoWithHead, StackPrInfo};
use crate::github::pr_template::{discover_pr_templates, select_template_interactive};
use crate::github::GitHubClient;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::remote::{self, RemoteInfo};
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Editor, Input, Select};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitScope {
    Branch,
    Downstack,
    Upstack,
    Stack,
}

impl SubmitScope {
    fn label(self) -> &'static str {
        match self {
            SubmitScope::Branch => "branch",
            SubmitScope::Downstack => "downstack",
            SubmitScope::Upstack => "upstack",
            SubmitScope::Stack => "stack",
        }
    }
}

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
    scope: SubmitScope,
    draft: bool,
    no_pr: bool,
    _force: bool, // kept for CLI compatibility
    yes: bool,
    no_prompt: bool,
    reviewers: Vec<String>,
    labels: Vec<String>,
    assignees: Vec<String>,
    quiet: bool,
    verbose: bool,
    template: Option<String>,
    no_template: bool,
    edit: bool,
    ai_body: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let config = Config::load()?;
    let _ = yes; // Used for future auto-confirm features

    // Track if --draft was explicitly passed (we'll ask interactively if not)
    let draft_flag_set = draft;

    if matches!(scope, SubmitScope::Branch) && current == stack.trunk {
        anyhow::bail!(
            "Cannot submit trunk '{}' as a single branch.\n\
             Checkout a tracked branch and run `stax branch submit`, or run `stax submit` for the whole stack.",
            stack.trunk
        );
    }

    let branches_to_submit = resolve_branches_for_scope(&stack, &current, scope);
    if branches_to_submit.is_empty() {
        if !quiet {
            println!("{}", "No tracked branches to submit.".yellow());
        }
        return Ok(());
    }

    // Validation phase
    if !quiet {
        println!("{} {}...", "Submitting".bold(), scope.label().bold());
    }

    // Check for needs restack - show warning but continue (like fp)
    let needs_restack: Vec<_> = branches_to_submit
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
            println!("  {} {} needs restack", "!".yellow(), b.cyan());
        }
    }

    // Check for branches with no changes (empty branches)
    let empty_branches: Vec<_> = branches_to_submit
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
    let empty_set: HashSet<_> = empty_branches.iter().cloned().collect();

    if !empty_branches.is_empty() && !quiet {
        println!("  {} Empty branches (will push, skip PR):", "!".yellow());
        for b in &empty_branches {
            println!("    {}", b.dimmed());
        }
    }

    let remote_info = RemoteInfo::from_repo(&repo, &config)?;

    let owner = remote_info.owner().to_string();
    let repo_name = remote_info.repo.clone();

    // Fetch to ensure we have latest remote refs (non-fatal if it fails)
    if !quiet {
        print!("  Fetching from {}... ", remote_info.name);
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }
    match remote::fetch_remote(repo.workdir()?, &remote_info.name) {
        Ok(()) => {
            if !quiet {
                println!("{}", "done".green());
            }
        }
        Err(_) => {
            if !quiet {
                println!("{} (continuing with local refs)", "skipped".yellow());
            }
        }
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

    if matches!(scope, SubmitScope::Branch | SubmitScope::Upstack) {
        validate_narrow_scope_submit(
            scope,
            &repo,
            &stack,
            &current,
            &remote_info.name,
            &branches_to_submit,
        )?;
    }

    // Build plan - determine which PRs need create vs update
    if !quiet {
        print!("  Planning PR operations... ");
        std::io::Write::flush(&mut std::io::stdout()).ok();
    }

    let mut plans: Vec<PrPlan> = Vec::new();
    let mut rt: Option<tokio::runtime::Runtime> = None;
    let mut client: Option<GitHubClient> = None;

    if no_pr {
        let runtime = tokio::runtime::Runtime::new().ok();
        let gh_client = runtime.as_ref().and_then(|runtime| {
            runtime
                .block_on(async {
                    GitHubClient::new(&owner, &repo_name, remote_info.api_base_url.clone())
                })
                .ok()
        });
        let mut open_prs_by_head: Option<HashMap<String, PrInfoWithHead>> = None;

        for branch in &branches_to_submit {
            let mut meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;
            let is_empty = empty_set.contains(branch);
            let needs_push = branch_needs_push(repo.workdir()?, &remote_info.name, branch);
            let mut existing_pr = None;

            // Best-effort metadata refresh when no-pr is used.
            if !is_empty {
                if let (Some(runtime), Some(gh_client)) = (runtime.as_ref(), gh_client.as_ref()) {
                    let mut found_pr: Option<PrInfoWithHead> = None;

                    if let Some(pr_info) = meta.pr_info.as_ref().filter(|p| p.number > 0) {
                        found_pr = runtime
                            .block_on(async { gh_client.get_pr_with_head(pr_info.number).await })
                            .ok();
                    }

                    if found_pr.is_none() {
                        if open_prs_by_head.is_none() {
                            open_prs_by_head = runtime
                                .block_on(async { gh_client.list_open_prs_by_head().await })
                                .ok();
                        }
                        if let Some(map) = &open_prs_by_head {
                            found_pr = map.get(branch).cloned();
                        }
                    }

                    if let Some(pr) = found_pr {
                        existing_pr = Some(pr.info.number);
                        let owner_matches = pr
                            .head_label
                            .as_ref()
                            .and_then(|label| label.split_once(':').map(|(owner, _)| owner))
                            .map(|owner| owner == remote_info.owner())
                            .unwrap_or(false);

                        let needs_meta_update = meta
                            .pr_info
                            .as_ref()
                            .map(|info| {
                                info.number != pr.info.number
                                    || info.state != pr.info.state
                                    || info.is_draft.unwrap_or(false) != pr.info.is_draft
                            })
                            .unwrap_or(true);

                        if needs_meta_update && owner_matches {
                            meta = BranchMetadata {
                                pr_info: Some(crate::engine::metadata::PrInfo {
                                    number: pr.info.number,
                                    state: pr.info.state.clone(),
                                    is_draft: Some(pr.info.is_draft),
                                }),
                                ..meta
                            };
                            meta.write(repo.inner(), branch)?;
                        }
                    }
                }
            }

            plans.push(PrPlan {
                branch: branch.clone(),
                parent: meta.parent_branch_name,
                existing_pr,
                title: None,
                body: None,
                is_draft: None,
                needs_push,
                needs_pr_update: false,
                is_empty,
            });
        }
    } else {
        let runtime = tokio::runtime::Runtime::new()?;
        let gh_client = runtime.block_on(async {
            GitHubClient::new(&owner, &repo_name, remote_info.api_base_url.clone())
        })?;
        let mut open_prs_by_head: Option<HashMap<String, PrInfoWithHead>> = None;

        for branch in &branches_to_submit {
            let meta = BranchMetadata::read(repo.inner(), branch)?
                .context(format!("No metadata for branch {}", branch))?;

            let is_empty = empty_set.contains(branch);

            // Check if PR exists (skip for empty branches)
            let mut existing_pr: Option<PrInfoWithHead> = None;
            if !is_empty {
                if verbose && !quiet {
                    println!("    Checking PR for {}", branch.cyan());
                }

                if let Some(pr_info) = meta.pr_info.as_ref().filter(|p| p.number > 0) {
                    if verbose && !quiet {
                        println!("      Using metadata PR #{}", pr_info.number);
                    }

                    match runtime
                        .block_on(async { gh_client.get_pr_with_head(pr_info.number).await })
                    {
                        Ok(pr) => {
                            if pr.head == *branch && pr.info.state == "Open" {
                                existing_pr = Some(pr);
                            } else if verbose && !quiet {
                                println!(
                                    "      PR #{} head '{}' does not match '{}', falling back",
                                    pr_info.number, pr.head, branch
                                );
                            }
                        }
                        Err(_) => {
                            if verbose && !quiet {
                                println!(
                                    "      Failed to fetch PR #{} from metadata, falling back",
                                    pr_info.number
                                );
                            }
                        }
                    }
                }

                if existing_pr.is_none() {
                    if open_prs_by_head.is_none() {
                        if verbose && !quiet {
                            println!("      Listing open PRs...");
                        }
                        let prs =
                            runtime.block_on(async { gh_client.list_open_prs_by_head().await })?;
                        if verbose && !quiet {
                            println!("      Cached {} open PRs", prs.len());
                        }
                        open_prs_by_head = Some(prs);
                    }
                    if let Some(map) = &open_prs_by_head {
                        existing_pr = map.get(branch).cloned();
                        if verbose && !quiet {
                            if let Some(found) = &existing_pr {
                                println!("      Found open PR #{} in list", found.info.number);
                            } else {
                                println!("      No open PR found in list");
                            }
                        }
                    }
                }
            } else if verbose && !quiet {
                println!("    Empty branch {}, skipping PR lookup", branch.cyan());
            }
            let pr_number = existing_pr.as_ref().map(|p| p.info.number);

            if let Some(pr) = &existing_pr {
                let owner_matches = pr
                    .head_label
                    .as_ref()
                    .and_then(|label| label.split_once(':').map(|(owner, _)| owner))
                    .map(|owner| owner == remote_info.owner())
                    .unwrap_or(false);

                let needs_meta_update = meta
                    .pr_info
                    .as_ref()
                    .map(|info| {
                        info.number != pr.info.number
                            || info.state != pr.info.state
                            || info.is_draft.unwrap_or(false) != pr.info.is_draft
                    })
                    .unwrap_or(true);

                if needs_meta_update && owner_matches {
                    let updated_meta = BranchMetadata {
                        pr_info: Some(crate::engine::metadata::PrInfo {
                            number: pr.info.number,
                            state: pr.info.state.clone(),
                            is_draft: Some(pr.info.is_draft),
                        }),
                        ..meta.clone()
                    };
                    updated_meta.write(repo.inner(), branch)?;
                    if verbose && !quiet {
                        println!("      Cached PR #{} in metadata", pr.info.number);
                    }
                } else if needs_meta_update && verbose && !quiet {
                    println!(
                        "      Skipped caching PR #{} (fork or unknown owner)",
                        pr.info.number
                    );
                }
            }

            // Determine the base branch for PR
            let base = meta.parent_branch_name.clone();

            // Check if we actually need to push
            let needs_push = branch_needs_push(repo.workdir()?, &remote_info.name, branch);

            // Check if PR base needs updating (not for empty branches)
            let needs_pr_update = if is_empty {
                false
            } else if let Some(pr) = &existing_pr {
                pr.info.base != base || needs_push
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

        rt = Some(runtime);
        client = Some(gh_client);
    }

    if !quiet {
        println!("{}", "done".green());
    }

    // Show plan summary (exclude empty branches from PR counts)
    let creates: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_none() && !p.is_empty)
        .collect();
    let updates: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_some() && p.needs_pr_update && !p.is_empty)
        .collect();
    let noops: Vec<_> = plans
        .iter()
        .filter(|p| p.existing_pr.is_some() && !p.needs_pr_update && !p.needs_push && !p.is_empty)
        .collect();

    if !quiet {
        if !creates.is_empty() {
            println!(
                "  {} {} {} to create",
                creates.len().to_string().cyan(),
                "▸".dimmed(),
                if creates.len() == 1 { "PR" } else { "PRs" }
            );
        }
        if !updates.is_empty() {
            println!(
                "  {} {} {} to update",
                updates.len().to_string().cyan(),
                "▸".dimmed(),
                if updates.len() == 1 { "PR" } else { "PRs" }
            );
        }
        if !noops.is_empty() {
            println!(
                "  {} {} {} already up to date",
                noops.len().to_string().dimmed(),
                "▸".dimmed(),
                if noops.len() == 1 { "PR" } else { "PRs" }
            );
        }
    }

    // Collect PR details for new PRs BEFORE pushing (skip empty branches)
    if !no_pr {
        // Discover all available PR templates
        let discovered_templates = if no_template {
            Vec::new()
        } else {
            discover_pr_templates(repo.workdir()?).unwrap_or_default()
        };
        let new_prs: Vec<_> = plans
            .iter()
            .filter(|p| p.existing_pr.is_none() && !p.is_empty)
            .collect();
        if !new_prs.is_empty() && !quiet {
            println!();
            println!("{}", "New PR details:".bold());
        }

        for plan in &mut plans {
            if plan.existing_pr.is_some() || plan.is_empty {
                continue;
            }

            // Template selection per branch
            let selected_template = if no_template {
                None
            } else if let Some(ref template_name) = template {
                // --template flag: find by name
                let found = discovered_templates
                    .iter()
                    .find(|t| t.name == *template_name)
                    .cloned();

                if found.is_none() && !quiet {
                    eprintln!(
                        "  {} Template '{}' not found, using no template",
                        "!".yellow(),
                        template_name
                    );
                }
                found
            } else if no_prompt {
                // --no-prompt: use first template if exactly one exists
                if discovered_templates.len() == 1 {
                    Some(discovered_templates[0].clone())
                } else {
                    None
                }
            } else {
                // Interactive selection (handles empty list, single template, and multiple)
                select_template_interactive(&discovered_templates)?
            };

            let commit_messages =
                collect_commit_messages(repo.workdir()?, &plan.parent, &plan.branch);
            let default_title = default_pr_title(&commit_messages, &plan.branch);

            // Use selected template content if available
            let template_content = selected_template.as_ref().map(|t| t.content.as_str());
            let default_body =
                build_default_pr_body(template_content, &plan.branch, &commit_messages);

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

            let body = if ai_body {
                // --ai-body flag: generate body using AI agent
                if !quiet {
                    println!("    {}", "Generating PR body with AI...".dimmed());
                }
                let ai_body_result = generate_ai_body(
                    repo.workdir()?,
                    &plan.parent,
                    &plan.branch,
                    template_content,
                );
                match ai_body_result {
                    Ok(generated) => {
                        if edit {
                            Editor::new().edit(&generated)?.unwrap_or(generated)
                        } else {
                            generated
                        }
                    }
                    Err(e) => {
                        if !quiet {
                            eprintln!(
                                "    {} AI generation failed: {}. Falling back to default.",
                                "⚠".yellow(),
                                e
                            );
                        }
                        default_body
                    }
                }
            } else if no_prompt {
                default_body
            } else if edit {
                // --edit flag: always open editor
                Editor::new().edit(&default_body)?.unwrap_or(default_body)
            } else {
                // Interactive prompt
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
                    "Edit" => Editor::new().edit(&default_body)?.unwrap_or(default_body),
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
        let branch_names: Vec<String> = branches_needing_push
            .iter()
            .map(|p| p.branch.clone())
            .collect();
        tx.plan_branches(&repo, &branch_names)?;

        // Plan remote refs (record current remote state before pushing)
        for plan in &branches_needing_push {
            tx.plan_remote_branch(&repo, &remote_info.name, &plan.branch)?;
        }

        let summary = PlanSummary {
            branches_to_rebase: 0,
            branches_to_push: branches_needing_push.len(),
            description: vec![format!(
                "Submit {} {}",
                branches_needing_push.len(),
                if branches_needing_push.len() == 1 {
                    "branch"
                } else {
                    "branches"
                }
            )],
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
    let any_pr_work = plans
        .iter()
        .any(|p| !p.is_empty && (p.existing_pr.is_none() || p.needs_pr_update));

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

    let rt = rt.context("Internal error: missing runtime for PR submission")?;
    let client = client.context("Internal error: missing GitHub client for PR submission")?;

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
                    println!(
                        "{} {}",
                        "created".green(),
                        format!("#{}", pr.number).dimmed()
                    );
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
            let stack_comment =
                generate_stack_comment(&pr_infos, *pr_number, &remote_info, &stack.trunk);
            client
                .update_stack_comment(*pr_number, &stack_comment)
                .await?;
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
                        println!("  {} {}", "✓".green(), remote_info.pr_url(num));
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

fn resolve_branches_for_scope(stack: &Stack, current: &str, scope: SubmitScope) -> Vec<String> {
    let branches = match scope {
        SubmitScope::Stack => stack.current_stack(current),
        SubmitScope::Downstack => {
            let mut ancestors = stack.ancestors(current);
            ancestors.reverse();
            ancestors.push(current.to_string());
            ancestors
        }
        SubmitScope::Upstack => {
            let mut upstack = vec![current.to_string()];
            upstack.extend(stack.descendants(current));
            upstack
        }
        SubmitScope::Branch => vec![current.to_string()],
    };

    branches
        .into_iter()
        .filter(|branch| branch != &stack.trunk)
        .collect()
}

fn validate_narrow_scope_submit(
    scope: SubmitScope,
    repo: &GitRepo,
    stack: &Stack,
    current: &str,
    remote_name: &str,
    branches_to_submit: &[String],
) -> Result<()> {
    if matches!(scope, SubmitScope::Branch) && current == stack.trunk {
        anyhow::bail!(
            "Cannot submit trunk '{}' as a single branch.\n\
             Checkout a tracked branch and run `stax branch submit`, or run `stax submit` for the whole stack.",
            stack.trunk
        );
    }

    let current_meta = BranchMetadata::read(repo.inner(), current)?;
    if current != stack.trunk && current_meta.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked by stax.\n\
             Use `stax branch track --parent <branch>` (or `stax branch reparent`) and retry.",
            current
        );
    }

    let submitted: HashSet<&str> = branches_to_submit.iter().map(String::as_str).collect();

    for branch in branches_to_submit {
        let meta = BranchMetadata::read(repo.inner(), branch)?
            .context(format!("No metadata for branch {}", branch))?;
        let parent = meta.parent_branch_name;

        if parent == stack.trunk || submitted.contains(parent.as_str()) {
            continue;
        }

        let needs_restack = stack
            .branches
            .get(branch)
            .map(|b| b.needs_restack)
            .unwrap_or(false);
        if needs_restack {
            anyhow::bail!(
                "Branch '{}' needs restack before scoped submit.\n\
                 Run `stax restack` or submit with ancestor scope: `stax downstack submit` / `stax submit`.",
                branch
            );
        }

        if !branch_matches_remote(repo.workdir()?, remote_name, &parent) {
            anyhow::bail!(
                "Parent branch '{}' is not in sync with '{}/{}'.\n\
                 Narrow scope submit for '{}' is unsafe because its parent is excluded.\n\
                 Run `stax downstack submit` or `stax submit` to include ancestors first.",
                parent,
                remote_name,
                parent,
                branch
            );
        }
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
        (Some(_), None) => true,      // Branch not on remote yet
        _ => true,                    // Default to push if unsure
    }
}

fn branch_matches_remote(workdir: &Path, remote: &str, branch: &str) -> bool {
    let local = Command::new("git")
        .args(["rev-parse", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let remote_ref = format!("{}/{}", remote, branch);
    let remote_commit = Command::new("git")
        .args(["rev-parse", &remote_ref])
        .current_dir(workdir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    match (local, remote_commit) {
        (Some(l), Some(r)) => l == r,
        _ => false,
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

// Deprecated: Use github::pr_template::discover_pr_templates instead
#[allow(dead_code)]
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

/// Generate a PR body using an AI agent (for --ai-body flag).
/// Loads agent/model from config, collects diff and commits, invokes the AI CLI.
fn generate_ai_body(
    workdir: &Path,
    parent: &str,
    branch: &str,
    template: Option<&str>,
) -> Result<String> {
    use super::generate;

    let config = Config::load()?;
    let agent = config
        .ai
        .agent
        .as_deref()
        .filter(|a| !a.is_empty())
        .context(
            "No AI agent configured. Run `stax generate --pr-body` first to set up, \
             or add [ai] agent = \"claude\" (or \"codex\" / \"gemini\" / \"opencode\") to ~/.config/stax/config.toml",
        )?
        .to_string();

    let model = config.ai.model.clone();

    let diff_stat = generate::get_diff_stat(workdir, parent, branch);
    let diff = generate::get_full_diff(workdir, parent, branch);
    let commits = collect_commit_messages(workdir, parent, branch);
    let prompt = generate::build_ai_prompt(&diff_stat, &diff, &commits, template);

    generate::invoke_ai_agent(&agent, model.as_deref(), &prompt)
}
