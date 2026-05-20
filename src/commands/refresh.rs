use crate::commands;
use crate::engine::Stack;
use crate::git::{local_branch_exists_in, GitRepo};
use anyhow::Result;
use colored::Colorize;

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
) -> Result<()> {
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

    println!("{}", "Updating stack...".bold());
    println!("  1. Sync trunk");
    println!("  2. Restack current stack onto updated parents");
    if no_submit {
        println!("  3. Skip push and PR updates (--no-submit)");
    } else if no_pr {
        println!("  3. Push branches without updating PRs");
    } else {
        println!("  3. Push branches and update PRs");
    }

    commands::sync::run(
        true,  // restack
        false, // prune
        false, // full
        false, // delete_merged
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        &submit_fetch_refs,
    )?;

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

fn restore_original_branch(
    repo: &GitRepo,
    workdir: &std::path::Path,
    original: &str,
) -> Result<()> {
    if !repo.rebase_in_progress()?
        && repo.current_branch()? != original
        && local_branch_exists_in(workdir, original)
    {
        repo.checkout(original)?;
    }

    Ok(())
}
