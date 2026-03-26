use crate::engine::BranchMetadata;
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

    // Determine whether the current branch has any of its own commits.
    // When a branch is freshly created (e.g. `stax create <name>`) without an
    // initial commit, HEAD still points at the parent branch's tip.  Amending
    // in that state would rewrite a commit that belongs to the parent, wrongly
    // changing its author/date and silently including unrelated history.
    // Instead, create a brand-new commit in this case.
    let branch_has_own_commits = {
        let meta = BranchMetadata::read(repo.inner(), &current)?;
        match meta {
            Some(m) => {
                let parent_sha = repo.branch_commit(&m.parent_branch_name).ok();
                let head_sha = repo.branch_commit(&current).ok();
                // If we can resolve both, compare them; any difference means the
                // branch already has at least one commit of its own.
                match (parent_sha, head_sha) {
                    (Some(p), Some(h)) => p != h,
                    // If resolution fails, fall back to amend (safe default for
                    // branches not managed by stax).
                    _ => true,
                }
            }
            // No stax metadata → branch was not created by stax; assume it has
            // its own commits and use the normal amend path.
            None => true,
        }
    };

    // Stage all changes
    let add_status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workdir)
        .status()
        .context("Failed to stage changes")?;

    if !add_status.success() {
        anyhow::bail!("Failed to stage changes");
    }

    if branch_has_own_commits {
        // Amend the existing commit on this branch.
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
                println!(
                    "{} {} {}",
                    "Amended".green(),
                    current.cyan(),
                    "(keeping message)".dimmed()
                );
            }
        }
    } else {
        // Branch has no commits of its own yet – create a new commit rather
        // than amending the parent branch's tip.
        let commit_message = message.as_deref().unwrap_or("WIP");

        let commit_status = Command::new("git")
            .args(["commit", "-m", commit_message])
            .current_dir(workdir)
            .status()
            .context("Failed to create commit")?;

        if !commit_status.success() {
            anyhow::bail!("Failed to create commit");
        }

        if !quiet {
            println!("{} {}", "Committed".green(), current.cyan());
        }
    }

    Ok(())
}
