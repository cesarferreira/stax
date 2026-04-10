use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm};
use std::path::Path;
use std::process::Command;

enum ModifyTarget {
    Amend,
    CreateFirstCommit { parent: String },
}

/// Amend staged changes into the current branch tip.
/// When files are already staged, only those files are committed.
/// When nothing is staged, prompts to stage all (or use `-a`).
/// On a fresh tracked branch, `-m` creates the first branch-local commit safely.
pub fn run(message: Option<String>, all: bool, quiet: bool, no_verify: bool, restack: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?;
    let current = repo.current_branch()?;

    // Check if there are any changes at all
    if !repo.is_dirty()? {
        if !quiet {
            println!("{}", "No changes to amend.".dimmed());
        }
        return Ok(());
    }

    let target = modify_target(&repo, &current)?;

    if all {
        // Explicit --all: force-stage everything, even when some files are
        // already selectively staged.
        stage_all(workdir)?;
    } else {
        // Check whether anything is already staged
        let has_staged = !is_staging_area_empty(workdir)?;

        if !has_staged {
            // Nothing staged — prompt interactively, bail otherwise
            if Term::stderr().is_term() {
                let change_count = count_uncommitted_changes(workdir);
                let prompt = if change_count > 0 {
                    format!(
                        "No files staged. Stage all changes ({} files modified)?",
                        change_count
                    )
                } else {
                    "No files staged. Stage all changes?".to_string()
                };

                let should_stage = Confirm::with_theme(&ColorfulTheme::default())
                    .with_prompt(prompt)
                    .default(true)
                    .interact()?;

                if !should_stage {
                    println!(
                        "{}",
                        "Aborted. Stage files with `git add` first, or use `stax modify -a`."
                            .dimmed()
                    );
                    return Ok(());
                }
            } else {
                anyhow::bail!(
                    "No files staged. Stage files with `git add` first, or use `stax modify -a`."
                );
            }

            stage_all(workdir)?;
        }
        // else: staged changes exist — proceed with them as-is
    }

    match target {
        ModifyTarget::Amend => {
            let mut amend_args = vec!["commit", "--amend"];

            if no_verify {
                amend_args.push("--no-verify");
            }

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
        }
        ModifyTarget::CreateFirstCommit { parent } => {
            let commit_message = message.as_deref().ok_or_else(|| {
                anyhow::anyhow!(
                    "`stax modify` has nothing to amend on '{}'.\n\
                     Branch '{}' has no commits ahead of '{}', so amending would rewrite an inherited parent commit.\n\
                     Re-run with `-m <message>` to create the first branch-local commit.",
                    current,
                    current,
                    parent,
                )
            })?;

            let mut commit_args = vec!["commit", "-m", commit_message];
            if no_verify {
                commit_args.push("--no-verify");
            }

            let commit_status = Command::new("git")
                .args(&commit_args)
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
    }

    if restack {
        if !quiet {
            println!();
        }
        super::restack::run(
            false,  // all
            false,  // stop_here
            false,  // continue
            false,  // dry_run
            true,   // yes (skip confirmation)
            quiet,
            false,  // auto_stash_pop
            super::restack::SubmitAfterRestack::No,
        )?;
    }

    Ok(())
}

/// Run `git add -A` to stage all changes (tracked, modified, untracked).
fn stage_all(workdir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workdir)
        .status()
        .context("Failed to stage changes")?;

    if !status.success() {
        anyhow::bail!("Failed to stage changes");
    }
    Ok(())
}

/// Returns true when the staging area has no changes relative to HEAD.
fn is_staging_area_empty(workdir: &Path) -> Result<bool> {
    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(workdir)
        .status()
        .context("Failed to check staged changes")?;
    Ok(status.success())
}

/// Count files with uncommitted changes (staged + unstaged + untracked).
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

fn modify_target(repo: &GitRepo, current: &str) -> Result<ModifyTarget> {
    let Some(meta) = BranchMetadata::read(repo.inner(), current)? else {
        return Ok(ModifyTarget::Amend);
    };

    let parent = meta.parent_branch_name.trim();
    if parent.is_empty() || parent == current {
        return Ok(ModifyTarget::Amend);
    }

    let head = repo.branch_commit(current)?;
    let stored_parent_boundary = meta.parent_branch_revision.trim();
    if !stored_parent_boundary.is_empty() && head == stored_parent_boundary {
        return Ok(ModifyTarget::CreateFirstCommit {
            parent: parent.to_string(),
        });
    }

    let (ahead, _) = match repo.commits_ahead_behind(parent, current) {
        Ok(counts) => counts,
        Err(_) => return Ok(ModifyTarget::Amend),
    };

    if ahead > 0 {
        return Ok(ModifyTarget::Amend);
    }

    Ok(ModifyTarget::CreateFirstCommit {
        parent: parent.to_string(),
    })
}
