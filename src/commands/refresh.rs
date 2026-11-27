use crate::commands;
use crate::commands::all_stacks::{
    self, LoopOutcome, StackPlan, collect_stack_plans, restore_original_branch, root_of,
};
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use std::io::IsTerminal;

#[allow(clippy::too_many_arguments)]
pub fn run(
    no_pr: bool,
    no_submit: bool,
    force: bool,
    safe: bool,
    verbose: bool,
    yes: bool,
    no_prompt: bool,
    auto_stash_pop: bool,
    all_stacks: bool,
    delete_merged: bool,
    get: bool,
) -> Result<()> {
    if all_stacks {
        return run_all_stacks(
            no_pr,
            no_submit,
            force,
            safe,
            verbose,
            yes,
            no_prompt,
            auto_stash_pop,
            delete_merged,
            get,
        );
    }

    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;
    let submit_fetch_refs = if no_submit {
        Vec::new()
    } else {
        stack
            .current_stack(&original)
            .into_iter()
            .filter(|branch| branch != &stack.trunk)
            .collect::<Vec<_>>()
    };

    let step3 = if no_submit {
        "Skip push and PR updates (--no-submit)"
    } else if no_pr {
        "Push branches without updating PRs"
    } else {
        "Push branches and update PRs"
    };
    println!("{} {}", "▸".cyan().bold(), "Updating stack".bold());
    println!("  {} Sync trunk", "├─".dimmed());
    println!(
        "  {} Restack current stack onto updated parents",
        "├─".dimmed()
    );
    println!("  {} {}", "└─".dimmed(), step3);

    let sync_result = commands::sync::run(
        true,  // restack
        false, // full
        false, // tags
        delete_merged,
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        commands::sync::StashPolicy::from_auto_stash_pop(auto_stash_pop),
        false, // json
        &submit_fetch_refs,
        true, // refresh/update is an explicit workflow — no Sync plan prompt
        commands::sync::GetScope::from_flags(get, false),
    );
    commands::sync::with_dirty_tree_hint(sync_result, Some("--auto-stash-pop"))?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if no_submit {
        return restore_original_branch(&repo, &workdir, &original);
    }

    commands::submit::run(
        commands::submit::SubmitScope::Stack,
        commands::submit::SubmitOptions {
            no_pr,
            prefetched: true,
            yes,
            no_prompt,
            verbose,
            ..Default::default()
        },
    )?;

    restore_original_branch(&repo, &workdir, &original)
}

fn print_all_stacks_header(no_pr: bool, no_submit: bool) {
    let step3 = if no_submit {
        "Skip push and PR updates (--no-submit)"
    } else if no_pr {
        "Push branches without updating PRs"
    } else {
        "Push branches and update PRs"
    };
    println!("{} {}", "▸".cyan().bold(), "Updating all stacks".bold());
    println!("  {} Sync trunk (once)", "├─".dimmed());
    println!(
        "  {} Restack every stack onto updated parents",
        "├─".dimmed()
    );
    println!("  {} {}", "└─".dimmed(), step3);
}

fn confirm_all_stacks(plans: &[StackPlan], yes: bool, force: bool) -> Result<bool> {
    if yes || force || !std::io::stdin().is_terminal() {
        return Ok(true);
    }
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Refresh {} stacks?", plans.len()))
        .default(true)
        .interact()
        .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
fn run_all_stacks(
    no_pr: bool,
    no_submit: bool,
    force: bool,
    safe: bool,
    verbose: bool,
    yes: bool,
    no_prompt: bool,
    auto_stash_pop: bool,
    delete_merged: bool,
    get: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;

    let plans = collect_stack_plans(&stack);
    if plans.is_empty() {
        println!("{}", "No tracked stacks to refresh.".yellow());
        return Ok(());
    }

    all_stacks::ensure_clean_tree(&repo, auto_stash_pop, "st refresh --all-stacks")?;

    print_all_stacks_header(no_pr, no_submit);
    all_stacks::print_stack_plans(&stack, &plans);

    if !confirm_all_stacks(&plans, yes, force)? {
        println!("Aborted.");
        return Ok(());
    }

    let submit_fetch_refs: Vec<String> = if no_submit {
        Vec::new()
    } else {
        plans
            .iter()
            .flat_map(|p| p.branches.iter().cloned())
            .collect()
    };

    let sync_root = root_of(&stack, &original);
    let sync_result = commands::sync::run(
        true,  // restack
        false, // full
        false, // tags
        delete_merged,
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        commands::sync::StashPolicy::from_auto_stash_pop(auto_stash_pop),
        false, // json
        &submit_fetch_refs,
        true, // refresh/update is an explicit workflow — no Sync plan prompt
        commands::sync::GetScope::from_flags(get, true),
    );
    if let Err(error) = commands::sync::with_dirty_tree_hint(sync_result, Some("--auto-stash-pop"))
    {
        if let Some(root) = sync_root.as_deref() {
            all_stacks::print_progress(&plans, &[], root, "st refresh --all-stacks");
        }
        return Err(error);
    }
    if repo.rebase_in_progress()? {
        if let Some(root) = sync_root.as_deref() {
            all_stacks::print_progress(&plans, &[], root, "st refresh --all-stacks");
        }
        return Ok(());
    }

    let live_stack = Stack::load(&repo)?;
    let live_plans = collect_stack_plans(&live_stack);

    match all_stacks::restack_each_stack(
        &repo,
        &live_plans,
        false,
        auto_stash_pop,
        "st refresh --all-stacks",
        |plan| {
            if no_submit {
                return Ok(());
            }
            repo.checkout(&plan.root).with_context(|| {
                format!(
                    "could not check out '{}' to submit its stack \
                     (is it checked out in another worktree?)",
                    plan.root
                )
            })?;
            commands::submit::run(
                commands::submit::SubmitScope::Stack,
                commands::submit::SubmitOptions {
                    no_pr,
                    prefetched: true,
                    yes,
                    no_prompt,
                    verbose,
                    ..Default::default()
                },
            )
            .with_context(|| format!("submit failed for stack '{}'", plan.root))
        },
    )? {
        LoopOutcome::PausedForRebase => return Ok(()),
        LoopOutcome::Finished => {}
    }

    restore_original_branch(&repo, &workdir, &original)
}
