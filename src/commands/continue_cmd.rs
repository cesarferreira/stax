use crate::engine::BranchMetadata;
use crate::git::{GitRepo, RebaseResult};
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;

    if !repo.rebase_in_progress()? {
        println!("{}", "No rebase in progress.".yellow());
        return Ok(());
    }

    println!("Continuing rebase...");

    match repo.rebase_continue()? {
        RebaseResult::Success => {
            // Update metadata for current branch
            let current = repo.current_branch()?;
            if let Some(meta) = BranchMetadata::read(repo.inner(), &current)? {
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), &current)?;
            }

            println!("{}", "✓ Rebase completed successfully!".green());
            println!();
            println!("You may want to run {} to continue restacking.", "stax rs".cyan());
        }
        RebaseResult::Conflict => {
            println!("{}", "More conflicts to resolve.".yellow());
            println!();
            println!("Resolve the conflicts and run {} again.", "stax continue".cyan());
        }
    }

    Ok(())
}
