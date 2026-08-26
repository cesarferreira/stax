use super::pool;
use super::shared::managed_worktrees_dir;
use crate::config::Config;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let before = repo.list_worktrees()?;
    let prunable_before: Vec<_> = before
        .iter()
        .filter(|worktree| worktree.is_prunable)
        .cloned()
        .collect();

    if prunable_before.is_empty() {
        println!("{}", "Nothing to prune.".dimmed());
        return Ok(());
    }

    repo.worktree_prune()?;

    if config.worktree.reuse_slots {
        reconcile_pool_manifest(&repo, &config)?;
    }

    let after = repo.list_worktrees()?;
    let remaining_prunable: Vec<_> = after
        .iter()
        .filter(|worktree| worktree.is_prunable)
        .map(|worktree| worktree.path.clone())
        .collect();

    let pruned = prunable_before
        .iter()
        .filter(|worktree| !remaining_prunable.contains(&worktree.path))
        .count();
    let skipped = prunable_before.len().saturating_sub(pruned);

    println!(
        "{}  {} stale {} pruned",
        "Pruned".green().bold(),
        pruned.to_string().cyan(),
        if pruned == 1 { "entry" } else { "entries" }
    );

    if skipped > 0 {
        println!(
            "  {} {} {} still marked prunable",
            "Skipped".yellow().bold(),
            skipped.to_string().yellow(),
            if skipped == 1 { "entry" } else { "entries" }
        );
    }

    Ok(())
}

/// Drop pool slots whose directory no longer exists on disk.
fn reconcile_pool_manifest(repo: &GitRepo, config: &Config) -> Result<()> {
    let worktrees_dir = managed_worktrees_dir(repo, config)?;
    if !worktrees_dir.exists() {
        return Ok(());
    }

    pool::with_lock(&worktrees_dir, |pool| {
        pool.slots.retain(|slot| slot.path.exists());
        Ok(())
    })
}
