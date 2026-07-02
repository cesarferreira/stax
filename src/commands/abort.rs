use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;

    if !repo.rebase_in_progress()? {
        println!("{}", "Nothing to abort.".yellow());
        return Ok(());
    }

    repo.rebase_abort()?;
    println!("{}", "Rebase aborted.".green());

    Ok(())
}
