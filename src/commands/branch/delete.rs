use crate::application::{NoopOperationReporter, RepositorySession};
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{Confirm, FuzzySelect, theme::ColorfulTheme};

pub fn run(branch: Option<String>, force: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;

    let target = match branch {
        Some(b) => b,
        None => {
            // Interactive selection
            let mut branches = repo.list_branches()?;
            branches.retain(|b| b != &trunk && b != &current);
            branches.sort();

            if branches.is_empty() {
                println!("No branches to delete.");
                return Ok(());
            }

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Select branch to delete")
                .items(&branches)
                .interact()?;

            branches[selection].clone()
        }
    };

    if target == trunk {
        anyhow::bail!("Cannot delete trunk branch '{}'", trunk);
    }

    if target == current {
        anyhow::bail!("Cannot delete current branch. Checkout a different branch first.");
    }

    // Confirm if not forced
    if !force {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Delete branch '{}'?", target))
            .default(false)
            .interact()?;

        if !confirm {
            println!("Cancelled.");
            return Ok(());
        }
    }

    RepositorySession::open(repo.workdir()?)?
        .delete_branch(&target, force, &mut NoopOperationReporter)
        .map_err(|error| anyhow::anyhow!("{}\n{}", error.primary, error.action))?;

    println!("Deleted branch '{}'", target.red());

    Ok(())
}
