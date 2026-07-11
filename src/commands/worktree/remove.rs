use super::pool;
use super::shared::{
    compute_worktree_details, find_current_worktree, find_worktree, managed_worktrees_dir,
    run_blocking_hook, spawn_background_hook, worktree_removal_blockers,
};
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::git::repo::WorktreeInfo;
use anyhow::{Result, bail};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use std::io::IsTerminal;

fn effective_remove_force(force: bool, confirmed_dirty_removal: bool) -> bool {
    force || confirmed_dirty_removal
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemovalMode {
    AllowParking,
    RealRemove,
}

pub(crate) fn remove_worktree_with_hooks(
    repo: &GitRepo,
    config: &Config,
    worktree: &WorktreeInfo,
    force: bool,
    mode: RemovalMode,
) -> Result<String> {
    let main_workdir = repo.main_repo_workdir()?;
    run_blocking_hook(
        config.worktree.hooks.pre_remove.as_deref(),
        &main_workdir,
        "pre_remove",
    )?;

    let display_name = retire_worktree(repo, config, worktree, force, mode)?;

    spawn_background_hook(
        config.worktree.hooks.post_remove.as_deref(),
        &main_workdir,
        "post_remove",
    )?;

    Ok(display_name)
}

pub(crate) fn retire_worktree(
    repo: &GitRepo,
    config: &Config,
    worktree: &WorktreeInfo,
    force: bool,
    mode: RemovalMode,
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

    let display_name = worktree
        .branch
        .clone()
        .unwrap_or_else(|| worktree.name.clone());
    let removing_current_process_worktree = process_is_inside_worktree(&worktree.path);
    let pool_dir = if config.worktree.reuse_slots && !removing_current_process_worktree {
        pool_dir_for(repo, config, &worktree.path)?
    } else {
        None
    };

    // A `--force` dirty removal must NEVER park: the caller explicitly asked to
    // discard the worktree, so parking (which keeps the directory) would defeat
    // the intent and hand out a slot that still carries the dirty tree.
    if mode == RemovalMode::AllowParking && config.worktree.reuse_slots && !force {
        if let Some(worktrees_dir) = pool_dir.as_deref() {
            if try_park_slot(repo, config, worktree, worktrees_dir)? {
                return Ok(display_name);
            }
        }
    }

    repo.worktree_remove(&worktree.path, force)?;

    // Real removal: forget any pooled slot entry that referenced this path.
    if config.worktree.reuse_slots {
        if let Some(worktrees_dir) = pool_dir.as_deref() {
            let path = worktree.path.clone();
            let _ = pool::with_lock(worktrees_dir, |pool| {
                pool.remove_path(&path);
                Ok(())
            });
        }
    }

    Ok(display_name)
}

fn process_is_inside_worktree(worktree_path: &std::path::Path) -> bool {
    let Ok(cwd) = std::env::current_dir() else {
        return false;
    };
    let canonical_cwd = std::fs::canonicalize(&cwd).unwrap_or(cwd);
    let canonical_worktree =
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    canonical_cwd.starts_with(canonical_worktree)
}

/// The managed worktrees directory when `worktree_path` lives inside it (so its
/// pool manifest applies), otherwise `None`.
fn pool_dir_for(
    repo: &GitRepo,
    config: &Config,
    worktree_path: &std::path::Path,
) -> Result<Option<std::path::PathBuf>> {
    let worktrees_dir = managed_worktrees_dir(repo, config)?;
    // Git reports canonicalized worktree paths, while the managed root is derived
    // from raw config/HOME, so canonicalize both before the containment check to
    // avoid /var vs /private/var (macOS) and symlinked-home mismatches.
    let canonical_root =
        std::fs::canonicalize(&worktrees_dir).unwrap_or_else(|_| worktrees_dir.clone());
    let canonical_path =
        std::fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
    Ok(canonical_path
        .starts_with(&canonical_root)
        .then_some(worktrees_dir))
}

/// Attempt to park a disposable worktree as an idle warm slot. Returns `true`
/// when the slot was parked (directory kept), `false` when it is not disposable
/// and the caller should fall through to a real removal.
fn try_park_slot(
    repo: &GitRepo,
    config: &Config,
    worktree: &WorktreeInfo,
    worktrees_dir: &std::path::Path,
) -> Result<bool> {
    let detail = compute_worktree_details(repo, worktree.clone())?;
    if detail.dirty || !worktree_removal_blockers(&detail).is_empty() {
        return Ok(false);
    }

    // Only recycle a branch that is safe to discard (merged/equivalent to trunk).
    if let Some(branch) = worktree.branch.as_deref() {
        if !repo.is_branch_merged_equivalent_to_trunk(branch)? {
            return Ok(false);
        }
    }

    let idle_count = pool::load(worktrees_dir)?.idle_count();
    if idle_count >= config.worktree.max_idle_slots {
        return Ok(false);
    }

    let trunk = repo.trunk_branch()?;
    repo.park_slot(&worktree.path, &trunk)?;

    let path = worktree.path.clone();
    let branch = worktree.branch.clone();
    pool::with_lock(worktrees_dir, |pool| {
        pool.mark_idle(&path, branch);
        Ok(())
    })?;

    Ok(true)
}

pub(crate) fn run_real_remove(
    name: Option<String>,
    force: bool,
    delete_branch: bool,
) -> Result<()> {
    run_with_mode(name, force, delete_branch, RemovalMode::RealRemove)
}

pub fn run(name: Option<String>, force: bool, delete_branch: bool) -> Result<()> {
    run_with_mode(name, force, delete_branch, RemovalMode::AllowParking)
}

fn run_with_mode(
    name: Option<String>,
    force: bool,
    delete_branch: bool,
    mode: RemovalMode,
) -> Result<()> {
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
        if !std::io::stdin().is_terminal() {
            bail!(
                "`st wt rm` needs confirmation to remove a dirty worktree in non-interactive mode. Re-run with `--force`."
            );
        }
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
    let display_name = remove_worktree_with_hooks(&repo, &config, &worktree, remove_force, mode)?;

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
