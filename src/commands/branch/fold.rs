use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::process::Command;

/// Fold the current branch into its parent (merge commits into parent)
pub fn run(keep_branch: bool, skip_confirm: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let workdir = repo.workdir()?;

    // Check if current branch is tracked
    let meta = BranchMetadata::read(repo.inner(), &current)?
        .context("Current branch is not tracked. Use 'stax branch track' first.")?;

    let parent = &meta.parent_branch_name;

    // Don't fold into trunk
    if parent == &stack.trunk {
        println!(
            "{}",
            "Cannot fold into trunk. Use 'stax submit' to merge into trunk via PR.".yellow()
        );
        return Ok(());
    }

    // Check if current branch has children
    if let Some(branch_info) = stack.branches.get(&current) {
        if !branch_info.children.is_empty() {
            println!("{}", "Cannot fold: this branch has children.".red());
            println!("Children: {}", branch_info.children.join(", ").cyan());
            println!();
            println!("Please fold or delete child branches first.");
            return Ok(());
        }
    }

    // Count commits to fold
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
        println!("{}", "No commits to fold.".yellow());
        return Ok(());
    }

    println!(
        "Folding '{}' ({} commit(s)) into '{}'",
        current.cyan(),
        commit_count.to_string().cyan(),
        parent.green()
    );

    // Show the commits
    let log_output = Command::new("git")
        .args(["log", "--oneline", &format!("{}..HEAD", parent)])
        .current_dir(workdir)
        .output()
        .context("Failed to show commits")?;

    println!();
    println!("{}", "Commits to fold:".bold());
    for line in String::from_utf8_lossy(&log_output.stdout).lines() {
        println!("  {}", line.dimmed());
    }
    println!();

    // Confirm (unless --yes flag)
    let action = if keep_branch { "fold" } else { "fold and delete" };
    if !skip_confirm {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("{}  '{}' into '{}'?", action, current, parent))
            .default(true)
            .interact()?;

        if !confirm {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
    }

    // Checkout parent
    print!("Checking out {}... ", parent.cyan());
    let checkout_status = Command::new("git")
        .args(["checkout", parent])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to checkout parent")?;

    if !checkout_status.success() {
        println!("{}", "failed".red());
        anyhow::bail!("Failed to checkout parent branch");
    }
    println!("{}", "done".green());

    // Merge current branch into parent
    print!("Merging {}... ", current.cyan());
    let merge_status = Command::new("git")
        .args(["merge", "--squash", &current])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to merge")?;

    if !merge_status.success() {
        println!("{}", "failed".red());

        // Abort merge and reset working tree
        let _ = Command::new("git")
            .args(["merge", "--abort"])
            .current_dir(workdir)
            .status();

        // Reset any staged changes from the failed squash merge
        let _ = Command::new("git")
            .args(["reset", "--hard", "HEAD"])
            .current_dir(workdir)
            .status();

        // Restore original branch
        let _ = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .status();

        anyhow::bail!(
            "Failed to merge branch. There may be conflicts.\n\
             Restored to branch '{}'.",
            current
        );
    }
    println!("{}", "done".green());

    // Commit the merge
    print!("Committing... ");
    let commit_msg = format!("Fold {} into {}", current, parent);
    let commit_status = Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(workdir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to commit")?;

    if !commit_status.success() {
        // Maybe nothing to commit
        println!("{}", "no changes".yellow());
    } else {
        println!("{}", "done".green());
    }

    // Delete the old branch unless --keep
    if !keep_branch {
        print!("Deleting {}... ", current.cyan());
        let delete_status = Command::new("git")
            .args(["branch", "-D", &current])
            .current_dir(workdir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("Failed to delete branch")?;

        if delete_status.success() {
            // Also delete metadata
            let _ = BranchMetadata::delete(repo.inner(), &current);
            println!("{}", "done".green());
        } else {
            println!("{}", "failed".yellow());
        }
    }

    // Update parent's metadata (it may need restack now)
    if let Some(parent_meta) = BranchMetadata::read(repo.inner(), parent)? {
        let parent_commit = repo.branch_commit(&parent_meta.parent_branch_name)?;
        let updated_parent = BranchMetadata {
            parent_branch_revision: parent_commit,
            ..parent_meta
        };
        updated_parent.write(repo.inner(), parent)?;
    }

    println!();
    println!(
        "{} Folded '{}' into '{}'",
        "✓".green(),
        current,
        parent
    );

    Ok(())
}
