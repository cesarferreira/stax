use super::shared::{find_worktree, pick_worktree_interactively};
use crate::git::GitRepo;
use crate::git::repo::WorktreeInfo;
use anyhow::{Result, bail};
use colored::Colorize;

fn resolve(repo: &GitRepo, name: Option<String>) -> Result<WorktreeInfo> {
    match name {
        Some(name) => find_worktree(repo, &name)?
            .ok_or_else(|| anyhow::anyhow!("No worktree named '{}'", name)),
        None => pick_worktree_interactively(repo),
    }
}

pub fn run_lock(name: Option<String>, reason: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let worktree = resolve(&repo, name)?;
    if worktree.is_main {
        bail!("The main worktree cannot be locked");
    }
    if worktree.is_locked {
        bail!("Worktree '{}' is already locked", worktree.name);
    }
    repo.worktree_lock(&worktree.path, reason.as_deref())?;
    println!("{}  {}", "Locked".green().bold(), worktree.name.cyan());
    Ok(())
}

pub fn run_unlock(name: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let worktree = resolve(&repo, name)?;
    if worktree.is_main {
        bail!("The main worktree cannot be unlocked");
    }
    if !worktree.is_locked {
        bail!("Worktree '{}' is not locked", worktree.name);
    }
    repo.worktree_unlock(&worktree.path)?;
    println!("{}  {}", "Unlocked".green().bold(), worktree.name.cyan());
    Ok(())
}
