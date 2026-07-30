use crate::commands;
use crate::engine::Stack;
use crate::git::{GitRepo, local_branch_exists_in};
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

    commands::sync::run(
        true,  // restack
        false, // full
        false, // delete_merged
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        commands::sync::StashPolicy::Prompt,
        false, // json
        &submit_fetch_refs,
        yes || no_prompt,
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
