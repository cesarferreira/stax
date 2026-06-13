use crate::commands::checkout;
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::path::Path;
use std::process::Command;

pub fn run(branch: String, parent: Option<String>, no_checkout: bool, force: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let config = Config::load()?;
    let remote = config.remote_name().to_string();
    let branch = normalize_remote_branch(&branch, &remote)?;
    let trunk = repo.trunk_branch()?;
    let parent_branch = parent.unwrap_or_else(|| trunk.clone());

    if branch == trunk {
        anyhow::bail!(
            "'{}' is the trunk branch and cannot be tracked. Use {} to checkout trunk.",
            branch,
            "stax trunk".cyan()
        );
    }

    if repo.branch_commit(&parent_branch).is_err() {
        anyhow::bail!("Parent branch '{}' does not exist locally.", parent_branch);
    }

    println!(
        "{} {} from {}...",
        "Fetching".blue().bold(),
        branch.cyan(),
        remote.cyan()
    );
    fetch_remote_branch(&workdir, &remote, &branch)?;

    let remote_ref = format!("{}/{}", remote, branch);
    let remote_sha = rev_parse(&workdir, &remote_ref)?;
    let local_exists = local_branch_exists(&workdir, &branch)?;

    if local_exists {
        let local_sha = rev_parse(&workdir, &branch)?;
        if local_sha != remote_sha {
            if !force {
                anyhow::bail!(
                    "Local branch '{}' already exists and differs from '{}'.\n\
                     Re-run with {} to reset the local branch to the remote tip.",
                    branch,
                    remote_ref,
                    "--force".cyan()
                );
            }
            force_update_local_branch(&workdir, &branch, &remote_ref)?;
            println!(
                "{} {} to {}.",
                "Reset".yellow().bold(),
                branch.cyan(),
                remote_ref.cyan()
            );
        }
        set_upstream(&workdir, &branch, &remote_ref)?;
    } else {
        create_tracking_branch(&workdir, &branch, &remote_ref)?;
        println!(
            "{} {} tracking {}.",
            "Created".green().bold(),
            branch.cyan(),
            remote_ref.cyan()
        );
    }

    let repo = GitRepo::open()?;
    write_tracking_metadata(&repo, &branch, &parent_branch, &remote)?;

    if no_checkout {
        println!(
            "{} {} with parent {}.",
            "Tracked".green().bold(),
            branch.cyan(),
            parent_branch.cyan()
        );
    } else {
        checkout::run(Some(branch), None, false, false, None, false)?;
    }

    Ok(())
}

fn normalize_remote_branch(branch: &str, remote: &str) -> Result<String> {
    let trimmed = branch.trim();
    if trimmed.is_empty() {
        anyhow::bail!("Branch name cannot be empty.");
    }

    if let Some(local) = trimmed.strip_prefix(&format!("{remote}/")) {
        if local.is_empty() {
            anyhow::bail!("Branch name cannot be empty.");
        }
        return Ok(local.to_string());
    }

    if trimmed.starts_with("refs/") {
        anyhow::bail!("Pass a branch name, not a full ref: '{}'.", branch);
    }

    Ok(trimmed.to_string())
}

fn write_tracking_metadata(
    repo: &GitRepo,
    branch: &str,
    parent_branch: &str,
    remote: &str,
) -> Result<()> {
    if let Some(existing) = BranchMetadata::read(repo.inner(), branch)? {
        if existing.parent_branch_name != parent_branch {
            anyhow::bail!(
                "Branch '{}' is already tracked with parent '{}'.\n\
                 Use {} to change its parent.",
                branch,
                existing.parent_branch_name,
                "stax branch reparent".cyan()
            );
        }
        if existing.source_remote.as_deref() != Some(remote) {
            let updated = BranchMetadata {
                source_remote: Some(remote.to_string()),
                ..existing
            };
            updated.write(repo.inner(), branch)?;
        }
        return Ok(());
    }

    let parent_rev = repo
        .merge_base(parent_branch, branch)
        .or_else(|_| repo.branch_commit(parent_branch))?;
    let meta = BranchMetadata {
        source_remote: Some(remote.to_string()),
        ..BranchMetadata::new(parent_branch, &parent_rev)
    };
    meta.write(repo.inner(), branch)?;
    Ok(())
}

fn fetch_remote_branch(workdir: &Path, remote: &str, branch: &str) -> Result<()> {
    let refspec = format!("refs/heads/{branch}:refs/remotes/{remote}/{branch}");
    let output = Command::new("git")
        .args(["fetch", "--no-tags", remote, &refspec])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to run git fetch {}", remote))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Remote branch '{}' was not found on '{}'.\n\ngit stderr:\n{}",
        branch,
        remote,
        stderr.trim()
    );
}

fn create_tracking_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--track", branch, remote_ref])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to create local branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to create local tracking branch '{}': {}",
        branch,
        stderr.trim()
    );
}

fn force_update_local_branch(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--force", branch, remote_ref])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to reset local branch '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Failed to reset local branch '{}': {}",
        branch,
        stderr.trim()
    );
}

fn set_upstream(workdir: &Path, branch: &str, remote_ref: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["branch", "--set-upstream-to", remote_ref, branch])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to set upstream for '{}'", branch))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("Failed to set upstream for '{}': {}", branch, stderr.trim());
}

fn local_branch_exists(workdir: &Path, branch: &str) -> Result<bool> {
    let output = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .current_dir(workdir)
        .output()
        .context("Failed to inspect local branches")?;
    Ok(output.status.success())
}

fn rev_parse(workdir: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", rev])
        .current_dir(workdir)
        .output()
        .with_context(|| format!("Failed to resolve '{}'", rev))?;

    if output.status.success() {
        return Ok(String::from_utf8(output.stdout)?.trim().to_string());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!("Failed to resolve '{}': {}", rev, stderr.trim());
}
