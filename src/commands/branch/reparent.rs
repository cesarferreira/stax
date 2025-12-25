use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::remote;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

/// Update the parent of a tracked branch
pub fn run(branch: Option<String>, parent: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;
    let target = branch.unwrap_or(current);

    if target == trunk {
        println!(
            "{} is the trunk branch and cannot be reparented.",
            target.yellow()
        );
        return Ok(());
    }

    // Determine parent
    let parent_branch = match parent {
        Some(p) => {
            if repo.branch_commit(&p).is_err() {
                anyhow::bail!("Branch '{}' does not exist", p);
            }
            p
        }
        None => {
            let mut branches = repo.list_branches()?;
            branches.retain(|b| b != &target);
            branches.sort();

            if let Some(pos) = branches.iter().position(|b| b == &trunk) {
                branches.remove(pos);
                branches.insert(0, trunk.clone());
            }

            if branches.is_empty() {
                anyhow::bail!("No branches available to be parent");
            }

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
                .with_prompt(format!("Select new parent branch for '{}'", target))
                .items(&items)
                .default(0)
                .interact()?;

            branches[selection].clone()
        }
    };

    if parent_branch == target {
        anyhow::bail!("Parent branch cannot be the same as '{}'", target);
    }

    let parent_rev = repo.branch_commit(&parent_branch)?;
    let merge_base = repo.merge_base(&parent_branch, &target).unwrap_or(parent_rev.clone());

    let existing = BranchMetadata::read(repo.inner(), &target)?;
    let updated = if let Some(meta) = existing {
        BranchMetadata {
            parent_branch_name: parent_branch.clone(),
            parent_branch_revision: merge_base.clone(),
            ..meta
        }
    } else {
        BranchMetadata::new(&parent_branch, &merge_base)
    };

    updated.write(repo.inner(), &target)?;

    let config = crate::config::Config::load()?;
    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name()) {
        if !remote_branches.contains(&parent_branch) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }

    println!(
        "✓ Reparented '{}' onto '{}'",
        target.green(),
        parent_branch.blue()
    );

    if parent_rev != merge_base {
        println!("{}", "Note: restack is recommended for this branch.".yellow());
    }

    Ok(())
}
