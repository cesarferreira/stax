use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::remote;
use anyhow::{bail, Result};
use colored::Colorize;
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
    let (input, commit_message) = match (&name, &message) {
        (Some(n), _) => (n.clone(), None),
        (None, Some(m)) => (m.clone(), Some(m.clone())),
        (None, None) => bail!("Branch name required. Use: stax bc <name> or stax bc -m \"message\""),
    };

    // -a/--all flag: stage changes (like git commit --all)
    let should_stage = all || message.is_some();

    // Format the branch name according to config
    let branch_name = match prefix.as_deref() {
        Some(_) => config.format_branch_name_with_prefix_override(&input, prefix.as_deref()),
        None => config.format_branch_name(&input),
    };

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

    // Stage changes if -a or -m was used
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
