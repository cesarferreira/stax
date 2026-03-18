use crate::commands::restack;
use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, OpReceipt, OpStatus};
use anyhow::Result;
use colored::Colorize;

pub(crate) fn continue_rebase_and_update_metadata(repo: &GitRepo) -> Result<RebaseResult> {
    match repo.rebase_continue()? {
        RebaseResult::Success => {
            // Update metadata for current branch
            let current = repo.current_branch()?;
            if let Some(meta) = BranchMetadata::read(repo.inner(), &current)? {
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), &current)?;
            }
            Ok(RebaseResult::Success)
        }
        RebaseResult::Conflict => Ok(RebaseResult::Conflict),
    }
}

fn latest_failed_restack(repo: &GitRepo) -> Result<Option<OpReceipt>> {
    let git_dir = repo.git_dir()?;
    let current = repo.current_branch()?;
    let workdir = repo.workdir()?.to_string_lossy().to_string();

    Ok(OpReceipt::load_latest(git_dir)?.filter(|receipt| {
        receipt.kind == OpKind::Restack
            && receipt.status == OpStatus::Failed
            && receipt.repo_workdir == workdir
            && receipt
                .error
                .as_ref()
                .and_then(|error| error.failed_branch.as_deref())
                == Some(current.as_str())
    }))
}

fn continue_impl(repo: &GitRepo, resume_restack: bool) -> Result<()> {
    if !repo.rebase_in_progress()? {
        println!("{}", "No rebase in progress.".yellow());
        return Ok(());
    }

    println!("Continuing rebase...");

    match continue_rebase_and_update_metadata(repo)? {
        RebaseResult::Success => {
            println!("{}", "✓ Rebase completed successfully!".green());

            if resume_restack {
                if let Some(receipt) = latest_failed_restack(repo)? {
                    println!();
                    println!("{}", "Continuing restack...".bold());
                    restack::resume_after_rebase(
                        receipt.auto_stash_pop,
                        Some(receipt.head_branch_before.clone()),
                    )?;
                    return Ok(());
                }
            }

            let config = Config::load().unwrap_or_default();
            if config.ui.tips {
                println!();
                println!(
                    "You may want to run {} to continue restacking.",
                    "stax rs".cyan()
                );
            }
        }
        RebaseResult::Conflict => {
            println!("{}", "More conflicts to resolve.".yellow());
            let config = Config::load().unwrap_or_default();
            if config.ui.tips {
                println!();
                println!(
                    "Resolve the conflicts and run {} again.",
                    "stax continue".cyan()
                );
            }
        }
    }

    Ok(())
}

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    continue_impl(&repo, false)
}

pub fn run_and_resume_restack() -> Result<()> {
    let repo = GitRepo::open()?;
    continue_impl(&repo, true)
}
