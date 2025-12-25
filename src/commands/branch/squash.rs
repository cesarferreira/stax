use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use std::process::Command;

/// Squash all commits on the current branch into a single commit
pub fn run(message: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?;

    // Check if current branch is tracked
    let meta = BranchMetadata::read(repo.inner(), &current)?
        .context("Current branch is not tracked. Use 'stax branch track' first.")?;

    let parent = &meta.parent_branch_name;

    // Count commits to squash
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..HEAD", parent)])
        .current_dir(workdir)
        .output()
        .context("Failed to count commits")?;

    let commit_count: usize = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);

    if commit_count == 0 {
        println!("{}", "No commits to squash.".yellow());
        return Ok(());
    }

    if commit_count == 1 {
        println!("{}", "Only one commit on this branch, nothing to squash.".yellow());
        return Ok(());
    }

    println!(
        "Found {} commits on '{}' (parent: '{}')",
        commit_count.to_string().cyan(),
        current.cyan(),
        parent.dimmed()
    );

    // Show the commits
    let log_output = Command::new("git")
        .args(["log", "--oneline", &format!("{}..HEAD", parent)])
        .current_dir(workdir)
        .output()
        .context("Failed to show commits")?;

    println!();
    println!("{}", "Commits to squash:".bold());
    for line in String::from_utf8_lossy(&log_output.stdout).lines() {
        println!("  {}", line.dimmed());
    }
    println!();

    // Get commit message
    let squash_message = if let Some(msg) = message {
        msg
    } else {
        // Get the first commit's message as default
        let first_msg_output = Command::new("git")
            .args(["log", "-1", "--format=%s", &format!("{}..HEAD", parent)])
            .current_dir(workdir)
            .output()
            .context("Failed to get commit message")?;

        let default_msg = String::from_utf8_lossy(&first_msg_output.stdout)
            .trim()
            .to_string();

        Input::with_theme(&ColorfulTheme::default())
            .with_prompt("Squash commit message")
            .default(default_msg)
            .interact_text()?
    };

    // Confirm
    let confirm = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Squash {} commits into one?", commit_count))
        .default(true)
        .interact()?;

    if !confirm {
        println!("{}", "Aborted.".red());
        return Ok(());
    }

    // Perform soft reset to parent
    print!("Squashing commits... ");

    let reset_status = Command::new("git")
        .args(["reset", "--soft", parent])
        .current_dir(workdir)
        .status()
        .context("Failed to reset")?;

    if !reset_status.success() {
        println!("{}", "failed".red());
        anyhow::bail!("Failed to reset to parent");
    }

    // Create new squashed commit
    let commit_status = Command::new("git")
        .args(["commit", "-m", &squash_message])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to commit")?;

    if !commit_status.success() {
        println!("{}", "failed".red());
        anyhow::bail!("Failed to create squashed commit");
    }

    println!("{}", "done".green());

    // Update metadata with new parent revision
    let parent_commit = repo.branch_commit(parent)?;
    let updated_meta = BranchMetadata {
        parent_branch_revision: parent_commit,
        ..meta
    };
    updated_meta.write(repo.inner(), &current)?;

    println!();
    println!(
        "{} Squashed {} commits into one.",
        "✓".green(),
        commit_count
    );

    Ok(())
}
