use crate::commands;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(no_submit: bool, no_pr: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;

    println!("{}", "Cascading stack...".bold());

    commands::navigate::bottom()?;
    commands::restack::run(false, false, true)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    commands::upstack::restack::run()?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if !no_submit {
        commands::submit::run(
            commands::submit::SubmitScope::Stack,
            false,  // draft
            no_pr,  // no_pr
            false,  // _force
            true,   // yes
            true,   // no_prompt
            vec![], // reviewers
            vec![], // labels
            vec![], // assignees
            false,  // quiet
            false,  // verbose
            None,   // template
            false,  // no_template
            false,  // edit
            false,  // ai_body
        )?;
    }

    if !repo.rebase_in_progress()? && repo.current_branch()? != original {
        repo.checkout(&original)?;
    }

    Ok(())
}
