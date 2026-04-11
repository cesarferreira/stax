use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Result of file-level attribution: which branch owns each changed file.
struct AbsorbPlan {
    /// Files grouped by target branch name.
    groups: Vec<(String, Vec<String>)>,
    /// Files that could not be attributed to any stack branch.
    unattributed: Vec<String>,
}

pub fn run(dry_run: bool, all: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        bail!("Cannot absorb on trunk. Checkout a stacked branch first.");
    }

    let workdir = repo.workdir()?;

    // Stage all if requested
    if all {
        let status = Command::new("git")
            .args(["add", "-A"])
            .current_dir(workdir)
            .status()?;
        if !status.success() {
            bail!("Failed to stage changes");
        }
    }

    // Get list of staged files
    let staged_output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(workdir)
        .output()?;

    if !staged_output.status.success() {
        bail!("Failed to list staged files");
    }

    let staged_files: Vec<String> = String::from_utf8_lossy(&staged_output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|s| s.to_string())
        .collect();

    if staged_files.is_empty() {
        println!(
            "{}",
            "No staged changes to absorb. Stage files or use `st absorb -a`.".yellow()
        );
        return Ok(());
    }

    // Build the stack branch list (from trunk up to current, excluding trunk)
    let ancestors = stack.ancestors(&current);
    let mut stack_branches: Vec<String> = ancestors.into_iter().rev().collect();
    stack_branches.push(current.clone());
    // Remove trunk from the list
    stack_branches.retain(|b| *b != stack.trunk);

    if stack_branches.is_empty() {
        bail!("No stack branches found above trunk.");
    }

    // For each branch, find its parent boundary for commit scoping
    let mut branch_boundaries: Vec<(String, String)> = Vec::new();
    for branch in &stack_branches {
        let meta = BranchMetadata::read(repo.inner(), branch)?;
        let parent = meta
            .as_ref()
            .map(|m| m.parent_branch_name.clone())
            .unwrap_or_else(|| stack.trunk.clone());
        branch_boundaries.push((branch.clone(), parent));
    }

    // Attribute each file to a branch
    let plan = attribute_files(workdir, &staged_files, &branch_boundaries)?;

    // Display the plan
    if plan.groups.is_empty() && plan.unattributed.is_empty() {
        println!("{}", "No changes to absorb.".yellow());
        return Ok(());
    }

    println!("{}", "Absorb plan:".bold());
    for (branch, files) in &plan.groups {
        let marker = if *branch == current {
            " (current)".dimmed().to_string()
        } else {
            String::new()
        };
        println!("  {} {}{}", "→".green(), branch.cyan(), marker);
        for file in files {
            println!("    {}", file);
        }
    }
    if !plan.unattributed.is_empty() {
        println!(
            "  {} {}",
            "?".yellow(),
            "unattributed (staying staged)".dimmed()
        );
        for file in &plan.unattributed {
            println!("    {}", file);
        }
    }
    println!();

    if dry_run {
        println!("{}", "Dry run — no changes made.".dimmed());
        return Ok(());
    }

    // Check if there are changes targeting other branches (not just current)
    let other_branch_groups: Vec<_> = plan
        .groups
        .iter()
        .filter(|(b, _)| *b != current)
        .collect();

    if other_branch_groups.is_empty() {
        println!(
            "{}",
            "All changes already target the current branch. Nothing to absorb.".dimmed()
        );
        return Ok(());
    }

    // Perform absorption: move changes to their target branches
    // Strategy: extract patches per group, stash everything, apply to each branch
    let mut patches: Vec<(String, String, Vec<String>)> = Vec::new(); // (branch, patch_content, files)

    for (branch, files) in &plan.groups {
        if *branch == current {
            continue; // Leave current-branch changes staged
        }

        // Extract patch for these files
        let mut diff_args = vec!["diff".to_string(), "--cached".to_string(), "--".to_string()];
        diff_args.extend(files.iter().cloned());

        let diff_output = Command::new("git")
            .args(&diff_args)
            .current_dir(workdir)
            .output()?;

        if diff_output.status.success() && !diff_output.stdout.is_empty() {
            patches.push((
                branch.clone(),
                String::from_utf8_lossy(&diff_output.stdout).to_string(),
                files.clone(),
            ));
        }
    }

    if patches.is_empty() {
        println!("{}", "No changes to move to other branches.".dimmed());
        return Ok(());
    }

    // Stash current state (include untracked files with -u)
    let stash_output = Command::new("git")
        .args(["stash", "push", "-u", "-m", "stax-absorb"])
        .current_dir(workdir)
        .output()?;

    let stash_msg = String::from_utf8_lossy(&stash_output.stdout);
    let stashed = stash_output.status.success() && !stash_msg.contains("No local changes to save");

    if !stashed {
        bail!("Failed to stash changes before absorbing");
    }

    let mut absorbed_files: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    for (branch, patch, files) in &patches {
        // Checkout target branch
        let co = Command::new("git")
            .args(["checkout", branch])
            .current_dir(workdir)
            .status()?;

        if !co.success() {
            errors.push(format!("Failed to checkout '{}'", branch));
            let _ = Command::new("git")
                .args(["checkout", &current])
                .current_dir(workdir)
                .status();
            continue;
        }

        // Apply the patch
        let mut apply_cmd = Command::new("git")
            .args(["apply", "--cached", "-"])
            .current_dir(workdir)
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = apply_cmd.stdin.take() {
            use std::io::Write;
            let _ = stdin.write_all(patch.as_bytes());
            drop(stdin); // Close stdin before wait to avoid deadlock
        }
        let apply_status = apply_cmd.wait()?;

        if !apply_status.success() {
            errors.push(format!(
                "Failed to apply patch to '{}' (files may conflict)",
                branch
            ));
            // Reset and go back
            let _ = Command::new("git")
                .args(["reset"])
                .current_dir(workdir)
                .status();
            let _ = Command::new("git")
                .args(["checkout", &current])
                .current_dir(workdir)
                .status();
            continue;
        }

        // Get the tip commit message for the fixup label
        let tip_msg = get_branch_tip_message(workdir, branch);

        // Commit
        let commit_msg = format!("fixup! {}", tip_msg.unwrap_or_else(|| branch.clone()));
        let commit_status = Command::new("git")
            .args(["commit", "-m", &commit_msg])
            .current_dir(workdir)
            .status()?;

        if !commit_status.success() {
            errors.push(format!("Failed to commit fixup on '{}'", branch));
            let _ = Command::new("git")
                .args(["reset"])
                .current_dir(workdir)
                .status();
        } else {
            absorbed_files.extend(files.iter().cloned());
            println!(
                "  {} {} file(s) → {}",
                "✓".green(),
                files.len(),
                branch.cyan()
            );
        }

        // Return to original branch
        let _ = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .status();
    }

    // Restore stash
    let pop = Command::new("git")
        .args(["stash", "pop"])
        .current_dir(workdir)
        .status()?;

    if !pop.success() {
        println!(
            "{}",
            "Warning: failed to pop stash. Run `git stash pop` manually.".yellow()
        );
    }

    // Unstage and discard absorbed files.
    // For tracked files: reset index + checkout from HEAD.
    // For untracked/new files: reset index + remove from working tree.
    for file in &absorbed_files {
        // Unstage
        let _ = Command::new("git")
            .args(["reset", "HEAD", "--", file])
            .current_dir(workdir)
            .status();

        // Try to checkout from HEAD (works for tracked files)
        let checkout = Command::new("git")
            .args(["checkout", "HEAD", "--", file])
            .current_dir(workdir)
            .status();

        // If checkout failed (untracked file not in HEAD), remove it
        if checkout.map(|s| !s.success()).unwrap_or(true) {
            let file_path = workdir.join(file);
            let _ = std::fs::remove_file(file_path);
        }
    }

    if !errors.is_empty() {
        println!();
        println!("{}", "Some files could not be absorbed:".yellow());
        for e in &errors {
            println!("  {}", e);
        }
    }

    println!();
    println!("{}", "Absorb complete.".green());

    Ok(())
}

/// Attribute each staged file to a stack branch based on which branch most recently
/// modified it (file-level attribution via `git log`).
fn attribute_files(
    workdir: &Path,
    files: &[String],
    branch_boundaries: &[(String, String)],
) -> Result<AbsorbPlan> {
    let mut branch_files: HashMap<String, Vec<String>> = HashMap::new();
    let mut unattributed: Vec<String> = Vec::new();

    for file in files {
        let mut attributed = false;

        // Walk branches from top to bottom (most recent first)
        for (branch, parent) in branch_boundaries.iter().rev() {
            let output = Command::new("git")
                .args([
                    "log",
                    "--oneline",
                    "-1",
                    &format!("{}..{}", parent, branch),
                    "--",
                    file,
                ])
                .current_dir(workdir)
                .output()?;

            if output.status.success() && !output.stdout.is_empty() {
                branch_files
                    .entry(branch.clone())
                    .or_default()
                    .push(file.clone());
                attributed = true;
                break;
            }
        }

        if !attributed {
            // File was not modified by any stack branch (new file or changed on trunk)
            // Default to current branch
            unattributed.push(file.clone());
        }
    }

    // Build ordered groups matching the branch order
    let groups: Vec<(String, Vec<String>)> = branch_boundaries
        .iter()
        .filter_map(|(branch, _)| {
            branch_files
                .get(branch)
                .map(|files| (branch.clone(), files.clone()))
        })
        .collect();

    Ok(AbsorbPlan {
        groups,
        unattributed,
    })
}

/// Get the first-line commit message of a branch's tip.
fn get_branch_tip_message(workdir: &Path, branch: &str) -> Option<String> {
    Command::new("git")
        .args(["log", "-1", "--format=%s", branch])
        .current_dir(workdir)
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                let msg = String::from_utf8_lossy(&o.stdout).trim().to_string();
                if msg.is_empty() {
                    None
                } else {
                    Some(msg)
                }
            } else {
                None
            }
        })
}
