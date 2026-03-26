use crate::commands::restack_conflict::{print_restack_conflict, RestackConflictContext};
use crate::commands::restack_parent::normalize_scope_parents_for_restack;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::io::IsTerminal;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitAfterRestack {
    Ask,
    Yes,
    No,
}

#[allow(clippy::too_many_arguments)]
pub fn run(
    all: bool,
    stop_here: bool,
    r#continue: bool,
    dry_run: bool,
    yes: bool,
    quiet: bool,
    auto_stash_pop: bool,
    submit_after: SubmitAfterRestack,
) -> Result<()> {
    let repo = GitRepo::open()?;

    if r#continue {
        crate::commands::continue_cmd::run()?;
        if repo.rebase_in_progress()? {
            return Ok(());
        }
    }

    run_impl(
        &repo,
        all,
        stop_here,
        dry_run,
        yes,
        quiet,
        auto_stash_pop,
        submit_after,
        r#continue,
        None,
    )
}

pub(crate) fn resume_after_rebase(
    auto_stash_pop: bool,
    restore_branch: Option<String>,
) -> Result<()> {
    let repo = GitRepo::open()?;
    run_impl(
        &repo,
        false,
        false,
        false,
        true,
        false,
        auto_stash_pop,
        SubmitAfterRestack::No,
        true,
        restore_branch,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_impl(
    repo: &GitRepo,
    all: bool,
    stop_here: bool,
    dry_run: bool,
    yes: bool,
    quiet: bool,
    auto_stash_pop: bool,
    submit_after: SubmitAfterRestack,
    skip_prediction: bool,
    restore_branch: Option<String>,
) -> Result<()> {
    let current = repo.current_branch()?;
    let restore_branch = restore_branch.unwrap_or_else(|| current.clone());
    let mut stack = Stack::load(repo)?;

    let mut stashed = false;
    if repo.is_dirty()? {
        if auto_stash_pop {
            stashed = repo.stash_push()?;
            if stashed && !quiet {
                println!("{}", "✓ Stashed working tree changes.".green());
            }
        } else if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        } else {
            let stash = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Working tree has uncommitted changes. Stash them before restack?")
                .default(true)
                .interact()?;

            if stash {
                stashed = repo.stash_push()?;
                println!("{}", "✓ Stashed working tree changes.".green());
            } else {
                println!("{}", "Aborted.".red());
                return Ok(());
            }
        }
    }

    // Determine the operation scope once, then evaluate restack status live per branch.
    let mut scope_branches: Vec<String> = if all {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    } else if stop_here {
        // Current stack up to the current branch: ancestors + current, excluding descendants.
        let mut branches = stack.ancestors(&current);
        branches.reverse();
        branches.retain(|branch| branch != &stack.trunk);
        if current != stack.trunk {
            branches.push(current.clone());
        }
        branches
    } else {
        // Current stack: ancestors + current + descendants, excluding trunk.
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    };

    if all {
        // Parent-first ordering minimizes repeated rebases across unrelated stacks.
        scope_branches.sort_by(|a, b| {
            stack
                .ancestors(a)
                .len()
                .cmp(&stack.ancestors(b).len())
                .then_with(|| a.cmp(b))
        });
    }

    let normalized = normalize_scope_parents_for_restack(repo, &scope_branches, quiet)?;
    if normalized > 0 {
        stack = Stack::load(repo)?;
    }

    let branches_to_restack = branches_needing_restack(&stack, &scope_branches);

    if branches_to_restack.is_empty() {
        if !quiet {
            println!("{}", "✓ Stack is up to date, nothing to restack.".green());
        }
        if stashed {
            repo.stash_pop()?;
        }
        return Ok(());
    }

    // Predict conflicts before proceeding
    if !skip_prediction {
        let timer = LiveTimer::maybe_new(!quiet, "Checking for conflicts...");
        let branch_parent_pairs: Vec<(String, String)> = branches_to_restack
            .iter()
            .filter_map(|b| {
                BranchMetadata::read(repo.inner(), b)
                    .ok()
                    .flatten()
                    .map(|m| (b.clone(), m.parent_branch_name.clone()))
            })
            .collect();
        let predictions = repo.predict_restack_conflicts(&branch_parent_pairs);

        if predictions.is_empty() {
            LiveTimer::maybe_finish_ok(timer, "no conflicts predicted");
        } else {
            LiveTimer::maybe_finish_warn(
                timer,
                &format!("{} branch(es) with conflicts", predictions.len()),
            );
            println!();
            for p in &predictions {
                println!(
                    "  {} {} → {}",
                    "✗".red(),
                    p.branch.yellow().bold(),
                    p.onto.dimmed()
                );
                for file in &p.conflicting_files {
                    println!("    {} {}", "│".dimmed(), file.red());
                }
            }
            println!();
        }

        if dry_run {
            if stashed {
                repo.stash_pop()?;
            }
            return Ok(());
        }

        if !predictions.is_empty() && !yes {
            let confirm = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Conflicts predicted. Continue with restack?")
                .default(true)
                .interact()?;
            if !confirm {
                if stashed {
                    repo.stash_pop()?;
                }
                return Ok(());
            }
        }
    }

    let branch_word = if scope_branches.len() == 1 {
        "branch"
    } else {
        "branches"
    };
    if !quiet {
        println!(
            "Restacking up to {} {}...",
            scope_branches.len().to_string().cyan(),
            branch_word
        );
    }

    // Begin transaction
    let mut tx = Transaction::begin(OpKind::Restack, repo, quiet)?;
    tx.plan_branches(repo, &scope_branches)?;
    let summary = PlanSummary {
        branches_to_rebase: scope_branches.len(),
        branches_to_push: 0,
        description: vec![format!(
            "Restack up to {} {}",
            scope_branches.len(),
            branch_word
        )],
    };
    tx::print_plan(tx.kind(), &summary, quiet);
    tx.set_plan_summary(summary);
    tx.set_auto_stash_pop(auto_stash_pop);
    tx.snapshot()?;

    let mut summary: Vec<(String, String)> = Vec::new();

    for (index, branch) in scope_branches.iter().enumerate() {
        let live_stack = Stack::load(repo)?;
        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|br| br.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        // Get metadata
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        let restack_timer = LiveTimer::maybe_new(
            !quiet,
            &format!("{} onto {}", branch, meta.parent_branch_name),
        );

        // Rebase using provenance-aware upstream inference to avoid replaying
        // already-integrated commits after squash/cherry-pick merges.
        match repo.rebase_branch_onto_with_provenance(
            branch,
            &meta.parent_branch_name,
            &meta.parent_branch_revision,
            auto_stash_pop,
        )? {
            RebaseResult::Success => {
                // Update metadata with new parent revision
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), branch)?;

                // Record the after-OID for this branch
                tx.record_after(repo, branch)?;

                LiveTimer::maybe_finish_ok(restack_timer, "done");
                summary.push((branch.clone(), "ok".to_string()));
            }
            RebaseResult::Conflict => {
                LiveTimer::maybe_finish_err(restack_timer, "conflict");
                let completed_branches: Vec<String> = summary
                    .iter()
                    .filter(|(_, status)| status == "ok")
                    .map(|(name, _)| name.clone())
                    .collect();
                print_restack_conflict(
                    repo,
                    &RestackConflictContext {
                        branch,
                        parent_branch: &meta.parent_branch_name,
                        completed_branches: &completed_branches,
                        remaining_branches: scope_branches.len().saturating_sub(index + 1),
                        continue_commands: &[
                            "stax resolve",
                            "stax continue",
                            "stax restack --continue",
                        ],
                    },
                );
                if stashed {
                    println!("{}", "Stash kept to avoid conflicts.".yellow());
                }
                summary.push((branch.clone(), "conflict".to_string()));

                // Finish transaction with error
                tx.finish_err("Rebase conflict", Some("rebase"), Some(branch))?;

                return Ok(());
            }
        }
    }

    // Return to original branch
    repo.checkout(&restore_branch)?;

    // Finish transaction successfully
    tx.finish_ok()?;

    if !quiet {
        println!();
        println!("{}", "✓ Stack restacked successfully!".green());
    }

    if !quiet && !summary.is_empty() {
        println!();
        println!("{}", "Restack summary:".dimmed());
        for (branch, status) in &summary {
            let symbol = if status == "ok" { "✓" } else { "✗" };
            println!("  {} {} {}", symbol, branch, status);
        }
    }

    // Check for merged branches and offer to delete them
    cleanup_merged_branches(repo, quiet, yes)?;

    if stashed {
        repo.stash_pop()?;
        if !quiet {
            println!("{}", "✓ Restored stashed changes.".green());
        }
    }

    let should_submit = should_submit_after_restack(&summary, quiet, submit_after)?;

    if should_submit {
        submit_after_restack(quiet)?;
    }

    Ok(())
}

fn branches_needing_restack(stack: &Stack, scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|branch| {
            stack
                .branches
                .get(*branch)
                .map(|b| b.needs_restack)
                .unwrap_or(false)
        })
        .cloned()
        .collect()
}

/// Check for merged branches and prompt to delete them
fn cleanup_merged_branches(repo: &GitRepo, quiet: bool, auto_confirm: bool) -> Result<()> {
    if quiet {
        return Ok(());
    }

    let workdir = repo.workdir()?;

    // Only check stax-tracked branches (not all local branches) for merge status.
    // Also exclude the currently checked-out branch — we never offer to delete it.
    let stack = Stack::load(repo)?;
    let current = repo.current_branch()?;
    let tracked: Vec<String> = stack
        .branches
        .keys()
        .filter(|b| *b != &stack.trunk && *b != &current)
        .cloned()
        .collect();

    let timer = LiveTimer::maybe_new(!quiet, "Checking for merged branches...");
    let mut merged = Vec::new();
    for branch in &tracked {
        if repo
            .is_branch_merged_equivalent_to_trunk(branch)
            .unwrap_or(false)
        {
            merged.push(branch.clone());
        }
    }
    LiveTimer::maybe_finish_timed(timer);

    if merged.is_empty() {
        return Ok(());
    }

    let branch_word = if merged.len() == 1 {
        "branch"
    } else {
        "branches"
    };

    println!();
    println!(
        "{}",
        format!("Found {} merged {}:", merged.len(), branch_word).dimmed()
    );
    for branch in &merged {
        println!("  {} {}", "▸".bright_black(), branch.yellow());
    }
    println!();

    let confirm = if auto_confirm {
        true
    } else {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Delete {} merged {}?", merged.len(), branch_word))
            .default(true)
            .interact()?
    };

    if !confirm {
        return Ok(());
    }

    for branch in &merged {
        let live_stack = Stack::load(repo)?;
        let recorded_parent_branch = live_stack
            .branches
            .get(branch)
            .and_then(|b| b.parent.clone())
            .unwrap_or_else(|| live_stack.trunk.clone());
        let parent_branch = if repo.branch_commit(&recorded_parent_branch).is_ok() {
            recorded_parent_branch.clone()
        } else if recorded_parent_branch != live_stack.trunk
            && repo.branch_commit(&live_stack.trunk).is_ok()
        {
            live_stack.trunk.clone()
        } else {
            recorded_parent_branch.clone()
        };

        let children: Vec<String> = live_stack
            .branches
            .iter()
            .filter(|(_, info)| info.parent.as_deref() == Some(branch.as_str()))
            .map(|(name, _)| name.clone())
            .collect();

        if !children.is_empty() && repo.branch_commit(&parent_branch).is_err() {
            println!(
                "  {} {}",
                "⚠".yellow(),
                format!(
                    "Skipped deleting {}: couldn't resolve local fallback parent '{}'.",
                    branch, parent_branch
                )
                .dimmed()
            );
            continue;
        }

        let merged_branch_tip = repo.branch_commit(branch).ok();
        for child in &children {
            if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
                let old_parent_boundary = merged_branch_tip
                    .clone()
                    .unwrap_or_else(|| child_meta.parent_branch_revision.clone());
                let updated_meta = BranchMetadata {
                    parent_branch_name: parent_branch.clone(),
                    parent_branch_revision: old_parent_boundary,
                    ..child_meta
                };
                updated_meta.write(repo.inner(), child)?;
                println!(
                    "  {} {}",
                    "↪".cyan(),
                    format!("Reparented {} → {}", child, parent_branch).dimmed()
                );
            }
        }

        let branch_existed_before = local_branch_exists(workdir, branch);
        let delete_output = if branch_existed_before {
            Some(
                Command::new("git")
                    .args(["branch", "-D", branch])
                    .current_dir(workdir)
                    .output(),
            )
        } else {
            None
        };

        let (local_deleted, local_worktree_blocked) = match delete_output {
            Some(Ok(out)) => {
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                (out.status.success(), stderr.contains("used by worktree"))
            }
            Some(Err(_)) | None => (false, false),
        };

        let local_still_exists = local_branch_exists(workdir, branch);
        let metadata_deleted = if !local_still_exists {
            let _ = BranchMetadata::delete(repo.inner(), branch);
            true
        } else {
            false
        };

        if local_deleted {
            println!(
                "  {} {}",
                "✓".green(),
                format!("Deleted {}", branch).dimmed()
            );
        } else if !branch_existed_before || !local_still_exists {
            println!(
                "  {} {}",
                "✓".green(),
                format!("{} already absent locally", branch).dimmed()
            );
            if metadata_deleted {
                println!(
                    "  {} {}",
                    "↷".cyan(),
                    format!("Removed metadata for {}", branch).dimmed()
                );
            }
        } else if local_worktree_blocked {
            println!(
                "  {} {}",
                "⚠".yellow(),
                format!(
                    "Kept {}: branch is checked out in another worktree.",
                    branch
                )
                .dimmed()
            );
            if let Ok(Some(resolution)) = repo.branch_delete_resolution(branch) {
                if let Some(remove_cmd) = resolution.remove_worktree_cmd() {
                    println!(
                        "  {} {}",
                        "↷".yellow(),
                        "Run to remove that worktree:".dimmed()
                    );
                    println!("    {}", remove_cmd.cyan());
                }
                println!(
                    "  {} {}",
                    "↷".yellow(),
                    if resolution.worktree.is_main {
                        "Run to free the branch in the main worktree:".dimmed()
                    } else {
                        "Or keep the worktree and free the branch:".dimmed()
                    }
                );
                println!("    {}", resolution.switch_branch_cmd().cyan());
            }
            println!(
                "  {} {}",
                "↷".yellow(),
                "Metadata kept because the local branch still exists.".dimmed()
            );
        } else {
            println!(
                "  {} {}",
                "○".dimmed(),
                format!("Skipped {}", branch).dimmed()
            );
            if !metadata_deleted {
                println!(
                    "  {} {}",
                    "↷".yellow(),
                    "Metadata kept because the local branch still exists.".dimmed()
                );
            }
        }
    }

    Ok(())
}

fn local_branch_exists(workdir: &Path, branch: &str) -> bool {
    let local_ref = format!("refs/heads/{}", branch);
    Command::new("git")
        .args(["show-ref", "--verify", "--quiet", &local_ref])
        .current_dir(workdir)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn should_submit_after_restack(
    summary: &[(String, String)],
    quiet: bool,
    submit_after: SubmitAfterRestack,
) -> Result<bool> {
    // Offer submit only if at least one branch was successfully rebased.
    if !summary.iter().any(|(_, status)| status == "ok") {
        return Ok(false);
    }

    let should_submit = match submit_after {
        SubmitAfterRestack::Yes => true,
        SubmitAfterRestack::No => false,
        SubmitAfterRestack::Ask => {
            if quiet || !std::io::stdin().is_terminal() {
                return Ok(false);
            }

            println!();
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Submit stack now (`stax ss`)?")
                .default(true)
                .interact()?
        }
    };

    Ok(should_submit)
}

fn submit_after_restack(quiet: bool) -> Result<()> {
    if !quiet {
        println!();
    }

    crate::commands::submit::run(
        crate::commands::submit::SubmitScope::Stack,
        false,  // draft
        false,  // no_pr
        false,  // no_fetch
        false,  // force
        true,   // yes
        true,   // no_prompt
        vec![], // reviewers
        vec![], // labels
        vec![], // assignees
        quiet,
        false, // open
        false, // verbose
        None,  // template
        false, // no_template
        false, // edit
        false, // ai_body
        false, // rerequest_review
    )?;

    Ok(())
}
