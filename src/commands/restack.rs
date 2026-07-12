use crate::application::{
    NoopOperationReporter, OperationError, OperationErrorDetails, OperationErrorKind,
    OperationOutcome, OperationReceipt, OperationWarning, RepositorySession,
    RestackExecutionOptions, RestackScope,
};
use crate::commands::restack_conflict::{RestackConflictContext, print_restack_conflict};
use crate::engine::{BranchMetadata, Stack};
use crate::errors::ConflictStopped;
use crate::git::GitRepo;
use crate::progress::LiveTimer;
use anyhow::{Result, anyhow};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use std::collections::HashSet;
use std::io::IsTerminal;

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

    let _ = yes; // `yes` is preserved in the CLI API but no longer used during restack itself.
    run_adapter(
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
    run_adapter(
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
fn run_adapter(
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
    if dry_run {
        return run_dry_run(repo, all, stop_here, quiet, &current);
    }
    if !all && Stack::load(repo)?.trunk == current {
        return Ok(());
    }

    if repo.is_dirty()? && !auto_stash_pop {
        if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        }
        let stash = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Working tree has uncommitted changes. Stash them before restack?")
            .default(true)
            .interact()?;
        if !stash {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
        auto_stash_pop = true;
    }

    let scope = restack_scope(all, stop_here, &current);
    let session = RepositorySession::open(repo.workdir()?)?;
    let options = RestackExecutionOptions {
        scope,
        auto_stash: auto_stash_pop,
        restore_branch,
        completed_from_receipt,
    };
    let receipt = match session.restack_with_options(options, &mut NoopOperationReporter) {
        Ok(receipt) => receipt,
        Err(error) if error.kind == OperationErrorKind::RebaseConflict => {
            render_restack_error(repo, &error, false);
            return Err(ConflictStopped.into());
        }
        Err(error) => return Err(operation_error(error)),
    };

    render_restack_receipt(&receipt, quiet);
    let restacked = restacked_branches(&receipt);
    let should_submit = should_submit_after_restack(&restacked, quiet, submit_after)?;

    if should_submit {
        submit_after_restack(quiet)?;
    }

    Ok(())
}

fn restack_scope(all: bool, stop_here: bool, current: &str) -> RestackScope {
    if all {
        RestackScope::All
    } else if stop_here {
        RestackScope::ThroughBranch(current.to_string())
    } else {
        RestackScope::StackContaining(current.to_string())
    }
}

fn run_dry_run(
    repo: &GitRepo,
    all: bool,
    stop_here: bool,
    quiet: bool,
    current: &str,
) -> Result<()> {
    let stack = Stack::load(repo)?;
    let scope_branches = dry_run_scope_branches(&stack, all, stop_here, current);
    let branches_to_restack = branches_needing_restack(&stack, &scope_branches);
    let timer = LiveTimer::maybe_new(!quiet, "Checking for conflicts...");
    let branch_parent_pairs: Vec<(String, String)> = branches_to_restack
        .iter()
        .filter_map(|branch| {
            BranchMetadata::read(repo.inner(), branch)
                .ok()
                .flatten()
                .map(|metadata| (branch.clone(), metadata.parent_branch_name))
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
        for prediction in &predictions {
            println!(
                "  {} {} → {}",
                "✗".red(),
                prediction.branch.yellow().bold(),
                prediction.onto.dimmed()
            );
            for file in &prediction.conflicting_files {
                println!("    {} {}", "│".dimmed(), file.red());
            }
        }
        println!();
    }
    Ok(())
}

fn dry_run_scope_branches(stack: &Stack, all: bool, stop_here: bool, current: &str) -> Vec<String> {
    let mut branches = if all {
        stack
            .branches
            .keys()
            .filter(|branch| *branch != &stack.trunk)
            .cloned()
            .collect::<Vec<_>>()
    } else if stop_here {
        let mut branches = stack.ancestors(current);
        branches.reverse();
        branches.retain(|branch| branch != &stack.trunk);
        if current != stack.trunk {
            branches.push(current.to_string());
        }
        branches
    } else {
        stack
            .current_stack(current)
            .into_iter()
            .filter(|branch| branch != &stack.trunk)
            .collect()
    };
    if all {
        branches.sort_by(|a, b| {
            stack
                .ancestors(a)
                .len()
                .cmp(&stack.ancestors(b).len())
                .then_with(|| a.cmp(b))
        });
    }
    branches
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

fn should_submit_after_restack(
    restacked: &[String],
    quiet: bool,
    submit_after: SubmitAfterRestack,
) -> Result<bool> {
    if restacked.is_empty() {
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

fn render_restack_receipt(receipt: &OperationReceipt, quiet: bool) {
    if quiet {
        return;
    }
    render_warnings(&receipt.warnings);
    let (branches, skipped_frozen) = match &receipt.outcome {
        OperationOutcome::Restacked {
            branches,
            skipped_frozen,
        } => (branches, skipped_frozen),
        _ => return,
    };
    if !skipped_frozen.is_empty() {
        println!(
            "  {} Skipping frozen {}: {}",
            "▸".dimmed(),
            if skipped_frozen.len() == 1 {
                "branch"
            } else {
                "branches"
            },
            skipped_frozen.join(", ").cyan()
        );
    }
    if branches.is_empty() {
        println!("{}", "✓ Stack is up to date, nothing to restack.".green());
        return;
    }
    println!();
    println!("{}", "✓ Stack restacked successfully!".green());
    println!();
    println!("{}", "Restack summary:".dimmed());
    for branch in branches {
        println!("  ✓ {} ok", branch);
    }
}

fn render_warnings(warnings: &[OperationWarning]) {
    for warning in warnings {
        match warning {
            OperationWarning::RestackBoundaryAdjusted { reason, .. } => {
                println!("  {} {}", "preflight:".yellow().bold(), reason);
            }
            OperationWarning::StashRestoreFailed {
                worktree,
                diagnostic,
            } => {
                println!(
                    "{}",
                    "Warning: some stash pops failed. Run `git stash pop` manually in:".yellow()
                );
                println!("  {}: {}", worktree.display(), diagnostic);
            }
            OperationWarning::BranchNameNormalized { .. }
            | OperationWarning::SubmitReviewersUnsupported { .. }
            | OperationWarning::SubmitNativeStackAdvisory { .. } => {}
        }
    }
}

fn restacked_branches(receipt: &OperationReceipt) -> Vec<String> {
    match &receipt.outcome {
        OperationOutcome::Restacked { branches, .. } => branches.clone(),
        _ => Vec::new(),
    }
}

fn render_restack_error(repo: &GitRepo, error: &OperationError, quiet: bool) {
    if quiet {
        return;
    }
    if let OperationErrorDetails::Rebase {
        branch: Some(branch),
        ..
    } = &error.details
    {
        if let Ok(Some(meta)) = BranchMetadata::read(repo.inner(), branch) {
            if let Ok(stack) = Stack::load(repo) {
                let completed = error
                    .receipt
                    .as_ref()
                    .and_then(|receipt| match &receipt.outcome {
                        OperationOutcome::Restacked { branches, .. } => Some(branches.as_slice()),
                        _ => None,
                    })
                    .unwrap_or(&[]);
                let stack_branches = stack.current_stack(branch);
                let remaining = stack_branches
                    .iter()
                    .filter(|candidate| *candidate != &stack.trunk)
                    .filter(|candidate| *candidate != branch)
                    .filter(|candidate| !completed.contains(candidate))
                    .count();
                let context = RestackConflictContext {
                    branch,
                    parent_branch: &meta.parent_branch_name,
                    completed_branches: completed,
                    remaining_branches: remaining,
                    continue_commands: &["stax restack --continue", "git rebase --abort"],
                    stack_branches: &stack_branches,
                };
                print_restack_conflict(repo, &context);
                return;
            }
        }
    }
    println!("{}", error.primary.red());
    println!("{}", error.action);
}

fn operation_error(error: OperationError) -> anyhow::Error {
    anyhow!(
        "{}\n{}\n{}",
        error.primary,
        error.action,
        error.diagnostic_chain
    )
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
