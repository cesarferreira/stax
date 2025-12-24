use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

pub fn run(parent: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;

    // Can't track trunk
    if current == trunk {
        println!(
            "{} is the trunk branch and cannot be tracked.",
            current.yellow()
        );
        return Ok(());
    }

    // Check if already tracked
    if let Some(existing) = BranchMetadata::read(repo.inner(), &current)? {
        println!(
            "Branch '{}' is already tracked with parent '{}'.",
            current.yellow(),
            existing.parent_branch_name.blue()
        );
        println!("Use {} to update.", "stax rs".cyan());
        return Ok(());
    }

    // Determine parent
    let parent_branch = match parent {
        Some(p) => {
            // Validate the branch exists
            if repo.branch_commit(&p).is_err() {
                anyhow::bail!("Branch '{}' does not exist", p);
            }
            p
        }
        None => {
            // Build list of potential parents
            let mut branches = repo.list_branches()?;
            branches.retain(|b| b != &current);
            branches.sort();

            // Put trunk first as the recommended default
            if let Some(pos) = branches.iter().position(|b| b == &trunk) {
                branches.remove(pos);
                branches.insert(0, trunk.clone());
            }

            if branches.is_empty() {
                anyhow::bail!("No branches available to be parent");
            }

            // Build display with recommendation hint
            let items: Vec<String> = branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    if i == 0 {
                        format!("{} (recommended)", b)
                    } else {
                        b.clone()
                    }
                })
                .collect();

            let selection = Select::with_theme(&ColorfulTheme::default())
                .with_prompt(format!("Select parent branch for '{}'", current))
                .items(&items)
                .default(0)
                .interact()?;

            branches[selection].clone()
        }
    };

    let parent_rev = repo.branch_commit(&parent_branch)?;

    // Create metadata
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &current)?;

    println!(
        "✓ Tracking '{}' with parent '{}'",
        current.green(),
        parent_branch.blue()
    );

    Ok(())
}
