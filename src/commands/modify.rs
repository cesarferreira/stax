use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

/// Stage all changes and amend them to the current commit
pub fn run(message: Option<String>, quiet: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?;
    let current = repo.current_branch()?;

    // Check if there are any changes to stage
    if !repo.is_dirty()? {
        if !quiet {
            println!("{}", "No changes to amend.".dimmed());
        }
        return Ok(());
    }

    // Stage all changes
    let add_status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workdir)
        .status()
        .context("Failed to stage changes")?;

    if !add_status.success() {
        anyhow::bail!("Failed to stage changes");
    }

    // Amend the commit
    let mut amend_args = vec!["commit", "--amend"];

    if let Some(ref msg) = message {
        amend_args.push("-m");
        amend_args.push(msg);
    } else {
        amend_args.push("--no-edit");
    }

    let amend_status = Command::new("git")
        .args(&amend_args)
        .current_dir(workdir)
        .status()
        .context("Failed to amend commit")?;

    if !amend_status.success() {
        anyhow::bail!("Failed to amend commit");
    }

    if !quiet {
        if message.is_some() {
            println!("{} {}", "Amended".green(), current.cyan());
        } else {
            println!("{} {} {}", "Amended".green(), current.cyan(), "(keeping message)".dimmed());
        }
    }

    Ok(())
}
