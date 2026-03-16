use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let managed_worktrees = repo
        .list_worktrees()?
        .into_iter()
        .filter(|worktree| {
            worktree.path.exists()
                && !worktree.is_prunable
                && worktree
                    .branch
                    .as_deref()
                    .and_then(|branch| BranchMetadata::read(repo.inner(), branch).ok().flatten())
                    .is_some()
        })
        .collect::<Vec<_>>();

    if managed_worktrees.is_empty() {
        println!("{}", "No stax-managed worktrees found.".dimmed());
        return Ok(());
    }

    println!(
        "Restacking {} stax-managed {}...\n",
        managed_worktrees.len(),
        if managed_worktrees.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        }
    );

    let stax_bin = std::env::current_exe().unwrap_or_else(|_| "stax".into());
    let mut ok = 0usize;
    let mut failed = 0usize;

    for worktree in managed_worktrees {
        let branch = worktree
            .branch
            .clone()
            .unwrap_or_else(|| "(detached)".to_string());
        print!("  {} ({}) ... ", worktree.name.cyan(), branch.dimmed());

        let output = Command::new(&stax_bin)
            .args(["restack", "--all", "--quiet", "--yes"])
            .current_dir(&worktree.path)
            .output()
            .with_context(|| format!("Failed to restack worktree '{}'", worktree.name));

        match output {
            Ok(output) if output.status.success() => {
                println!("{}", "ok".green());
                ok += 1;
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let first_line = stderr.lines().next().unwrap_or("unknown error");
                println!("{} {}", "failed".red(), first_line.dimmed());
                failed += 1;
            }
            Err(error) => {
                println!("{} {}", "failed".red(), error.to_string().dimmed());
                failed += 1;
            }
        }
    }

    println!();
    println!(
        "Done: {} synced, {} failed",
        ok.to_string().green(),
        if failed > 0 {
            failed.to_string().red().to_string()
        } else {
            failed.to_string().dimmed().to_string()
        }
    );

    if failed > 0 {
        anyhow::bail!("{} worktree(s) failed to restack", failed);
    }

    Ok(())
}
