use crate::config::Config;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::remote;
use anyhow::{bail, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, Input, Select};
use std::path::Path;
use std::process::Command;

pub fn run(
    name: Option<String>,
    message: Option<String>,
    from: Option<String>,
    prefix: Option<String>,
    all: bool,
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
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum StageMode {
        None,
        ExistingOnly,
        All,
    }

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

    // Create the branch
    if parent_branch == current {
        repo.create_branch(&branch_name)?;
    } else {
        repo.create_branch_at(&branch_name, &parent_branch)?;
    }

    // Track it with current branch as parent
    let parent_rev = repo.branch_commit(&parent_branch)?;
    let meta = BranchMetadata::new(&parent_branch, &parent_rev);
    meta.write(repo.inner(), &branch_name)?;

    // Checkout the new branch
    repo.checkout(&branch_name)?;

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
    // - StageMode::All => explicit stage all (`-a` or wizard choice)
    // - StageMode::ExistingOnly => respect index; prompt when empty (used by `-m` default)
    // - StageMode::None => no staging/committing changes
    if stage_mode != StageMode::None {
        let workdir = repo.workdir()?;

        if stage_mode == StageMode::All {
            stage_all(workdir)?;
        } else if stage_mode == StageMode::ExistingOnly && is_staging_area_empty(workdir) {
            // Nothing staged — prompt interactively (like `stax modify`)
            if has_uncommitted_changes(workdir) {
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

                    if should_stage {
                        stage_all(workdir)?;
                    } else {
                        println!(
                            "{}",
                            "Aborted. Stage files with `git add` first, or use `stax create -a -m \"message\"`."
                                .dimmed()
                        );
                        return Ok(());
                    }
                } else {
                    bail!(
                        "No files staged. Stage files with `git add` first, or use `stax create -a -m \"message\"`."
                    );
                }
            }
        }

        // Only commit if -m was provided
        if let Some(msg) = commit_message {
            // Check if there are staged changes to commit
            let diff_output = Command::new("git")
                .args(["diff", "--cached", "--quiet"])
                .current_dir(workdir)
                .status()?;

            if !diff_output.success() {
                // There are staged changes, commit them
                let commit_status = Command::new("git")
                    .args(["commit", "-m", &msg])
                    .current_dir(workdir)
                    .status()?;

                if !commit_status.success() {
                    bail!("Failed to commit changes");
                }

                println!("Committed: {}", msg.cyan());
            } else {
                println!("{}", "No changes to commit".dimmed());
            }
        } else if stage_mode == StageMode::All {
            println!("{}", "Changes staged".dimmed());
        }
    }

    Ok(())
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
