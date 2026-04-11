use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::theme::ColorfulTheme;
use dialoguer::FuzzySelect;

/// Reparent the current branch AND all its descendants onto a new parent.
/// The subtree structure is preserved -- only the root's parent changes.
pub fn run(target: Option<String>, restack: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;

    if current == trunk {
        bail!("Cannot reparent trunk. Checkout a stacked branch first.");
    }

    // Determine new parent
    let new_parent = match target {
        Some(t) => {
            if repo.branch_commit(&t).is_err() {
                bail!("Branch '{}' does not exist", t);
            }
            t
        }
        None => pick_parent_interactively(&repo, &current, &trunk)?,
    };

    if new_parent == current {
        bail!("Cannot reparent a branch onto itself.");
    }

    // Prevent circular dependency: new parent cannot be a descendant of current
    let descendants = stack.descendants(&current);
    if descendants.contains(&new_parent) {
        bail!(
            "Cannot reparent '{}' onto '{}': would create circular dependency.\n\
             '{}' is a descendant of '{}'.",
            current,
            new_parent,
            new_parent,
            current
        );
    }

    // Collect the subtree: current + all descendants
    let mut subtree = vec![current.clone()];
    subtree.extend(descendants);

    // Read old parent info for the root branch
    let old_meta = BranchMetadata::read(repo.inner(), &current)?;

    // Update only the root branch's parent pointer
    let parent_rev = repo.branch_commit(&new_parent)?;
    let merge_base = repo
        .merge_base(&new_parent, &current)
        .unwrap_or_else(|_| parent_rev.clone());

    let updated = if let Some(meta) = old_meta {
        BranchMetadata {
            parent_branch_name: new_parent.clone(),
            parent_branch_revision: merge_base,
            ..meta
        }
    } else {
        BranchMetadata::new(&new_parent, &merge_base)
    };
    updated.write(repo.inner(), &current)?;

    println!(
        "✓ Reparented '{}' onto '{}'",
        current.green(),
        new_parent.blue()
    );
    if subtree.len() > 1 {
        println!(
            "  {} descendant branch(es) moved with it:",
            (subtree.len() - 1).to_string().cyan()
        );
        for desc in &subtree[1..] {
            println!("    {}", desc.dimmed());
        }
    }

    if restack {
        println!();
        println!("{}", "Restacking moved branches...".bold());
        // Restack the full subtree using the existing upstack restack logic
        super::restack::run(false)?;
    } else {
        println!(
            "{}",
            "Run `stax restack` to rebase the moved branches onto their new parent."
                .yellow()
        );
    }

    Ok(())
}

fn pick_parent_interactively(repo: &GitRepo, current: &str, trunk: &str) -> Result<String> {
    let mut branches = repo.list_branches()?;
    // Remove current and its descendants (can't reparent onto them)
    let stack = Stack::load(repo)?;
    let descendants = stack.descendants(current);
    branches.retain(|b| b != current && !descendants.contains(b));
    branches.sort();

    // Put trunk first
    if let Some(pos) = branches.iter().position(|b| b == trunk) {
        branches.remove(pos);
        branches.insert(0, trunk.to_string());
    }

    if branches.is_empty() {
        bail!("No branches available as a new parent");
    }

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Select new parent for '{}' (and all its descendants)",
            current
        ))
        .items(&branches)
        .default(0)
        .interact()?;

    Ok(branches[selection].clone())
}
