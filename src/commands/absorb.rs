use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use std::collections::HashMap;
use std::io::Write;
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

    // Build (branch, parent) pairs from the Stack (no extra metadata reads needed)
    let ancestors = stack.ancestors(&current);
    let mut stack_branches: Vec<String> = ancestors.into_iter().rev().collect();
    stack_branches.push(current.clone());
    stack_branches.retain(|b| *b != stack.trunk);

    if stack_branches.is_empty() {
        bail!("No stack branches found above trunk.");
    }

    let branch_boundaries: Vec<(String, String)> = stack_branches
        .iter()
        .map(|branch| {
            let parent = stack
                .branches
                .get(branch)
                .and_then(|b| b.parent.clone())
                .unwrap_or_else(|| stack.trunk.clone());
            (branch.clone(), parent)
        })
        .collect();

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
    let has_other_targets = plan.groups.iter().any(|(b, _)| *b != current);

    if !has_other_targets {
        println!(
            "{}",
            "All changes already target the current branch. Nothing to absorb.".dimmed()
        );
        return Ok(());
    }

    // Extract patches as raw bytes (preserves binary diffs)
    let mut patches: Vec<(String, Vec<u8>, Vec<String>)> = Vec::new();

    for (branch, files) in &plan.groups {
        if *branch == current {
            continue;
        }

        let mut diff_args = vec!["diff".to_string(), "--cached".to_string(), "--".to_string()];
        diff_args.extend(files.iter().cloned());

        let diff_output = Command::new("git")
            .args(&diff_args)
            .current_dir(workdir)
            .output()?;

        if diff_output.status.success() && !diff_output.stdout.is_empty() {
            patches.push((branch.clone(), diff_output.stdout, files.clone()));
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

    for (branch, patch_bytes, files) in &patches {
        // Checkout target branch
        let co = Command::new("git")
            .args(["checkout", branch])
            .current_dir(workdir)
            .status()?;

        if !co.success() {
            errors.push(format!("Failed to checkout '{}'", branch));
            break; // Can't continue safely if checkout fails
        }

        // Apply the patch (git apply reads from stdin when no path is given)
        let mut apply_cmd = Command::new("git")
            .args(["apply", "--cached"])
            .current_dir(workdir)
            .stdin(std::process::Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = apply_cmd.stdin.take() {
            stdin.write_all(patch_bytes)?;
            drop(stdin);
        }
        let apply_status = apply_cmd.wait()?;

        if !apply_status.success() {
            errors.push(format!(
                "Failed to apply patch to '{}' (files may have diverged)",
                branch
            ));
            let _ = Command::new("git")
                .args(["reset"])
                .current_dir(workdir)
                .status();
        } else {
            // Get the tip commit message for the fixup label
            let tip_msg = get_branch_tip_message(workdir, branch);
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
                let reset_status = Command::new("git")
                    .args(["reset", "--hard", "HEAD"])
                    .current_dir(workdir)
                    .status()?;

                if !reset_status.success() {
                    errors.push(format!(
                        "Failed to clean worktree after committing absorbed changes on '{}'",
                        branch
                    ));
                    break;
                }

                absorbed_files.extend(files.iter().cloned());
                println!(
                    "  {} {} file(s) → {}",
                    "✓".green(),
                    files.len(),
                    branch.cyan()
                );
            }
        }

        // Return to original branch -- abort if this fails
        let co_back = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .status()?;

        if !co_back.success() {
            errors.push(format!(
                "Failed to return to '{}'. Repository may be on wrong branch.",
                current
            ));
            break;
        }
    }

    if repo.current_branch()? != current {
        let co_back = Command::new("git")
            .args(["checkout", &current])
            .current_dir(workdir)
            .status()?;

        if !co_back.success() {
            errors.push(format!(
                "Failed to return to '{}'. Repository may be on wrong branch.",
                current
            ));
        }
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
        let _ = Command::new("git")
            .args(["reset", "HEAD", "--", file])
            .current_dir(workdir)
            .status();

        let checkout = Command::new("git")
            .args(["checkout", "HEAD", "--", file])
            .current_dir(workdir)
            .status();

        if checkout.map(|s| !s.success()).unwrap_or(true) {
            let _ = std::fs::remove_file(workdir.join(file));
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
    let branch_by_file = collect_file_attribution(workdir, branch_boundaries)?;

    for file in files {
        match branch_by_file.get(file) {
            Some(branch) => branch_files
                .entry(branch.clone())
                .or_default()
                .push(file.clone()),
            None => unattributed.push(file.clone()),
        }
    }

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

fn collect_branch_tips(
    workdir: &Path,
    branch_boundaries: &[(String, String)],
) -> Result<HashMap<String, String>> {
    let mut branch_tips = HashMap::new();

    for (branch, _) in branch_boundaries {
        let output = Command::new("git")
            .args(["rev-parse", branch])
            .current_dir(workdir)
            .output()?;

        if !output.status.success() {
            bail!("Failed to resolve branch tip for '{}'", branch);
        }

        let tip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        branch_tips.insert(tip, branch.clone());
    }

    Ok(branch_tips)
}

fn next_branch_after(branch: &str, branch_boundaries: &[(String, String)]) -> Option<String> {
    let idx = branch_boundaries
        .iter()
        .position(|(candidate, _)| candidate == branch)?;
    branch_boundaries
        .get(idx + 1)
        .map(|(next_branch, _)| next_branch.clone())
}

fn collect_file_attribution(
    workdir: &Path,
    branch_boundaries: &[(String, String)],
) -> Result<HashMap<String, String>> {
    let mut branch_by_file = HashMap::new();
    let Some((first_branch, base)) = branch_boundaries.first() else {
        return Ok(branch_by_file);
    };
    let top_branch = branch_boundaries
        .last()
        .map(|(branch, _)| branch.as_str())
        .unwrap_or(base);
    let branch_tips = collect_branch_tips(workdir, branch_boundaries)?;
    let mut current_branch = first_branch.clone();
    let mut branch_after_current_commit: Option<String> = None;

    let output = Command::new("git")
        .args([
            "log",
            "--format=commit:%H",
            "--name-only",
            "--reverse",
            "--ancestry-path",
            &format!("{}..{}", base, top_branch),
        ])
        .current_dir(workdir)
        .output()?;

    if !output.status.success() {
        bail!("Failed to collect file attribution for absorb");
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.is_empty() {
            continue;
        }

        if let Some(commit) = line.strip_prefix("commit:") {
            if let Some(next_branch) = branch_after_current_commit.take() {
                current_branch = next_branch;
            }
            if let Some(tip_branch) = branch_tips.get(commit) {
                branch_after_current_commit = next_branch_after(tip_branch, branch_boundaries);
            }
            continue;
        }

        branch_by_file.insert(line.to_string(), current_branch.clone());
    }

    Ok(branch_by_file)
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
