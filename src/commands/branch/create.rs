use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::remote;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Input, Select};
use std::path::Path;
use std::process::Command;

pub fn run(
    name: Option<String>,
    message: Option<String>,
    from: Option<String>,
    prefix: Option<String>,
    all: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let current = repo.current_branch()?;
    let parent_branch = from.unwrap_or_else(|| current.clone());

    if repo.branch_commit(&parent_branch).is_err() {
        anyhow::bail!("Branch '{}' does not exist", parent_branch);
    }

    // Get the branch name from either name or message
    // When using -m, the message is used for both branch name AND commit message
    // When using -a (--all), stage changes but only commit if -m is also provided
    // When neither is provided, launch interactive wizard
    let (input, commit_message, should_stage) = match (&name, &message) {
        (Some(n), _) => (n.clone(), None, all),
        (None, Some(m)) => (m.clone(), Some(m.clone()), true),
        (None, None) => {
            // Launch interactive wizard
            let (wizard_name, wizard_msg, wizard_stage) =
                run_wizard(repo.workdir()?, &parent_branch)?;
            (wizard_name, wizard_msg, wizard_stage)
        }
    };

    // Format the branch name according to config
    let branch_name = match prefix.as_deref() {
        Some(_) => config.format_branch_name_with_prefix_override(&input, prefix.as_deref()),
        None => config.format_branch_name(&input),
    };

    // Check for branch name conflicts (Git doesn't allow both "foo" and "foo/bar")
    let existing_branches = repo.list_branches().unwrap_or_default();
    for existing in &existing_branches {
        // Check if new branch would be a child path of existing (e.g., creating "foo/bar" when "foo" exists)
        if branch_name.starts_with(&format!("{}/", existing)) {
            bail!(
                "Cannot create '{}': branch '{}' already exists.\n\
                 Git doesn't allow a branch and its sub-path to coexist.\n\
                 Either delete '{}' first, or use a different name like '{}-ui'.",
                branch_name,
                existing,
                existing,
                existing
            );
        }
        // Check if existing branch is a child path of new (e.g., creating "foo" when "foo/bar" exists)
        if existing.starts_with(&format!("{}/", branch_name)) {
            bail!(
                "Cannot create '{}': branch '{}' already exists.\n\
                 Git doesn't allow a branch and its sub-path to coexist.\n\
                 Either delete '{}' first, or use a different name.",
                branch_name,
                existing,
                existing
            );
        }
    }

    // Create the branch
    if parent_branch == current {
        repo.create_branch(&branch_name)?;
    } else {
        repo.create_branch_at(&branch_name, &parent_branch)?;
    }

    // Track it with current branch as parent
    let parent_rev = repo.branch_commit(&parent_branch)?;
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &branch_name)?;

    // Checkout the new branch
    repo.checkout(&branch_name)?;

    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name()) {
        if !remote_branches.contains(&parent_branch) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }

    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        parent_branch.blue()
    );

    // Stage changes if -a or -m was used or wizard selected it
    if should_stage {
        let workdir = repo.workdir()?;

        // Stage all changes (git add -A)
        let add_status = Command::new("git")
            .args(["add", "-A"])
            .current_dir(workdir)
            .status()?;

        if !add_status.success() {
            bail!("Failed to stage changes");
        }

        // Only commit if -m was provided
        if let Some(msg) = commit_message {
            // Check if there are changes to commit
            let diff_output = Command::new("git")
                .args(["diff", "--cached", "--quiet"])
                .current_dir(workdir)
                .status()?;

            if !diff_output.success() {
                // There are staged changes, commit them
                let commit_status = Command::new("git")
                    .args(["commit", "-m", &msg])
                    .current_dir(workdir)
                    .status()?;

                if !commit_status.success() {
                    bail!("Failed to commit changes");
                }

                println!("Committed: {}", msg.cyan());
            } else {
                println!("{}", "No changes to commit".dimmed());
            }
        } else {
            println!("{}", "Changes staged".dimmed());
        }
    }

    Ok(())
}

/// Interactive wizard for branch creation when no arguments provided
fn run_wizard(workdir: &Path, parent_branch: &str) -> Result<(String, Option<String>, bool)> {
    // Show header
    println!();
    println!("╭─ Create Stacked Branch ─────────────────────────────╮");
    println!(
        "│ Parent: {:<43} │",
        format!("{} (current branch)", parent_branch.cyan())
    );
    println!("╰─────────────────────────────────────────────────────╯");
    println!();

    // 1. Branch name prompt (required)
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Branch name")
        .interact_text()?;

    if name.trim().is_empty() {
        bail!("Branch name cannot be empty");
    }

    // 2. Check for uncommitted changes
    let has_changes = has_uncommitted_changes(workdir);
    let change_count = count_uncommitted_changes(workdir);

    let (should_stage, commit_message) = if has_changes {
        println!();

        // Show staging options with change count
        let stage_label = if change_count > 0 {
            format!("Stage all changes ({} files modified)", change_count)
        } else {
            "Stage all changes".to_string()
        };

        let options = vec![stage_label.as_str(), "Empty branch (no changes)"];

        let choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("What to include")
            .items(&options)
            .default(0)
            .interact()?;

        let stage = choice == 0;

        // 3. Optional commit message (only if staging)
        let msg = if stage {
            println!();
            let m: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Commit message (Enter to skip)")
                .allow_empty(true)
                .interact_text()?;
            if m.is_empty() {
                None
            } else {
                Some(m)
            }
        } else {
            None
        };

        (stage, msg)
    } else {
        (false, None)
    };

    println!();
    Ok((name, commit_message, should_stage))
}

/// Check if there are uncommitted changes in the working directory
fn has_uncommitted_changes(workdir: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Count the number of files with uncommitted changes
fn count_uncommitted_changes(workdir: &Path) -> usize {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .lines()
                .filter(|l| !l.is_empty())
                .count()
        })
        .unwrap_or(0)
}
