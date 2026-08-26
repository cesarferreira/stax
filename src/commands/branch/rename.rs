use crate::application::{NoopOperationReporter, RepositorySession};
use crate::config::Config;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{Confirm, Input, theme::ColorfulTheme};
use std::io::IsTerminal;
use std::process::Command;

/// Rename the current branch and optionally edit the commit message
pub fn run(
    new_name: Option<String>,
    edit_message: bool,
    push_remote: bool,
    literal: bool,
) -> Result<()> {
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

    RepositorySession::open(workdir)?.rename_branch(
        &old_name,
        &new_name,
        &mut NoopOperationReporter,
    )?;

    println!(
        "✓ Renamed branch '{}' → '{}'",
        old_name.bright_black(),
        new_name.green()
    );

    // Handle remote branch
    let remote_name = config.remote_name();
    let remote_branches =
        crate::remote::get_remote_branches(workdir, remote_name).unwrap_or_default();

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

    // Optionally edit commit message
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
