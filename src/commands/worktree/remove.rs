use super::shared::{
    find_current_worktree, find_worktree, run_blocking_hook, spawn_background_hook,
};
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::repo::WorktreeInfo;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

fn effective_remove_force(force: bool, confirmed_dirty_removal: bool) -> bool {
    force || confirmed_dirty_removal
}

pub(crate) fn remove_worktree_with_hooks(
    repo: &GitRepo,
    config: &Config,
    worktree: &WorktreeInfo,
    force: bool,
) -> Result<String> {
    if worktree.is_main {
        bail!("Cannot remove the main worktree.");
    }

    if !worktree.path.exists() {
        bail!(
            "Worktree path '{}' no longer exists. Run `stax worktree prune`.",
            worktree.path.display()
        );
    }

    let main_workdir = repo.main_repo_workdir()?;
    run_blocking_hook(
        config.worktree.hooks.pre_remove.as_deref(),
        &main_workdir,
        "pre_remove",
    )?;

    repo.worktree_remove(&worktree.path, force)?;

    spawn_background_hook(
        config.worktree.hooks.post_remove.as_deref(),
        &main_workdir,
        "post_remove",
    )?;

    Ok(worktree
        .branch
        .clone()
        .unwrap_or_else(|| worktree.name.clone()))
}

pub fn run(name: Option<String>, force: bool, delete_branch: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let worktree = match name {
        Some(name) => find_worktree(&repo, &name)?
            .ok_or_else(|| anyhow::anyhow!("No worktree named '{}'", name))?,
        None => find_current_worktree(&repo)?,
    };

    if worktree.is_main {
        bail!("Cannot remove the main worktree.");
    }

    if !worktree.path.exists() {
        bail!(
            "Worktree path '{}' no longer exists. Run `stax worktree prune`.",
            worktree.path.display()
        );
    }

    let mut confirmed_dirty_removal = false;
    if !force && repo.is_dirty_at(&worktree.path)? {
        eprintln!(
            "{} Worktree '{}' has uncommitted changes.",
            "Warning:".yellow().bold(),
            worktree.name
        );
        let proceed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Remove anyway?")
            .default(false)
            .interact()?;
        if !proceed {
            println!("{}", "Aborted.".dimmed());
            return Ok(());
        }

        confirmed_dirty_removal = true;
    }

    let branch = worktree.branch.clone();
    let main_workdir = repo.main_repo_workdir()?;
    let remove_force = effective_remove_force(force, confirmed_dirty_removal);
    let display_name = remove_worktree_with_hooks(&repo, &config, &worktree, remove_force)?;

    if delete_branch {
        let repo = GitRepo::open_from_path(&main_workdir)?;
        if let Some(branch) = branch.as_deref() {
            match repo.delete_branch(branch, force) {
                Ok(()) => {
                    let _ = BranchMetadata::delete(repo.inner(), branch);
                    println!("  Deleted branch '{}'", branch.blue());
                }
                Err(error) => {
                    eprintln!(
                        "{}",
                        format!("  Warning: could not delete branch '{}': {}", branch, error)
                            .yellow()
                    );
                }
            }
        } else {
            eprintln!(
                "{}",
                "  Warning: detached worktrees do not have a branch to delete.".yellow()
            );
        }
    }

    println!(
        "{}  worktree '{}'",
        "Removed".green().bold(),
        display_name.cyan()
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::effective_remove_force;

    #[test]
    fn confirmed_dirty_removal_upgrades_to_force() {
        assert!(effective_remove_force(false, true));
    }

    #[test]
    fn force_flag_stays_enabled_without_extra_confirmation() {
        assert!(effective_remove_force(true, false));
    }

    #[test]
    fn clean_non_forced_removal_stays_non_forced() {
        assert!(!effective_remove_force(false, false));
    }
}
