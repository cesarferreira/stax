use crate::git::GitRepo;
use super::registry::Registry;
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?.to_path_buf();
    let workdir = repo.workdir()?.to_path_buf();

    let mut registry = Registry::load(&git_dir)?;
    let pruned = registry.prune();
    registry.save()?;

    // Also run `git worktree prune` to clean up Git's internal bookkeeping
    let output = Command::new("git")
        .args(["worktree", "prune"])
        .current_dir(&workdir)
        .output();

    match output {
        Ok(o) if !o.status.success() => {
            let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
            eprintln!("{}", format!("  Warning: git worktree prune failed: {}", stderr).yellow());
        }
        Err(e) => {
            eprintln!("{}", format!("  Warning: could not run git worktree prune: {}", e).yellow());
        }
        _ => {}
    }

    if pruned == 0 {
        println!("{}", "Nothing to prune — all registered worktrees exist.".dimmed());
    } else {
        println!(
            "{}  {} stale {}",
            "Pruned".green().bold(),
            pruned,
            if pruned == 1 { "entry" } else { "entries" }
        );
    }

    Ok(())
}
