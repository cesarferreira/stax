use super::registry::Registry;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?.to_path_buf();

    let registry = Registry::load(&git_dir)?;

    if registry.entries.is_empty() {
        println!("{}", "No agent worktrees registered.".dimmed());
        return Ok(());
    }

    let existing: Vec<_> = registry
        .entries
        .iter()
        .filter(|e| e.path.exists())
        .collect();

    if existing.is_empty() {
        println!(
            "{}",
            "No live worktrees found. Run `stax agent prune` to clean up.".yellow()
        );
        return Ok(());
    }

    println!(
        "Syncing {} agent {}...\n",
        existing.len(),
        if existing.len() == 1 {
            "worktree"
        } else {
            "worktrees"
        }
    );

    let stax_bin = std::env::current_exe().unwrap_or_else(|_| "stax".into());

    let mut ok = 0usize;
    let mut failed = 0usize;

    for entry in &existing {
        print!("  {} ({}) ... ", entry.name.cyan(), entry.branch.dimmed());

        let output = Command::new(&stax_bin)
            .args(["restack", "--all", "--quiet", "--yes"])
            .current_dir(&entry.path)
            .output();

        match output {
            Ok(o) if o.status.success() => {
                println!("{}", "ok".green());
                ok += 1;
            }
            Ok(o) => {
                let stderr = String::from_utf8_lossy(&o.stderr).trim().to_string();
                let first_line = stderr.lines().next().unwrap_or("unknown error");
                println!("{} — {}", "failed".red(), first_line.dimmed());
                failed += 1;
            }
            Err(e) => {
                println!("{} — {}", "failed".red(), e.to_string().dimmed());
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
        anyhow::bail!("{} worktree(s) failed to sync", failed);
    }

    Ok(())
}
