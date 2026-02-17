use crate::commands;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(push_only: bool, no_push: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;

    println!("{}", "Cascading stack...".bold());

    commands::navigate::bottom()?;
    commands::restack::run(false, false, true, false)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    commands::upstack::restack::run(false)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if no_push {
        println!("{}", "Skipping push and PRs (--no-push flag)".dimmed());
    } else {
        commands::submit::run(
            commands::submit::SubmitScope::Stack,
            false,     // draft
            push_only, // no_pr (push but skip PR creation)
            false,     // force
            true,      // yes
            true,      // no_prompt
            vec![],    // reviewers
            vec![],    // labels
            vec![],    // assignees
            false,     // quiet
            false,     // verbose
            None,      // template
            false,     // no_template
            false,     // edit
            false,     // ai_body
        )?;
    }

    if !repo.rebase_in_progress()? && repo.current_branch()? != original {
        repo.checkout(&original)?;
    }

    Ok(())
}
