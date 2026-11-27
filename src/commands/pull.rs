//! `st pull` = `st sync --get --all-stacks --restack`, then restack every stack
//! (the `refresh --all-stacks` loop, without submit).

use crate::commands::all_stacks::{
    self, LoopOutcome, collect_stack_plans, restore_original_branch, root_of,
};
use crate::commands::sync::{self, GetScope, StashPolicy};
use crate::commands::sync_plan::{self, SyncPlanOptions};
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

const RERUN_COMMAND: &str = "st pull";

pub struct PullOptions {
    pub dry_run: bool,
    pub force: bool,
    pub no_restack: bool,
    pub no_delete: bool,
    pub safe: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub auto_stash_pop: bool,
}

pub fn run(o: PullOptions) -> Result<()> {
    if o.dry_run {
        return run_dry_run(&o);
    }
    if o.no_restack {
        return run_sync(&o, false);
    }

    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;

    all_stacks::ensure_clean_tree(&repo, o.auto_stash_pop, RERUN_COMMAND)?;

    let plans = collect_stack_plans(&stack);
    let sync_root = root_of(&stack, &original);
    if let Err(error) = run_sync(&o, true) {
        if let Some(root) = sync_root.as_deref() {
            all_stacks::print_progress(&plans, &[], root, RERUN_COMMAND);
        }
        return Err(error);
    }
    if repo.rebase_in_progress()? {
        if let Some(root) = sync_root.as_deref() {
            all_stacks::print_progress(&plans, &[], root, RERUN_COMMAND);
        }
        return Ok(());
    }

    let live_plans = collect_stack_plans(&Stack::load(&repo)?);
    if let LoopOutcome::PausedForRebase = all_stacks::restack_each_stack(
        &repo,
        &live_plans,
        o.quiet,
        o.auto_stash_pop,
        RERUN_COMMAND,
        |_| Ok(()),
    )? {
        return Ok(());
    }

    restore_original_branch(&repo, &workdir, &original)
}

fn run_sync(o: &PullOptions, restack: bool) -> Result<()> {
    let stash_policy = StashPolicy::from_auto_stash_pop(o.auto_stash_pop);
    let result = sync::run(
        restack,
        false, // full
        false, // tags
        !o.no_delete,
        false, // delete_upstream_gone
        o.force,
        o.safe,
        false, // continue
        o.quiet,
        o.verbose,
        o.auto_stash_pop,
        stash_policy,
        false, // json
        &[],
        restack, // skip_interactive_plan
        GetScope::AllStacks,
    );
    sync::with_dirty_tree_hint(result, Some("--auto-stash-pop"))
}

fn run_dry_run(o: &PullOptions) -> Result<()> {
    sync_plan::run(SyncPlanOptions {
        restack: !o.no_restack,
        full: false,
        tags: false,
        delete_merged: !o.no_delete,
        delete_upstream_gone: false,
        force: o.force,
        safe: o.safe,
        quiet: o.quiet,
        verbose: o.verbose,
        auto_stash_pop: o.auto_stash_pop,
        stash_policy: StashPolicy::Prompt,
        json: false,
        get: GetScope::AllStacks,
    })?;

    if o.no_restack || o.quiet {
        return Ok(());
    }

    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let plans = collect_stack_plans(&stack);
    println!();
    println!("{}", "Restack (every stack)".bold());
    if plans.is_empty() {
        println!("  No tracked stacks to restack.");
    } else {
        all_stacks::print_stack_plans(&stack, &plans);
    }
    Ok(())
}
