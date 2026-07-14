use super::remove::{RemovalMode, retire_worktree};
use super::shared::{
    WorktreeDetails, compute_worktree_details, emit_shell_message, emit_shell_payload,
    run_blocking_hook, spawn_background_hook,
};
use crate::commands::shell_setup;
use crate::config::Config;
use crate::git::GitRepo;
use crate::git::repo::WorktreeInfo;
use anyhow::{Context, Result, anyhow, bail};
use colored::Colorize;
use std::{fs, path::Path};

pub fn run(shell_output: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let worktrees = repo.list_worktrees()?;
    let source = worktrees
        .iter()
        .find(|worktree| worktree.is_current)
        .cloned()
        .context("Could not identify the current worktree")?;
    let main = worktrees
        .iter()
        .find(|worktree| worktree.is_main)
        .cloned()
        .context("Could not identify the main worktree")?;

    if source.is_main {
        bail!("Cannot promote the main worktree.");
    }
    let branch = source
        .branch
        .clone()
        .context("Cannot promote a detached worktree. Check out a branch first.")?;

    ensure_clean_checkout(
        "current linked worktree",
        &compute_worktree_details(&repo, source.clone())?,
    )?;
    ensure_clean_checkout(
        "main worktree",
        &compute_worktree_details(&repo, main.clone())?,
    )?;

    let main_head = repo.head_oid_in(&main.path)?;
    run_blocking_hook(
        config.worktree.hooks.pre_remove.as_deref(),
        &main.path,
        "pre_remove",
    )?;
    repo.switch_detached_in(&source.path, None)?;

    if let Err(error) = repo.switch_branch_in(&main.path, &branch) {
        let rollback = repo.switch_branch_in(&source.path, &branch).err();
        return Err(transaction_error(
            "switch the main worktree",
            error,
            rollback.into_iter(),
        ));
    }

    std::env::set_current_dir(&main.path).with_context(|| {
        format!(
            "Failed to enter the main worktree at '{}'",
            main.path.display()
        )
    })?;
    let retiring_source = WorktreeInfo {
        is_current: false,
        ..source.clone()
    };

    if let Err(error) = retire_worktree(
        &repo,
        &config,
        &retiring_source,
        false,
        RemovalMode::AllowParking,
    ) {
        if source_was_retired(&main, &source) {
            let removed_dangling_git_file = remove_dangling_git_file(&source.path)?;
            eprintln!(
                "Warning: Git reported a removal failure after it had already retired the linked worktree. \
                 Promotion completed in the main worktree.{} Inspect any leftover files at '{}'.",
                if removed_dangling_git_file {
                    " Removed its dangling .git file."
                } else {
                    ""
                },
                source.path.display()
            );
            spawn_background_hook(
                config.worktree.hooks.post_remove.as_deref(),
                &main.path,
                "post_remove",
            )?;
            finish_success(shell_output, &main.path, &branch);
            return Ok(());
        }
        let main_rollback = restore_checkout(&repo, &main, &main_head).err();
        let source_rollback = repo.switch_branch_in(&source.path, &branch).err();
        return Err(transaction_error(
            "retire the linked worktree",
            error,
            [main_rollback, source_rollback].into_iter().flatten(),
        ));
    }

    spawn_background_hook(
        config.worktree.hooks.post_remove.as_deref(),
        &main.path,
        "post_remove",
    )?;
    finish_success(shell_output, &main.path, &branch);
    Ok(())
}

fn source_was_retired(main: &WorktreeInfo, source: &WorktreeInfo) -> bool {
    GitRepo::list_worktrees_in(&main.path)
        .map(|worktrees| {
            worktrees
                .iter()
                .all(|worktree| worktree.path != source.path)
        })
        .unwrap_or(false)
}

fn remove_dangling_git_file(worktree_path: &Path) -> Result<bool> {
    let git_file = worktree_path.join(".git");
    let contents = match fs::read_to_string(&git_file) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("Failed to inspect '{}'", git_file.display()));
        }
    };
    let Some(gitdir) = contents.trim().strip_prefix("gitdir:") else {
        return Ok(false);
    };
    let gitdir = Path::new(gitdir.trim());
    let gitdir = if gitdir.is_absolute() {
        gitdir.to_path_buf()
    } else {
        worktree_path.join(gitdir)
    };
    if gitdir.exists() {
        return Ok(false);
    }
    fs::remove_file(&git_file)
        .with_context(|| format!("Failed to remove dangling '{}'.", git_file.display()))?;
    Ok(true)
}

fn ensure_clean_checkout(label: &str, details: &WorktreeDetails) -> Result<()> {
    let path = details.info.path.display();
    if !details.info.path.exists() || details.info.is_prunable {
        bail!("Cannot promote: the {label} path is unavailable at '{path}'.");
    }
    if details.info.is_locked {
        bail!("Cannot promote: the {label} is locked at '{path}'.");
    }
    if details.dirty {
        bail!(
            "Cannot promote: the {label} has uncommitted changes at '{path}'.\nCommit or stash those changes, then retry `st wt promote`."
        );
    }
    if details.rebase_in_progress {
        bail!("Cannot promote: the {label} has a rebase in progress at '{path}'.");
    }
    if details.merge_in_progress {
        bail!("Cannot promote: the {label} has a merge in progress at '{path}'.");
    }
    if details.has_conflicts {
        bail!("Cannot promote: the {label} has unresolved conflicts at '{path}'.");
    }
    Ok(())
}

fn restore_checkout(repo: &GitRepo, worktree: &WorktreeInfo, original_head: &str) -> Result<()> {
    match worktree.branch.as_deref() {
        Some(branch) => repo.switch_branch_in(&worktree.path, branch),
        None => repo.switch_detached_in(&worktree.path, Some(original_head)),
    }
}

fn transaction_error(
    action: &str,
    error: anyhow::Error,
    rollback_errors: impl Iterator<Item = anyhow::Error>,
) -> anyhow::Error {
    let mut message = format!("Failed to {action}: {error}");
    for rollback_error in rollback_errors {
        message.push_str(&format!("\nRollback also failed: {rollback_error}"));
    }
    anyhow!(message)
}

fn finish_success(shell_output: bool, main_path: &Path, branch: &str) {
    let message = format!("Promoted '{branch}' to the main worktree");
    if shell_output {
        emit_shell_payload(main_path, None);
        emit_shell_message(&message);
        return;
    }

    println!("{}  '{}'", "Promoted".green().bold(), branch.cyan());
    println!("  Main worktree: {}", main_path.display());
    println!();
    println!("{}", "Current shell did not move automatically.".yellow());
    println!("  {}", format!("cd {}", main_path.display()).cyan());
    if !shell_setup::is_installed() {
        println!();
        println!(
            "{}",
            "Tip: add shell integration for automatic cd:".dimmed()
        );
        println!("  {}", "stax setup".cyan());
    }
}
