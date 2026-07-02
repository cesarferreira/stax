use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
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
