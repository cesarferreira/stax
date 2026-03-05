use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

pub fn run(name: &str, force: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let worktrees = repo.list_worktrees()?;

    let wt = worktrees
        .iter()
        .find(|wt| {
            wt.name == name
                || wt.branch.as_deref() == Some(name)
                || wt.branch
                    .as_deref()
                    .map(|b| b.ends_with(&format!("/{}", name)))
                    .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("No worktree named '{}'", name))?;

    if wt.is_main {
        bail!("Cannot remove the main worktree.");
    }

    if wt.is_current {
        bail!(
            "Cannot remove the worktree you are currently in. Switch to another worktree first."
        );
    }

    // Warn about dirty state unless --force
    if !force && repo.is_dirty_at(&wt.path)? {
        eprintln!(
            "{} Worktree '{}' has uncommitted changes.",
            "Warning:".yellow().bold(),
            name
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

    let path = wt.path.clone();
    let branch = wt.branch.clone().unwrap_or_else(|| name.to_string());

    repo.worktree_remove(&path, force)?;

    println!(
        "{}  worktree '{}' (branch '{}')",
        "Removed".green().bold(),
        name.cyan(),
        branch.blue()
    );

    Ok(())
}
