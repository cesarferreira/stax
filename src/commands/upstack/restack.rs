use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get descendants that need restacking
    let descendants = stack.descendants(&current);
    let branches_to_restack: Vec<String> = descendants
        .into_iter()
        .filter(|b| {
            stack
                .branches
                .get(b)
                .map(|br| br.needs_restack)
                .unwrap_or(false)
        })
        .collect();

    if branches_to_restack.is_empty() {
        println!("{}", "✓ Upstack is up to date, nothing to restack.".green());
        return Ok(());
    }

    println!(
        "Restacking {} branch(es) above {}...",
        branches_to_restack.len().to_string().cyan(),
        current.blue()
    );

    for branch in &branches_to_restack {
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        println!(
            "  {} onto {}",
            branch.white(),
            meta.parent_branch_name.blue()
        );

        repo.checkout(branch)?;

        match repo.rebase(&meta.parent_branch_name)? {
            RebaseResult::Success => {
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), branch)?;
                println!("    {}", "✓ done".green());
            }
            RebaseResult::Conflict => {
                println!("    {}", "✗ conflict".red());
                println!();
                println!("{}", "Resolve conflicts and run:".yellow());
                println!("  {}", "stax continue".cyan());
                return Ok(());
            }
        }
    }

    // Return to original branch
    repo.checkout(&current)?;

    println!();
    println!("{}", "✓ Upstack restacked successfully!".green());

    Ok(())
}
