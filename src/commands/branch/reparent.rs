use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::remote;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

/// Update the parent of a tracked branch
pub fn run(branch: Option<String>, parent: Option<String>, restack: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;
    let target = branch.unwrap_or_else(|| current.clone());

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

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
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

    // Check for circular dependency: new parent cannot be a descendant of target
    let descendants = stack.descendants(&target);
    if descendants.contains(&parent_branch) {
        anyhow::bail!(
            "Cannot reparent '{}' onto '{}': would create circular dependency.\n\
             '{}' is a descendant of '{}'.",
            target,
            parent_branch,
            parent_branch,
            target
        );
    }

    let existing = BranchMetadata::read(repo.inner(), &target)?;
    if restack && existing.is_none() {
        anyhow::bail!(
            "`--restack` requires existing stax metadata so the previous parent can be used as the rebase boundary.\n\
             Use `{}` first, or run `{}` without `--restack` and then `{}`.",
            "stax branch track --parent <branch>".cyan(),
            "stax branch reparent".cyan(),
            "stax restack".cyan(),
        );
    }

    let parent_rev = repo.branch_commit(&parent_branch)?;
    let merge_base = repo
        .merge_base(&parent_branch, &target)
        .unwrap_or_else(|_| parent_rev.clone());
    let rebase_upstream = if restack {
        resolve_reparent_rebase_upstream(&repo, &existing, &parent_branch, &target, &merge_base)?
    } else {
        String::new()
    };

    let updated = if let Some(meta) = existing.clone() {
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
    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name())
    {
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

    if restack {
        match repo.rebase_branch_onto_with_provenance(
            &target,
            &parent_branch,
            &rebase_upstream,
            false,
        )? {
            RebaseResult::Success => {
                let new_parent_rev = repo.branch_commit(&parent_branch)?;
                if let Some(mut meta) = BranchMetadata::read(repo.inner(), &target)? {
                    meta.parent_branch_revision = new_parent_rev;
                    meta.write(repo.inner(), &target)?;
                }
                println!(
                    "{}",
                    format!("✓ Rebased '{}' onto '{}'", target, parent_branch).green()
                );
            }
            RebaseResult::Conflict => {
                anyhow::bail!(
                    "Rebase conflict while rebasing '{}' onto '{}'. Resolve conflicts, then run `{}` or `{}`.",
                    target,
                    parent_branch,
                    "stax continue",
                    "stax undo",
                );
            }
        }

        if repo.branch_commit(&current).is_ok() {
            let _ = repo.checkout(&current);
        }
    } else {
        println!(
            "{}",
            "Note: Reparent updated stax metadata only. Git still has the old commit ancestry — PRs may show the previous stack until you rebase. Run the same command with `--restack`, or run `stax restack` when this branch is flagged as needing restack.".yellow()
        );
    }

    Ok(())
}

/// Commit-ish to pass as `git rebase --onto <new-parent> <upstream>` so only commits
/// after the old direct parent are replayed.
fn resolve_reparent_rebase_upstream(
    repo: &GitRepo,
    existing: &Option<BranchMetadata>,
    new_parent: &str,
    target: &str,
    merge_base: &str,
) -> Result<String> {
    let Some(meta) = existing else {
        return Ok(merge_base.to_string());
    };

    let old_parent = meta.parent_branch_name.trim();
    if old_parent.is_empty() || old_parent == new_parent {
        return Ok(merge_base.to_string());
    }

    if let Ok(tip) = repo.branch_commit(old_parent) {
        if repo.is_ancestor(&tip, target)? {
            return Ok(tip);
        }
    }

    let stored = meta.parent_branch_revision.trim();
    if !stored.is_empty() && repo.is_ancestor(stored, target)? {
        return Ok(stored.to_string());
    }

    Ok(merge_base.to_string())
}
