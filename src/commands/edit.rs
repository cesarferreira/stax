use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::{bail, Result};
use colored::Colorize;
use console::Term;
use dialoguer::theme::ColorfulTheme;
use dialoguer::Select;
use std::io::Write;
use std::process::Command;

/// Commit info parsed from `git log`.
struct CommitInfo {
    sha: String,
    message: String,
}

impl CommitInfo {
    fn short_sha(&self) -> &str {
        &self.sha[..7.min(self.sha.len())]
    }
}

/// Actions the user can choose per commit (maps to git rebase -i verbs).
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditAction {
    Pick,
    Reword,
    Squash,
    Fixup,
    Drop,
}

impl EditAction {
    fn label(self) -> &'static str {
        match self {
            Self::Pick => "pick",
            Self::Reword => "reword",
            Self::Squash => "squash",
            Self::Fixup => "fixup",
            Self::Drop => "drop",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Self::Pick => "pick   - keep commit as-is",
            Self::Reword => "reword - change commit message",
            Self::Squash => "squash - combine with previous (keep both messages)",
            Self::Fixup => "fixup  - combine with previous (discard this message)",
            Self::Drop => "drop   - remove commit",
        }
    }
}

pub fn run(yes: bool, no_verify: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        bail!("Cannot edit commits on trunk. Checkout a stacked branch first.");
    }

    if repo.is_dirty()? {
        bail!("Working tree has uncommitted changes. Commit or stash them first.");
    }

    // Find the parent branch boundary
    let meta = BranchMetadata::read(repo.inner(), &current)?
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' is not tracked by stax", current))?;
    let parent = &meta.parent_branch_name;

    // Get commits between parent and HEAD (oldest first)
    let workdir = repo.workdir()?;
    let output = Command::new("git")
        .args([
            "log",
            "--reverse",
            "--format=%H %s",
            &format!("{}..HEAD", parent),
        ])
        .current_dir(workdir)
        .output()?;

    if !output.status.success() {
        bail!(
            "Failed to list commits: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let commits: Vec<CommitInfo> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let (sha, message) = line.split_once(' ').unwrap_or((line, ""));
            CommitInfo {
                sha: sha.to_string(),
                message: message.to_string(),
            }
        })
        .collect();

    if commits.is_empty() {
        println!(
            "{}",
            format!("No commits on '{}' ahead of '{}'.", current, parent).yellow()
        );
        return Ok(());
    }

    if commits.len() == 1 {
        println!(
            "{}",
            format!(
                "Only 1 commit on '{}'. Edit actions: reword, drop.",
                current
            )
            .dimmed()
        );
    }

    // Display commits
    println!(
        "{}",
        format!(
            "Commits on '{}' (oldest first, {} total):",
            current,
            commits.len()
        )
        .bold()
    );
    for (i, c) in commits.iter().enumerate() {
        println!(
            "  {}. {} {}",
            i + 1,
            c.short_sha().yellow(),
            c.message
        );
    }
    println!();

    if !Term::stderr().is_term() {
        bail!("Interactive terminal required for `stax edit`.");
    }

    // Collect actions for each commit
    let mut actions: Vec<EditAction> = vec![EditAction::Pick; commits.len()];

    for (i, commit) in commits.iter().enumerate() {
        let prompt = format!(
            "{} {} {}",
            format!("[{}/{}]", i + 1, commits.len()).dimmed(),
            commit.short_sha().yellow(),
            &commit.message
        );
        println!("{}", prompt);

        // Squash/fixup not available for the first commit (nothing to combine with)
        let available: Vec<EditAction> = if i == 0 {
            vec![EditAction::Pick, EditAction::Reword, EditAction::Drop]
        } else {
            vec![
                EditAction::Pick,
                EditAction::Reword,
                EditAction::Squash,
                EditAction::Fixup,
                EditAction::Drop,
            ]
        };

        let items: Vec<&str> = available.iter().map(|a| a.description()).collect();

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Action")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(idx) = selection else {
            println!("Cancelled.");
            return Ok(());
        };

        actions[i] = available[idx];
    }

    // Check if anything changed from default (all pick)
    if actions.iter().all(|a| *a == EditAction::Pick) {
        println!("{}", "No changes selected. Nothing to do.".yellow());
        return Ok(());
    }

    // Show summary
    println!();
    println!("{}", "Edit plan:".bold());
    let has_reword = actions.iter().any(|a| *a == EditAction::Reword);
    for (i, (commit, action)) in commits.iter().zip(actions.iter()).enumerate() {
        let action_str = match action {
            EditAction::Pick => action.label().dimmed().to_string(),
            EditAction::Drop => action.label().red().to_string(),
            _ => action.label().cyan().to_string(),
        };
        println!(
            "  {}. {} {} {}",
            i + 1,
            action_str,
            commit.short_sha().yellow(),
            &commit.message
        );
    }
    if has_reword {
        println!(
            "{}",
            "  Note: reword will open your editor for each rewarded commit.".dimmed()
        );
    }
    println!();

    // Confirm
    if !yes {
        let proceed = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Apply this edit plan?")
            .items(["Yes, apply", "Cancel"])
            .default(0)
            .interact_opt()?;

        match proceed {
            Some(0) => {}
            _ => {
                println!("Cancelled.");
                return Ok(());
            }
        }
    }

    // Create undo snapshot
    let mut tx = Transaction::begin(OpKind::Edit, &repo, false)?;
    tx.plan_branch(&repo, &current)?;
    tx.snapshot()?;

    // Build the rebase todo list
    let todo: String = commits
        .iter()
        .zip(actions.iter())
        .map(|(c, a)| format!("{} {} {}", a.label(), c.sha, c.message))
        .collect::<Vec<_>>()
        .join("\n");

    // Write todo to a temp file
    let mut tmp = tempfile::NamedTempFile::new()?;
    writeln!(tmp, "{}", todo)?;
    tmp.flush()?;
    let todo_path = tmp.path().to_string_lossy().to_string();

    // Run git rebase -i with GIT_SEQUENCE_EDITOR that replaces the todo.
    // Quote the source path to handle paths with spaces.
    let editor_cmd = format!("cp '{}' \"$1\"", todo_path.replace('\'', "'\\''"));

    let mut rebase_args = vec!["rebase", "-i"];
    if no_verify {
        rebase_args.push("--no-verify");
    }
    rebase_args.push(parent);

    let rebase_status = Command::new("git")
        .args(&rebase_args)
        .env("GIT_SEQUENCE_EDITOR", &editor_cmd)
        .current_dir(workdir)
        .status()?;

    if rebase_status.success() {
        // Update metadata to reflect new parent boundary
        let parent_rev = repo.branch_commit(parent)?;
        let updated = BranchMetadata {
            parent_branch_revision: parent_rev,
            ..meta
        };
        updated.write(repo.inner(), &current)?;

        tx.record_after(&repo, &current)?;
        tx.finish_ok()?;

        println!("{}", "Edit applied successfully.".green());
    } else if repo.rebase_in_progress()? {
        println!(
            "{}",
            "Rebase paused due to conflicts. Resolve them, then run `stax continue`.".yellow()
        );
    } else {
        bail!("Rebase failed. Run `stax abort` to undo.");
    }

    Ok(())
}
