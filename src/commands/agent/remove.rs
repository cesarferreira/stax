use super::registry::Registry;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use std::process::Command;

pub fn run(name_or_slug: Option<String>, force: bool, delete_branch: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let git_dir = repo.git_dir()?.to_path_buf();
    let workdir = repo.workdir()?.to_path_buf();

    let mut registry = Registry::load(&git_dir)?;

    if registry.entries.is_empty() {
        bail!("No agent worktrees registered.");
    }

    let entry = match name_or_slug {
        Some(ref name) => registry
            .find_by_name(name)
            .with_context(|| format!("No agent worktree found for '{}'", name))?
            .clone(),
        None => {
            let items: Vec<String> = registry
                .entries
                .iter()
                .map(|e| format!("{} ({})", e.name, e.branch))
                .collect();

            let selection =
                dialoguer::FuzzySelect::with_theme(&dialoguer::theme::ColorfulTheme::default())
                    .with_prompt("Select agent worktree to remove")
                    .items(&items)
                    .default(0)
                    .interact()
                    .context("Picker cancelled")?;

            registry.entries[selection].clone()
        }
    };

    // Remove the git worktree
    if entry.path.exists() {
        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        let path_str = entry
            .path
            .to_str()
            .context("Non-UTF-8 worktree path")?
            .to_string();
        args.push(&path_str);

        let output = Command::new("git")
            .args(&args)
            .current_dir(&workdir)
            .output()
            .context("Failed to run git worktree remove")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            if force {
                eprintln!(
                    "{}",
                    format!("  Warning: git worktree remove failed: {}", stderr).yellow()
                );
            } else {
                bail!(
                    "git worktree remove failed: {}\n  Hint: use --force to remove dirty worktrees.",
                    stderr
                );
            }
        }
    }

    // Optionally delete the branch and its metadata
    if delete_branch {
        let branch = entry.branch.clone();
        let current = repo.current_branch().unwrap_or_default();
        if branch == current {
            eprintln!(
                "{}",
                format!(
                    "  Skipping branch deletion: '{}' is currently checked out.",
                    branch
                )
                .yellow()
            );
        } else {
            // Force-delete since worktree is gone; user explicitly asked
            if let Err(e) = repo.delete_branch(&branch, true) {
                eprintln!(
                    "{}",
                    format!("  Warning: could not delete branch '{}': {}", branch, e).yellow()
                );
            } else {
                let _ = BranchMetadata::delete(repo.inner(), &branch);
                println!("  Deleted branch '{}'", branch.blue());
            }
        }
    }

    registry.remove_by_name(&entry.name);
    registry.save()?;

    println!(
        "{}  worktree '{}'",
        "Removed".green().bold(),
        entry.name.cyan()
    );

    Ok(())
}
