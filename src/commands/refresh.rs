use crate::commands;
use crate::git::{local_branch_exists_in, GitRepo};
use anyhow::Result;
use colored::Colorize;

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

    println!("{}", "Refreshing stack...".bold());
    println!("  1. Sync trunk and clean merged branches");
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
        true,  // delete_merged
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
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
