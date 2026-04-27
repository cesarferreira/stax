use crate::commands::staging::{self, ContinueLabel, StagingAction};
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::{Context, Result};
use colored::Colorize;
use std::process::Command;

enum ModifyTarget {
    Amend,
    CreateFirstCommit { parent: String },
}

/// Amend staged changes into the current branch tip.
/// When files are already staged, only those files are committed.
/// When nothing is staged, offers an interactive menu (stage all, --patch,
/// amend message only, abort). Non-TTY bails with guidance to use `-a`.
/// On a fresh tracked branch, `-m` creates the first branch-local commit safely.
pub fn run(
    message: Option<String>,
    all: bool,
    quiet: bool,
    no_verify: bool,
    restack: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let workdir = repo.workdir()?;
    let current = repo.current_branch()?;

    if !repo.is_dirty()? {
        if !quiet {
            println!("{}", "No changes to amend.".dimmed());
        }
        return Ok(());
    }

    let target = modify_target(&repo, &current)?;

    // True when the index is empty after resolving the menu — i.e. the user
    // picked "amend message only" or exited `git add -p` without staging.
    let mut empty_amend = false;

    if all {
        staging::stage_all(workdir)?;
    } else if staging::is_staging_area_empty(workdir)? {
        match staging::prompt_action(
            workdir,
            ContinueLabel::AmendMessageOnly,
            "Stage files with `git add` first, or use `stax modify -a`.",
        )? {
            StagingAction::All => staging::stage_all(workdir)?,
            StagingAction::Patch => {
                staging::stage_patch(workdir)?;
                if staging::is_staging_area_empty(workdir)? {
                    staging::print_patch_empty_notice();
                    return Ok(());
                }
            }
            StagingAction::Continue => {
                // Amend message only — leave the index empty and proceed.
                empty_amend = true;
            }
            StagingAction::Abort => {
                println!(
                    "{}",
                    "Aborted. Stage files with `git add` first, or use `stax modify -a`.".dimmed()
                );
                return Ok(());
            }
        }
    }
    // else: index already has staged changes — proceed with them as-is.

    match target {
        ModifyTarget::Amend => {
            let mut amend_args = vec!["commit", "--amend"];

            if no_verify {
                amend_args.push("--no-verify");
            }

            // Empty message-only amend: if the user also passed -m we pass it
            // through; otherwise open the editor with the existing message.
            if empty_amend && message.is_none() {
                // No `-m`, no `--no-edit` — fall through to the editor so the
                // user can rewrite the message (the whole point of picking
                // "amend message only").
            } else if let Some(ref msg) = message {
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
                if empty_amend && message.is_none() {
                    println!(
                        "{} {} {}",
                        "Amended".green(),
                        current.cyan(),
                        "(message only)".dimmed()
                    );
                } else if message.is_some() {
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

            if empty_amend {
                anyhow::bail!(
                    "Cannot create the first branch-local commit with an empty index.\n\
                     Stage files with `git add`, re-run with `-a`, or pick a staging option in the menu."
                );
            }

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
            false, // all
            false, // stop_here
            false, // continue
            false, // dry_run
            true,  // yes (skip confirmation)
            quiet,
            false, // auto_stash_pop
            super::restack::SubmitAfterRestack::No,
        )?;
    } else if !quiet && config.ui.tips {
        println!(
            "{}",
            "Hint: Run `st restack` to update child branches, or `st ss` to submit.".dimmed()
        );
    }

    Ok(())
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
