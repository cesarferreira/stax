use crate::commands::restack_conflict::{print_restack_conflict, RestackConflictContext};
use crate::engine::{BranchMetadata, Stack};
use crate::errors::ConflictStopped;
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use crate::progress::LiveTimer;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::collections::HashSet;
use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;

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

    let mut completed_from_receipt: HashSet<String> = HashSet::new();

    if r#continue {
        crate::commands::continue_cmd::run()?;
        if repo.rebase_in_progress()? {
            return Ok(());
        }

        // Recover metadata + completed list from the failed receipt.
        if let Some(receipt) = crate::commands::continue_cmd::latest_failed_restack_receipt(&repo)?
        {
            completed_from_receipt.extend(receipt.completed_branches.iter().cloned());

            // If the user finished the rebase via `git rebase --continue`
            // directly, the failed branch's metadata was never updated.
            if let Some(failed_branch) = receipt
                .error
                .as_ref()
                .and_then(|e| e.failed_branch.as_deref())
            {
                if let Some(meta) = BranchMetadata::read(repo.inner(), failed_branch)? {
                    if let Ok(actual_parent_rev) = repo.branch_commit(&meta.parent_branch_name) {
                        if meta.parent_branch_revision != actual_parent_rev {
                            let updated = BranchMetadata {
                                parent_branch_revision: actual_parent_rev,
                                ..meta
                            };
                            updated.write(repo.inner(), failed_branch)?;
                        }
                    }
                }
                completed_from_receipt.insert(failed_branch.to_string());
            }
        }
    }

    let _ = yes; // `yes` is preserved in the CLI API but no longer used during restack itself
    run_impl(
        &repo,
        all,
        stop_here,
        dry_run,
        quiet,
        auto_stash_pop,
        submit_after,
        None,
        completed_from_receipt,
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
        false,
        auto_stash_pop,
        SubmitAfterRestack::No,
        restore_branch,
        HashSet::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn run_impl(
    repo: &GitRepo,
    all: bool,
    stop_here: bool,
    dry_run: bool,
    quiet: bool,
    mut auto_stash_pop: bool,
    submit_after: SubmitAfterRestack,
    restore_branch: Option<String>,
    completed_from_receipt: HashSet<String>,
) -> Result<()> {
    let current = repo.current_branch()?;
    let current_workdir = normalized_workdir(repo)?;
    let restore_branch = restore_branch.unwrap_or_else(|| current.clone());
    let stack = Stack::load(repo)?;

    let mut stashed_worktrees: Vec<PathBuf> = Vec::new();
    let mut stashed_worktree_set: HashSet<PathBuf> = HashSet::new();
    if repo.is_dirty()? {
        if auto_stash_pop {
            let stashed = repo.stash_push()?;
            if stashed && !quiet {
                println!("{}", "✓ Stashed working tree changes.".green());
            }
            if stashed {
                stashed_worktree_set.insert(current_workdir.clone());
                stashed_worktrees.push(current_workdir.clone());
            }
        } else if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        } else {
            let stash = Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt("Working tree has uncommitted changes. Stash them before restack?")
                .default(true)
                .interact()?;

            if stash {
                let stashed = repo.stash_push()?;
                auto_stash_pop = true;
                println!("{}", "✓ Stashed working tree changes.".green());
                if stashed {
                    stashed_worktree_set.insert(current_workdir.clone());
                    stashed_worktrees.push(current_workdir.clone());
                }
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

    let branches_to_restack = branches_needing_restack(&stack, &scope_branches);

    if branches_to_restack.is_empty() {
        if !quiet {
            println!("{}", "✓ Stack is up to date, nothing to restack.".green());
        }
        restore_stashed_worktrees(repo, &stashed_worktrees, quiet)?;
        return Ok(());
    }

    // For --dry-run, do a pre-flight conflict check and then exit without rebasing.
    if dry_run {
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

        restore_stashed_worktrees(repo, &stashed_worktrees, quiet)?;
        return Ok(());
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

    // Load the stack once and keep it in memory.  After each successful rebase
    // we update the cached `needs_restack` flag for the rebased branch and its
    // direct children so subsequent iterations don't need another disk read.
    let mut live_stack = Stack::load(repo)?;

    for (index, branch) in scope_branches.iter().enumerate() {
        if completed_from_receipt.contains(branch) {
            continue;
        }

        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|br| br.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        // Retrieve parent name and stored revision from the in-memory cache.
        // Fall back to a metadata ref read only when the cache lacks the data
        // (shouldn't happen in practice, but keeps the code defensive).
        let (parent_branch_name, parent_branch_revision) = match live_stack.branches.get(branch) {
            Some(br) if br.parent.is_some() && br.parent_revision.is_some() => (
                br.parent.clone().unwrap(),
                br.parent_revision.clone().unwrap(),
            ),
            _ => match BranchMetadata::read(repo.inner(), branch)? {
                Some(m) => (m.parent_branch_name, m.parent_branch_revision),
                None => continue,
            },
        };

        let restack_timer =
            LiveTimer::maybe_new(!quiet, &format!("{} onto {}", branch, parent_branch_name));

        // Pre-stash dirty target worktrees so the rebase can proceed
        let target_workdir = repo.branch_rebase_target_workdir(branch)?;
        if auto_stash_pop
            && !stashed_worktree_set.contains(&target_workdir)
            && repo.is_dirty_at(&target_workdir)?
            && repo.stash_push_at(&target_workdir)?
        {
            stashed_worktree_set.insert(target_workdir.clone());
            stashed_worktrees.push(target_workdir.clone());
            if !quiet {
                print_stash_message(&current_workdir, &target_workdir);
            }
        }

        match repo.rebase_branch_onto_with_provenance(
            branch,
            &parent_branch_name,
            &parent_branch_revision,
            auto_stash_pop,
        )? {
            RebaseResult::Success => {
                // Update metadata with new parent revision
                let new_parent_rev = repo.branch_commit(&parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_name: parent_branch_name.clone(),
                    parent_branch_revision: new_parent_rev.clone(),
                    pr_info: live_stack.branches.get(branch).and_then(|br| {
                        br.pr_number.map(|n| crate::engine::PrInfo {
                            number: n,
                            state: br.pr_state.clone().unwrap_or_default(),
                            is_draft: br.pr_is_draft,
                        })
                    }),
                };
                updated_meta.write(repo.inner(), branch)?;

                // Update in-memory stack so subsequent branches see the new
                // parent revision without reloading from disk.
                if let Some(br) = live_stack.branches.get_mut(branch) {
                    br.needs_restack = false;
                    br.parent_revision = Some(new_parent_rev.clone());
                }
                // Direct children of this branch now have an updated parent tip
                // so their needs_restack status must be recalculated.
                let children: Vec<String> = live_stack
                    .branches
                    .get(branch)
                    .map(|br| br.children.clone())
                    .unwrap_or_default();
                for child in &children {
                    if let Some(child_br) = live_stack.branches.get_mut(child) {
                        // The stored parent_revision in the child still points to the
                        // old parent tip, so needs_restack becomes true.
                        child_br.needs_restack = child_br
                            .parent_revision
                            .as_deref()
                            .map(|rev| rev != new_parent_rev.as_str())
                            .unwrap_or(true);
                    }
                }

                // Record the after-OID for this branch
                tx.record_after(repo, branch)?;
                tx.push_completed_branch(branch);

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
                let conflict_stack = live_stack.current_stack(branch);
                print_restack_conflict(
                    repo,
                    &RestackConflictContext {
                        branch,
                        parent_branch: &parent_branch_name,
                        completed_branches: &completed_branches,
                        remaining_branches: scope_branches.len().saturating_sub(index + 1),
                        continue_commands: &[
                            "stax resolve",
                            "stax continue",
                            "stax restack --continue",
                        ],
                        stack_branches: &conflict_stack,
                    },
                );
                if !stashed_worktrees.is_empty() {
                    println!("{}", "Auto-stash kept to avoid conflicts.".yellow());
                }
                summary.push((branch.clone(), "conflict".to_string()));

                // Finish transaction with error
                tx.finish_err("Rebase conflict", Some("rebase"), Some(branch))?;

                return Err(ConflictStopped.into());
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

    restore_stashed_worktrees(repo, &stashed_worktrees, quiet)?;

    let should_submit = should_submit_after_restack(&summary, quiet, submit_after)?;

    if should_submit {
        submit_after_restack(quiet)?;
    }

    Ok(())
}

fn normalized_workdir(repo: &GitRepo) -> Result<PathBuf> {
    Ok(GitRepo::normalize_path(repo.workdir()?))
}

fn print_stash_message(current_workdir: &Path, target_workdir: &Path) {
    if target_workdir == current_workdir {
        println!("{}", "✓ Stashed working tree changes.".green());
    } else {
        println!(
            "{}",
            format!(
                "✓ Stashed working tree changes in {}.",
                target_workdir.display()
            )
            .green()
        );
    }
}

fn restore_stashed_worktrees(repo: &GitRepo, worktrees: &[PathBuf], quiet: bool) -> Result<()> {
    let current_workdir = normalized_workdir(repo)?;
    let mut errors: Vec<String> = Vec::new();
    for worktree in worktrees.iter().rev() {
        match repo.stash_pop_at(worktree) {
            Ok(()) => {
                if !quiet {
                    if *worktree == current_workdir {
                        println!("{}", "✓ Restored stashed changes.".green());
                    } else {
                        println!(
                            "{}",
                            format!("✓ Restored stashed changes in {}.", worktree.display())
                                .green()
                        );
                    }
                }
            }
            Err(e) => {
                errors.push(format!("{}: {}", worktree.display(), e));
            }
        }
    }
    if !errors.is_empty() {
        println!(
            "{}",
            "Warning: some stash pops failed. Run `git stash pop` manually in:".yellow()
        );
        for e in &errors {
            println!("  {}", e);
        }
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
        crate::commands::submit::SubmitOptions {
            yes: true,
            no_prompt: true,
            quiet,
            ..Default::default()
        },
    )?;

    Ok(())
}
