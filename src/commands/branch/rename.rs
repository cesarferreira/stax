use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm, Input};
use std::io::IsTerminal;
use std::process::Command;

/// Rename the current branch and optionally edit the commit message
pub fn run(new_name: Option<String>, edit_message: bool, push_remote: bool, literal: bool) -> Result<()> {
    let is_interactive = std::io::stdin().is_terminal();
    let repo = GitRepo::open()?;
    let old_name = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;
    let config = Config::load()?;
    let workdir = repo.workdir()?;

    if old_name == trunk {
        anyhow::bail!("Cannot rename the trunk branch '{}'", trunk);
    }

    // Get new name
    let new_name = match new_name {
        Some(name) => {
            if literal {
                name // Use as-is without prefix
            } else {
                config.format_branch_name(&name)
            }
        }
        None => {
            if !is_interactive {
                anyhow::bail!("New branch name required in non-interactive mode");
            }
            let input: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("New branch name")
                .interact_text()?;
            config.format_branch_name(&input)
        }
    };

    if new_name == old_name {
        println!("Branch name unchanged.");
        return Ok(());
    }

    // Check if new name already exists
    if repo.branch_commit(&new_name).is_ok() {
        anyhow::bail!("Branch '{}' already exists", new_name);
    }

    // Load stack to find children that reference this branch
    let stack = Stack::load(&repo)?;

    // 1. Rename the local branch
    let status = Command::new("git")
        .args(["branch", "-m", &old_name, &new_name])
        .current_dir(workdir)
        .status()
        .context("Failed to rename branch")?;

    if !status.success() {
        anyhow::bail!("Failed to rename branch from '{}' to '{}'", old_name, new_name);
    }

    println!(
        "✓ Renamed branch '{}' → '{}'",
        old_name.bright_black(),
        new_name.green()
    );

    // 2. Update metadata - copy old metadata to new branch name
    if let Some(meta) = BranchMetadata::read(repo.inner(), &old_name)? {
        meta.write(repo.inner(), &new_name)?;
        crate::git::refs::delete_metadata(repo.inner(), &old_name)?;
    }

    // 3. Update any children that have this branch as parent
    for (child_name, child_info) in &stack.branches {
        if child_info.parent.as_deref() == Some(&old_name) {
            if let Some(mut meta) = BranchMetadata::read(repo.inner(), child_name)? {
                meta.parent_branch_name = new_name.clone();
                meta.write(repo.inner(), child_name)?;
                println!(
                    "  Updated child '{}' to reference new parent",
                    child_name.cyan()
                );
            }
        }
    }

    // 4. Handle remote branch
    let remote_name = config.remote_name();
    let remote_branches = crate::remote::get_remote_branches(workdir, remote_name).unwrap_or_default();

    if remote_branches.contains(&old_name) {
        let should_push = if push_remote {
            true // --push flag was passed
        } else if is_interactive {
            Confirm::with_theme(&ColorfulTheme::default())
                .with_prompt(format!(
                    "Push '{}' and delete old remote '{}'?",
                    new_name, old_name
                ))
                .default(true)
                .interact()?
        } else {
            false
        };

        if should_push {
            // Push new branch
            print!("  Pushing {}... ", new_name.cyan());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let push_status = Command::new("git")
                .args(["push", "-u", remote_name, &new_name])
                .current_dir(workdir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            if push_status.map(|s| s.success()).unwrap_or(false) {
                println!("{}", "✓".green());
            } else {
                println!("{}", "failed".red());
            }

            // Delete old remote branch
            print!("  Deleting remote {}... ", old_name.bright_black());
            std::io::Write::flush(&mut std::io::stdout()).ok();
            let delete_status = Command::new("git")
                .args(["push", remote_name, "--delete", &old_name])
                .current_dir(workdir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();

            if delete_status.map(|s| s.success()).unwrap_or(false) {
                println!("{}", "✓".green());
            } else {
                println!("{}", "failed".red());
            }
        }
    }

    // 5. Optionally edit commit message
    let should_edit = if edit_message {
        true
    } else if is_interactive {
        Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Edit the commit message?")
            .default(false)
            .interact()?
    } else {
        false
    };

    if should_edit {
        let status = Command::new("git")
            .args(["commit", "--amend"])
            .current_dir(workdir)
            .status()
            .context("Failed to amend commit")?;

        if status.success() {
            println!("✓ Commit message updated");
        }
    }

    println!();
    println!(
        "Branch renamed: {} → {}",
        old_name.bright_black(),
        new_name.green().bold()
    );

    Ok(())
}
