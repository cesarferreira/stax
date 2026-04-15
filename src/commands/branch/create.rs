use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::remote;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageMode {
    None,
    ExistingOnly,
    All,
}

pub fn run(
    name: Option<String>,
    message: Option<String>,
    from: Option<String>,
    prefix: Option<String>,
    all: bool,
    insert: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let current = repo.current_branch()?;
    let parent_branch = from.unwrap_or_else(|| current.clone());
    let generated_from_message = name.is_none() && message.is_some();

    if repo.branch_commit(&parent_branch).is_err() {
        anyhow::bail!("Branch '{}' does not exist", parent_branch);
    }

    // Get the branch name from either name or message
    // When using -m, the message is used for both branch name and commit message.
    // `stax create -m` respects already-staged changes. When nothing is staged
    // it prompts interactively (or bails in non-TTY). Use -a/--all to skip the prompt.
    // When neither name nor message is provided, launch interactive wizard.
    let (input, commit_message, stage_mode) = match (&name, &message) {
        (Some(n), _) => (
            n.clone(),
            None,
            if all { StageMode::All } else { StageMode::None },
        ),
        (None, Some(m)) => (
            m.clone(),
            Some(m.clone()),
            if all {
                StageMode::All
            } else {
                StageMode::ExistingOnly
            },
        ),
        (None, None) => {
            // Check if we're in an interactive terminal
            if !Term::stderr().is_term() {
                bail!(
                    "Branch name required. Use: stax create <name> or stax create -m \"message\""
                );
            }
            // Launch interactive wizard
            let (wizard_name, wizard_msg, wizard_stage_all) =
                run_wizard(repo.workdir()?, &parent_branch)?;
            (
                wizard_name,
                wizard_msg,
                if wizard_stage_all {
                    StageMode::All
                } else {
                    StageMode::None
                },
            )
        }
    };

    // Format the branch name according to config
    let branch_name = match prefix.as_deref() {
        Some(_) => config.format_branch_name_with_prefix_override(&input, prefix.as_deref()),
        None => config.format_branch_name(&input),
    };
    let existing_branches = repo.list_branches()?;
    let branch_name =
        resolve_branch_name_conflicts(&branch_name, &existing_branches, generated_from_message)?;

    // Before creating the branch, check if we need to prompt about staging.
    // Doing this early means declining is a clean no-op (no orphaned branch).
    let needs_stage_all = if stage_mode == StageMode::ExistingOnly {
        let workdir = repo.workdir()?;
        if is_staging_area_empty(workdir) && has_uncommitted_changes(workdir) {
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
                        "Aborted. Stage files with `git add` first, or use `stax create -a -m \"message\"`."
                            .dimmed()
                    );
                    return Ok(());
                }
                true
            } else {
                bail!(
                    "No files staged. Stage files with `git add` first, or use `stax create -a -m \"message\"`."
                );
            }
        } else {
            false
        }
    } else {
        false
    };

    // Commit-first path: when the user supplied -m and the new branch stacks on
    // the current branch, run the commit on the current branch BEFORE creating
    // or switching to the new branch. If pre-commit hooks fail (or the user
    // hits Ctrl+C) we exit with no refs touched: no orphan branch, no name
    // drift to `-2`/`-3`, and the user stays on their original branch with
    // their working tree preserved. Only on a successful commit do we split
    // it off to the new branch and move the parent ref back.
    if let Some(msg) = commit_message.as_deref() {
        if parent_branch == current {
            return run_commit_first(
                &repo,
                &config,
                &current,
                &branch_name,
                msg,
                stage_mode,
                needs_stage_all,
                insert,
            );
        }
    }

    // Create the branch
    if parent_branch == current {
        repo.create_branch(&branch_name)?;
    } else {
        repo.create_branch_at(&branch_name, &parent_branch)?;
    }

    // Track it with current branch as parent
    let parent_rev = repo.branch_commit(&parent_branch)?;
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    if let Err(e) = meta.write(repo.inner(), &branch_name) {
        rollback_create(&repo, &current, &branch_name);
        return Err(e);
    }

    // If --insert, reparent children of the parent branch to the new branch
    if insert {
        let stack = Stack::load(&repo)?;
        if let Some(parent_info) = stack.branches.get(&parent_branch) {
            let children: Vec<String> = parent_info
                .children
                .iter()
                .filter(|c| *c != &branch_name)
                .cloned()
                .collect();

            if !children.is_empty() {
                let new_parent_rev = repo.branch_commit(&branch_name)?;
                for child in &children {
                    if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
                        let updated = BranchMetadata {
                            parent_branch_name: branch_name.clone(),
                            parent_branch_revision: new_parent_rev.clone(),
                            ..child_meta
                        };
                        updated.write(repo.inner(), child)?;
                    }
                }

                println!(
                    "Reparented {} child branch(es) to '{}'",
                    children.len(),
                    branch_name.green()
                );
                for child in &children {
                    println!("  {} -> {}", child.cyan(), branch_name.green());
                }
                println!(
                    "{}",
                    "Run `stax restack --all` to rebase the reparented branches.".yellow()
                );
            }
        }
    }

    // Checkout the new branch
    if let Err(e) = repo.checkout(&branch_name) {
        rollback_create(&repo, &current, &branch_name);
        return Err(e);
    }

    if let Ok(remote_branches) = remote::get_remote_branches(repo.workdir()?, config.remote_name())
    {
        if !remote_branches.contains(&parent_branch) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }

    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        parent_branch.blue()
    );

    // Stage/commit behavior:
    // - StageMode::All / needs_stage_all => run `git add -A`
    // - StageMode::ExistingOnly (files already staged) => keep current index
    // - StageMode::None => no staging/committing
    if stage_mode != StageMode::None {
        let workdir = repo.workdir()?;

        if stage_mode == StageMode::All || needs_stage_all {
            if let Err(e) = stage_all(workdir) {
                rollback_create(&repo, &current, &branch_name);
                return Err(e);
            }
        }

        // Only commit if -m was provided
        if let Some(msg) = commit_message {
            // Check if there are staged changes to commit
            let diff_output = Command::new("git")
                .args(["diff", "--cached", "--quiet"])
                .current_dir(workdir)
                .status();

            let diff_output = match diff_output {
                Ok(status) => status,
                Err(e) => {
                    rollback_create(&repo, &current, &branch_name);
                    return Err(e.into());
                }
            };

            if !diff_output.success() {
                // There are staged changes, commit them
                let commit_status = Command::new("git")
                    .args(["commit", "-m", &msg])
                    .current_dir(workdir)
                    .status();

                let commit_status = match commit_status {
                    Ok(status) => status,
                    Err(e) => {
                        rollback_create(&repo, &current, &branch_name);
                        return Err(e.into());
                    }
                };

                if !commit_status.success() {
                    rollback_create(&repo, &current, &branch_name);
                    bail!(
                        "Commit failed (pre-commit hook or other error). \
                         Branch rolled back. Fix the issue and retry."
                    );
                }

                println!("Committed: {}", msg.cyan());
            } else {
                println!("{}", "No changes to commit".dimmed());
            }
        } else if stage_mode == StageMode::All {
            println!("{}", "Changes staged".dimmed());
        }
    }

    if config.ui.tips {
        println!(
            "{}",
            "Hint: Run `st ss` to submit, or add changes with `st modify -a -m \"message\"`"
                .dimmed()
        );
    }

    Ok(())
}

/// Best-effort rollback: unstage changes, checkout the original branch,
/// delete the new branch and its metadata.
/// Errors during rollback are intentionally ignored (matching the pattern in split_hunk/app.rs).
fn rollback_create(repo: &GitRepo, original_branch: &str, new_branch: &str) {
    if let Ok(workdir) = repo.workdir() {
        // Reset index first so staged changes from stage_all don't block checkout
        // or leak onto the original branch. This preserves working tree files.
        let _ = Command::new("git")
            .args(["reset"])
            .current_dir(workdir)
            .status();
        let _ = Command::new("git")
            .args(["checkout", original_branch])
            .current_dir(workdir)
            .status();
    }
    let _ = repo.delete_branch(new_branch, true);
    let _ = BranchMetadata::delete(repo.inner(), new_branch);
}

/// Graphite-style commit-first flow: commit on the current branch, then split
/// the new commit off to a new branch and move the current branch ref back.
///
/// The key property: nothing observable changes until `git commit` returns
/// successfully. If pre-commit hooks reject the commit, or the user hits
/// Ctrl+C during the commit, no branch is created, no metadata is written,
/// and HEAD is untouched — so retrying the exact same command is a clean
/// operation that does not drift into `mybranch-2`, `mybranch-3`, etc.
///
/// Precondition: `parent_branch == current` (i.e. no explicit `--from`).
#[allow(clippy::too_many_arguments)]
fn run_commit_first(
    repo: &GitRepo,
    config: &Config,
    current: &str,
    branch_name: &str,
    message: &str,
    stage_mode: StageMode,
    needs_stage_all: bool,
    insert: bool,
) -> Result<()> {
    let workdir = repo.workdir()?;

    // Stage (if requested) BEFORE the commit so hooks see the final tree.
    if stage_mode == StageMode::All || needs_stage_all {
        stage_all(workdir)?;
    }

    // If nothing is staged by the time we reach here, there is no commit to
    // make. Fall back to creating an empty branch (mirrors the "No changes to
    // commit" path from the branch-first flow).
    if is_staging_area_empty(workdir) {
        repo.create_branch(branch_name)?;
        let parent_rev = repo.branch_commit(current)?;
        let meta = BranchMetadata::new(current, &parent_rev);
        if let Err(e) = meta.write(repo.inner(), branch_name) {
            rollback_create(repo, current, branch_name);
            return Err(e);
        }
        if insert {
            apply_insert_reparenting(repo, current, branch_name)?;
        }
        if let Err(e) = repo.checkout(branch_name) {
            rollback_create(repo, current, branch_name);
            return Err(e);
        }
        print_remote_parent_warning(repo, config, current);
        println!(
            "Created and switched to branch '{}' (stacked on {})",
            branch_name.green(),
            current.blue()
        );
        println!("{}", "No changes to commit".dimmed());
        print_tips(config);
        return Ok(());
    }

    // Capture the pre-commit SHA so we can move the current branch back to it
    // after splitting the new commit off.
    let old_parent_sha = repo.branch_commit(current)?;

    // Run the commit on the current branch. `--quiet` suppresses git's
    // "[<branch> <sha>] <msg>" summary that would otherwise show the commit
    // landing on the original branch — we move it off a few calls later and
    // print our own summary. Pre-commit hook output is not suppressed by -q.
    let commit_status = Command::new("git")
        .args(["commit", "--quiet", "-m", message])
        .current_dir(workdir)
        .status()
        .context("Failed to run git commit")?;

    if !commit_status.success() {
        bail!(
            "Commit failed (pre-commit hook or other error). \
             No branch was created — fix the issue and retry with the same command."
        );
    }

    // From here on the commit exists on the current branch. Any failure must
    // undo the commit with a soft reset so the user's working tree and staged
    // changes are preserved.
    let new_sha = repo.branch_commit(current)?;

    if let Err(e) = repo.create_branch_at_commit(branch_name, &new_sha) {
        rollback_after_commit(workdir, &old_parent_sha, None, repo);
        return Err(e);
    }

    let meta = BranchMetadata::new(current, &old_parent_sha);
    if let Err(e) = meta.write(repo.inner(), branch_name) {
        rollback_after_commit(workdir, &old_parent_sha, Some(branch_name), repo);
        return Err(e);
    }

    if insert {
        if let Err(e) = apply_insert_reparenting(repo, current, branch_name) {
            rollback_after_commit(workdir, &old_parent_sha, Some(branch_name), repo);
            return Err(e);
        }
    }

    // Move the current branch ref back to the pre-commit SHA. The new commit
    // now lives only on `branch_name`.
    let current_ref = format!("refs/heads/{}", current);
    if let Err(e) = repo.update_ref(&current_ref, &old_parent_sha) {
        rollback_after_commit(workdir, &old_parent_sha, Some(branch_name), repo);
        return Err(e);
    }

    // Switch to the new branch. HEAD still points at `current` (now at the old
    // SHA) while the working tree matches the new commit; `git checkout` moves
    // HEAD without touching the working tree since it's already correct. If
    // this fails there's nothing to undo — the commit lives on the new branch,
    // `current` is reset — the user just needs `git checkout <new>` manually.
    repo.checkout(branch_name)?;

    print_remote_parent_warning(repo, config, current);
    println!(
        "Created and switched to branch '{}' (stacked on {})",
        branch_name.green(),
        current.blue()
    );
    println!("Committed: {}", message.cyan());
    print_tips(config);

    Ok(())
}

/// Undo the partial state left by `run_commit_first` when a step after
/// `git commit` (branch creation, metadata write, ref update) fails.
///
/// `--soft` keeps the working tree and index exactly as git left them after
/// the successful commit, so the user can retry without re-staging.
fn rollback_after_commit(workdir: &Path, old_sha: &str, new_branch: Option<&str>, repo: &GitRepo) {
    if let Some(name) = new_branch {
        let _ = BranchMetadata::delete(repo.inner(), name);
        let _ = repo.delete_branch(name, true);
    }
    let _ = Command::new("git")
        .args(["reset", "--soft", old_sha])
        .current_dir(workdir)
        .status();
}

/// Reparent children of `parent_branch` onto `new_branch` and print the usual
/// `--insert` summary. Extracted from the branch-first path so both flows
/// share the same behaviour.
fn apply_insert_reparenting(repo: &GitRepo, parent_branch: &str, new_branch: &str) -> Result<()> {
    let stack = Stack::load(repo)?;
    let Some(parent_info) = stack.branches.get(parent_branch) else {
        return Ok(());
    };
    let children: Vec<String> = parent_info
        .children
        .iter()
        .filter(|c| c.as_str() != new_branch)
        .cloned()
        .collect();

    if children.is_empty() {
        return Ok(());
    }

    let new_parent_rev = repo.branch_commit(new_branch)?;
    for child in &children {
        if let Some(child_meta) = BranchMetadata::read(repo.inner(), child)? {
            let updated = BranchMetadata {
                parent_branch_name: new_branch.to_string(),
                parent_branch_revision: new_parent_rev.clone(),
                ..child_meta
            };
            updated.write(repo.inner(), child)?;
        }
    }

    println!(
        "Reparented {} child branch(es) to '{}'",
        children.len(),
        new_branch.green()
    );
    for child in &children {
        println!("  {} -> {}", child.cyan(), new_branch.green());
    }
    println!(
        "{}",
        "Run `stax restack --all` to rebase the reparented branches.".yellow()
    );

    Ok(())
}

fn print_remote_parent_warning(repo: &GitRepo, config: &Config, parent_branch: &str) {
    let Ok(workdir) = repo.workdir() else {
        return;
    };
    if let Ok(remote_branches) = remote::get_remote_branches(workdir, config.remote_name()) {
        if !remote_branches.contains(&parent_branch.to_string()) {
            println!(
                "{}",
                format!(
                    "Warning: parent '{}' is not on remote '{}'.",
                    parent_branch,
                    config.remote_name()
                )
                .yellow()
            );
        }
    }
}

fn print_tips(config: &Config) {
    if config.ui.tips {
        println!(
            "{}",
            "Hint: Run `st ss` to submit, or add changes with `st modify -a -m \"message\"`"
                .dimmed()
        );
    }
}

#[derive(Clone, Copy)]
enum BranchNameConflict<'a> {
    Exact(&'a str),
    ExistingIsAncestor(&'a str),
    ExistingIsDescendant(&'a str),
}

fn resolve_branch_name_conflicts(
    branch_name: &str,
    existing_branches: &[String],
    generated_from_message: bool,
) -> Result<String> {
    match detect_branch_name_conflict(branch_name, existing_branches) {
        None => Ok(branch_name.to_string()),
        Some(BranchNameConflict::Exact(_) | BranchNameConflict::ExistingIsDescendant(_))
            if generated_from_message =>
        {
            for suffix in 2..1000 {
                let candidate = append_branch_suffix(branch_name, suffix);
                if detect_branch_name_conflict(&candidate, existing_branches).is_none() {
                    return Ok(candidate);
                }
            }

            bail!(
                "Cannot create a unique branch name from '{}'. Too many similarly named branches already exist.",
                branch_name
            );
        }
        Some(conflict) => bail!("{}", branch_name_conflict_message(branch_name, conflict)),
    }
}

fn detect_branch_name_conflict<'a>(
    branch_name: &str,
    existing_branches: &'a [String],
) -> Option<BranchNameConflict<'a>> {
    for existing in existing_branches {
        if branch_name == existing {
            return Some(BranchNameConflict::Exact(existing));
        }

        if branch_name.starts_with(&format!("{}/", existing)) {
            return Some(BranchNameConflict::ExistingIsAncestor(existing));
        }

        if existing.starts_with(&format!("{}/", branch_name)) {
            return Some(BranchNameConflict::ExistingIsDescendant(existing));
        }
    }

    None
}

fn branch_name_conflict_message(branch_name: &str, conflict: BranchNameConflict<'_>) -> String {
    match conflict {
        BranchNameConflict::Exact(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Use `st checkout {}` or choose a different name.",
            branch_name, existing, existing
        ),
        BranchNameConflict::ExistingIsAncestor(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Git doesn't allow a branch and its sub-path to coexist.\n\
             Either delete '{}' first, or use a different name like '{}-ui'.",
            branch_name, existing, existing, existing
        ),
        BranchNameConflict::ExistingIsDescendant(existing) => format!(
            "Cannot create '{}': branch '{}' already exists.\n\
             Git doesn't allow a branch and its sub-path to coexist.\n\
             Either delete '{}' first, or use a different name.",
            branch_name, existing, existing
        ),
    }
}

fn append_branch_suffix(branch_name: &str, suffix: usize) -> String {
    match branch_name.rsplit_once('/') {
        Some((prefix, leaf)) => format!("{}/{}-{}", prefix, leaf, suffix),
        None => format!("{}-{}", branch_name, suffix),
    }
}

/// Interactive wizard for branch creation when no arguments provided
fn run_wizard(workdir: &Path, parent_branch: &str) -> Result<(String, Option<String>, bool)> {
    // Show header
    println!();
    println!("╭─ Create Stacked Branch ─────────────────────────────╮");
    println!(
        "│ Parent: {:<43} │",
        format!("{} (current branch)", parent_branch.cyan())
    );
    println!("╰─────────────────────────────────────────────────────╯");
    println!();

    // 1. Branch name prompt (required)
    let name: String = Input::with_theme(&ColorfulTheme::default())
        .with_prompt("Branch name")
        .interact_text()?;

    if name.trim().is_empty() {
        bail!("Branch name cannot be empty");
    }

    // 2. Check for uncommitted changes
    let has_changes = has_uncommitted_changes(workdir);
    let change_count = count_uncommitted_changes(workdir);

    let (should_stage, commit_message) = if has_changes {
        println!();

        // Show staging options with change count
        let stage_label = if change_count > 0 {
            format!("Stage all changes ({} files modified)", change_count)
        } else {
            "Stage all changes".to_string()
        };

        let options = vec![stage_label.as_str(), "Empty branch (no changes)"];

        let choice = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("What to include")
            .items(&options)
            .default(0)
            .interact()?;

        let stage = choice == 0;

        // 3. Optional commit message (only if staging)
        let msg = if stage {
            println!();
            let m: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Commit message (Enter to skip)")
                .allow_empty(true)
                .interact_text()?;
            if m.is_empty() {
                None
            } else {
                Some(m)
            }
        } else {
            None
        };

        (stage, msg)
    } else {
        (false, None)
    };

    println!();
    Ok((name, commit_message, should_stage))
}

/// Run `git add -A` to stage all changes (tracked, modified, untracked).
fn stage_all(workdir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workdir)
        .status()?;
    if !status.success() {
        bail!("Failed to stage changes");
    }
    Ok(())
}

/// Returns true when the staging area has no changes relative to HEAD.
fn is_staging_area_empty(workdir: &Path) -> bool {
    Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(workdir)
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

/// Check if there are uncommitted changes in the working directory
fn has_uncommitted_changes(workdir: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Count the number of files with uncommitted changes
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
