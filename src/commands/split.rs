use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::tui;
use anyhow::{Context, Result};
use colored::Colorize;
use std::io::IsTerminal;
use std::path::Path;
use std::process::Command;

pub fn run(hunk_mode: bool, file_pathspecs: Vec<String>, no_verify: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        anyhow::bail!(
            "Cannot split trunk branch. Create a branch first with {}",
            "stax create".cyan()
        );
    }

    let branch_info = stack.branches.get(&current);
    if branch_info.is_none() {
        anyhow::bail!(
            "Branch '{}' is not tracked. Use {} to track it first.",
            current,
            "stax branch track".cyan()
        );
    }

    let parent = branch_info.and_then(|b| b.parent.as_ref());
    if parent.is_none() {
        anyhow::bail!("Branch '{}' has no parent to split from.", current);
    }

    // Dispatch to file-based split (non-interactive)
    if !file_pathspecs.is_empty() {
        let parent_ref = parent.unwrap().clone();
        return split_by_file(&repo, &current, &parent_ref, &file_pathspecs, no_verify);
    }

    if !hunk_mode {
        let parent_ref = parent.unwrap();
        let commits = repo.commits_between(parent_ref, &current)?;
        if commits.is_empty() {
            anyhow::bail!(
                "No commits to split. Branch '{}' has no commits above '{}'.",
                current,
                parent_ref
            );
        }

        if commits.len() == 1 {
            anyhow::bail!(
                "Only 1 commit on branch '{}'. Need at least 2 commits to split.\n\
                 Tip: Use {} to split by hunk instead.",
                current,
                "stax split --hunk".cyan()
            );
        }
    }

    if !std::io::stdin().is_terminal() {
        anyhow::bail!("Split requires an interactive terminal.");
    }

    if hunk_mode {
        drop(repo);
        return tui::split_hunk::run(no_verify);
    }

    tui::split::run()
}

/// Split by extracting file-level changes into a new parent branch.
///
/// Strategy:
///   1. Compute the aggregate diff `parent..current` restricted to the pathspecs.
///   2. Create a new branch at parent's tip.
///   3. Apply the diff there and commit.
///   4. Reparent the current branch onto the new one.
///   5. On the current branch, revert the extracted files to the new-branch state
///      (which already contains them) so the current branch no longer carries those changes.
fn split_by_file(
    repo: &GitRepo,
    current: &str,
    parent: &str,
    pathspecs: &[String],
    no_verify: bool,
) -> Result<()> {
    let workdir = repo.workdir()?;

    // Bail early if the working tree is dirty
    if repo.is_dirty()? {
        anyhow::bail!(
            "Working tree has uncommitted changes. Please commit or stash them before splitting."
        );
    }

    // 1. Check that the pathspecs actually match something in the diff
    let diff_files = changed_files_between(workdir, parent, current, pathspecs)?;
    if diff_files.is_empty() {
        anyhow::bail!(
            "No changes match the given pathspec(s) between '{}' and '{}'.\n\
             Files checked: {}",
            parent,
            current,
            pathspecs.join(", ")
        );
    }

    println!(
        "Splitting {} file(s) from '{}' into a new parent branch:",
        diff_files.len().to_string().cyan(),
        current.green()
    );
    for f in &diff_files {
        println!("  {}", f.dimmed());
    }

    // 2. Generate a new branch name
    let new_branch = generate_split_branch_name(current, repo)?;

    // 3. Get the aggregate diff for the matching files
    let diff_output = git_diff_for_paths(workdir, parent, current, pathspecs)?;
    if diff_output.is_empty() {
        anyhow::bail!("Diff is empty for the given pathspecs. Nothing to split.");
    }

    // 4. Create the new branch at parent's tip
    repo.create_branch_at(&new_branch, parent)?;
    repo.checkout(&new_branch)?;

    // 5. Apply the diff on the new branch
    apply_diff(workdir, &diff_output)?;

    // 6. Stage and commit
    stage_files(workdir, &diff_files)?;
    let commit_msg = format!(
        "split: extract {} from {}",
        pathspecs.join(", "),
        current
    );
    commit(workdir, &commit_msg, no_verify)?;

    // 7. Record stax metadata: new branch is child of parent
    let parent_rev = repo.branch_commit(parent)?;
    let meta = BranchMetadata::new(parent, &parent_rev);
    meta.write(repo.inner(), &new_branch)?;

    // 8. Switch back to the original branch
    repo.checkout(current)?;

    // 9. Remove the extracted files from the current branch by restoring them
    //    to the state they have on the new branch (i.e. undo our changes to those files).
    //    We checkout the files from the *parent* so the diff parent..current no longer
    //    includes them, then amend.
    checkout_paths_from_ref(workdir, &new_branch, pathspecs)?;

    // Stage and amend
    stage_files(workdir, &diff_files)?;
    amend_head(workdir, no_verify)?;

    // 10. Update current branch metadata: parent is now the new branch
    let new_branch_rev = repo.branch_commit(&new_branch)?;
    if let Some(mut meta) = BranchMetadata::read(repo.inner(), current)? {
        meta.parent_branch_name = new_branch.clone();
        meta.parent_branch_revision = new_branch_rev;
        meta.write(repo.inner(), current)?;
    }

    println!();
    println!(
        "Created '{}' (stacked on '{}')",
        new_branch.green(),
        parent.blue()
    );
    println!(
        "Reparented '{}' onto '{}'",
        current.green(),
        new_branch.blue()
    );
    println!(
        "{}",
        "Tip: run `stax restack` if descendants need rebasing.".dimmed()
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Helper functions
// ---------------------------------------------------------------------------

/// Return the list of files that changed between `base` and `head` matching pathspecs.
fn changed_files_between(
    workdir: &Path,
    base: &str,
    head: &str,
    pathspecs: &[String],
) -> Result<Vec<String>> {
    let mut args = vec!["diff", "--name-only"];
    let range = format!("{}..{}", base, head);
    args.push(&range);
    args.push("--");
    let pathspec_refs: Vec<&str> = pathspecs.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&pathspec_refs);

    let output = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .output()
        .context("Failed to run git diff --name-only")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git diff --name-only failed: {}", stderr);
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(ToString::to_string)
        .collect())
}

/// Get the raw diff output for the given pathspecs.
fn git_diff_for_paths(
    workdir: &Path,
    base: &str,
    head: &str,
    pathspecs: &[String],
) -> Result<Vec<u8>> {
    let range = format!("{}..{}", base, head);
    let mut args = vec!["diff", &range, "--"];
    let pathspec_refs: Vec<&str> = pathspecs.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&pathspec_refs);

    let output = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .output()
        .context("Failed to run git diff")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git diff failed: {}", stderr);
    }

    Ok(output.stdout)
}

/// Apply a diff via `git apply`.
fn apply_diff(workdir: &Path, diff: &[u8]) -> Result<()> {
    let mut child = Command::new("git")
        .args(["apply", "--index", "-"])
        .current_dir(workdir)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .context("Failed to spawn git apply")?;

    use std::io::Write;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(diff)?;
    }

    let output = child.wait_with_output().context("git apply failed")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git apply --index failed: {}", stderr);
    }

    Ok(())
}

/// Stage specific files.
fn stage_files(workdir: &Path, files: &[String]) -> Result<()> {
    if files.is_empty() {
        return Ok(());
    }
    let status = Command::new("git")
        .arg("add")
        .arg("--")
        .args(files)
        .current_dir(workdir)
        .status()
        .context("Failed to run git add")?;

    if !status.success() {
        anyhow::bail!("git add failed");
    }
    Ok(())
}

/// Commit staged changes.
fn commit(workdir: &Path, message: &str, no_verify: bool) -> Result<()> {
    let mut args = vec!["commit", "-m", message];
    if no_verify {
        args.push("--no-verify");
    }
    let status = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .status()
        .context("Failed to run git commit")?;

    if !status.success() {
        anyhow::bail!("git commit failed");
    }
    Ok(())
}

/// Amend HEAD without changing the commit message.
fn amend_head(workdir: &Path, no_verify: bool) -> Result<()> {
    let mut args = vec!["commit", "--amend", "--no-edit"];
    if no_verify {
        args.push("--no-verify");
    }
    let status = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .status()
        .context("Failed to run git commit --amend")?;

    if !status.success() {
        anyhow::bail!("git commit --amend failed");
    }
    Ok(())
}

/// Checkout specific paths from a given ref.
fn checkout_paths_from_ref(workdir: &Path, refspec: &str, pathspecs: &[String]) -> Result<()> {
    let mut args = vec!["checkout", refspec, "--"];
    let pathspec_refs: Vec<&str> = pathspecs.iter().map(|s| s.as_str()).collect();
    args.extend_from_slice(&pathspec_refs);

    let output = Command::new("git")
        .args(&args)
        .current_dir(workdir)
        .output()
        .context("Failed to run git checkout <ref> -- <paths>")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!("git checkout {} -- <paths> failed: {}", refspec, stderr);
    }
    Ok(())
}

/// Generate a branch name for the split-off branch (e.g. "feature-split-1").
fn generate_split_branch_name(base_name: &str, repo: &GitRepo) -> Result<String> {
    let existing = repo.list_branches()?;
    let stem = format!("{}-split", base_name);
    if !existing.contains(&stem) {
        return Ok(stem);
    }
    for i in 2..1000 {
        let candidate = format!("{}-{}", stem, i);
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "Cannot generate a unique split branch name from '{}'",
        base_name
    );
}
