use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
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

    // Ensure the branch is tracked
    let old_meta = BranchMetadata::read(repo.inner(), &current)?.ok_or_else(|| {
        anyhow::anyhow!(
            "Branch '{}' is not tracked by stax. Run `st branch track` first.",
            current
        )
    })?;

    let descendants = stack.descendants(&current);

    // Determine new parent
    let new_parent = match target {
        Some(t) => {
            if repo.branch_commit(&t).is_err() {
                bail!("Branch '{}' does not exist", t);
            }
            t
        }
        None => pick_parent_interactively(&repo, &current, &trunk, &descendants)?,
    };

    if new_parent == current {
        bail!("Cannot reparent a branch onto itself.");
    }

    // No-op: already parented onto the target
    if old_meta.parent_branch_name == new_parent {
        println!(
            "{}",
            format!(
                "'{}' is already parented onto '{}'. Nothing to do.",
                current, new_parent
            )
            .dimmed()
        );
        return Ok(());
    }

    // Prevent circular dependency
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

    // Collect the subtree for display
    let mut subtree = vec![current.clone()];
    subtree.extend(descendants.iter().cloned());

    // Save old parent info for rebase upstream calculation
    let old_parent_name = old_meta.parent_branch_name.clone();
    let old_parent_rev = old_meta.parent_branch_revision.clone();

    // Update only the root branch's parent pointer
    let parent_rev = repo.branch_commit(&new_parent)?;
    let merge_base = repo
        .merge_base(&new_parent, &current)
        .unwrap_or_else(|_| parent_rev.clone());

    let updated = BranchMetadata {
        parent_branch_name: new_parent.clone(),
        parent_branch_revision: merge_base.clone(),
        ..old_meta
    };

    if !restack {
        updated.write(repo.inner(), &current)?;
    }

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

        // Rebase the root branch directly using old parent info as upstream
        // (same approach as reparent.rs -- we need the old parent boundary
        // because the metadata was already overwritten)
        let rebase_upstream = resolve_rebase_upstream(
            &repo,
            &old_parent_name,
            &old_parent_rev,
            &current,
            &merge_base,
        )?;

        match repo.rebase_branch_onto_with_provenance(
            &current,
            &new_parent,
            &rebase_upstream,
            false,
        )? {
            RebaseResult::Success => {
                // Persist metadata only after the root rebase succeeds
                let new_parent_rev = repo.branch_commit(&new_parent)?;
                let mut persisted = updated.clone();
                persisted.parent_branch_revision = new_parent_rev;
                persisted.write(repo.inner(), &current)?;
                println!(
                    "  {} rebased '{}' onto '{}'",
                    "✓".green(),
                    current,
                    new_parent
                );

                // Now restack descendants via upstack restack
                if subtree.len() > 1 {
                    super::restack::run(false)?;
                }
            }
            RebaseResult::Conflict => {
                bail!(
                    "Rebase conflict while rebasing '{}' onto '{}'. \
                     Resolve conflicts, then run `st continue` or `st abort`.",
                    current,
                    new_parent
                );
            }
        }

        // Return to original branch
        if repo.current_branch()? != current {
            let _ = repo.checkout(&current);
        }
    } else {
        println!(
            "{}",
            "Run `st restack` to rebase the moved branches onto their new parent.".yellow()
        );
    }

    Ok(())
}

/// Determine the upstream commit for `git rebase --onto` when reparenting.
/// Uses the old parent's tip if it is an ancestor of the target branch,
/// otherwise falls back to the stored parent revision or merge-base.
fn resolve_rebase_upstream(
    repo: &GitRepo,
    old_parent_name: &str,
    old_parent_rev: &str,
    target: &str,
    merge_base: &str,
) -> Result<String> {
    // Try old parent's current tip
    if let Ok(tip) = repo.branch_commit(old_parent_name) {
        if repo.is_ancestor(&tip, target)? {
            return Ok(tip);
        }
    }

    // Try stored parent revision
    if !old_parent_rev.is_empty() && repo.is_ancestor(old_parent_rev, target)? {
        return Ok(old_parent_rev.to_string());
    }

    Ok(merge_base.to_string())
}

fn pick_parent_interactively(
    repo: &GitRepo,
    current: &str,
    trunk: &str,
    descendants: &[String],
) -> Result<String> {
    let mut branches = repo.list_branches()?;
    branches.retain(|b| b != current && !descendants.contains(b));
    branches.sort();

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
