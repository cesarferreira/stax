use super::shared::{
    find_current_worktree, find_worktree, run_blocking_hook, spawn_background_hook,
};
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

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
    }

    let main_workdir = repo.main_repo_workdir()?;
    run_blocking_hook(
        config.worktree.hooks.pre_remove.as_deref(),
        &main_workdir,
        "pre_remove",
    )?;

    let branch = worktree.branch.clone();
    let path = worktree.path.clone();
    let display_name = branch.clone().unwrap_or_else(|| worktree.name.clone());
    repo.worktree_remove(&path, force)?;

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

    spawn_background_hook(
        config.worktree.hooks.post_remove.as_deref(),
        &main_workdir,
        "post_remove",
    )?;

    println!(
        "{}  worktree '{}'",
        "Removed".green().bold(),
        display_name.cyan()
    );

    Ok(())
}
