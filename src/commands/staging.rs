//! Shared staging prompt for `stax modify` and `stax create`.
//!
//! When a command that produces a commit is invoked with no staged changes
//! and an uncommitted working tree, it offers a graphite-style picker:
//!
//! ```text
//! ? No files staged. What would you like to do? ›
//! ❯ Stage all changes (N files modified)
//!   Select changes to commit (--patch)
//!   Continue without staging
//!   Abort
//! ```
//!
//! - Non-TTY callers skip the prompt entirely — see `prompt_action`.
//! - `-a/--all` flags on the caller should bypass this module; it only runs
//!   when the index is empty and `--all` wasn't supplied.

use anyhow::{bail, Context, Result};
use colored::Colorize;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Select};
use std::path::Path;
use std::process::Command;

/// The user's choice from the staging prompt, or the implied action for
/// non-TTY callers (`Abort`, which maps to a bail with an actionable error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingAction {
    /// `git add -A` — stage every tracked/untracked change.
    All,
    /// `git add -p` — interactively select hunks. After this runs, the
    /// caller must re-check whether the index is still empty.
    Patch,
    /// Proceed with the empty index. Command-specific meaning:
    /// - `stax create`: produces an empty branch.
    /// - `stax modify`: amends the current commit with no file changes
    ///   (message-only amend).
    Continue,
    /// User declined — caller should return early as a clean no-op.
    Abort,
}

/// Labels the third menu option ("Continue without staging") differently
/// based on caller context, so the picker reads naturally.
#[derive(Debug, Clone, Copy)]
pub enum ContinueLabel {
    /// "Empty branch (no changes)" — used by `stax create`.
    EmptyBranch,
    /// "Just edit the commit message" — used by `stax modify`.
    AmendMessageOnly,
}

impl ContinueLabel {
    fn as_str(self) -> &'static str {
        match self {
            Self::EmptyBranch => "Empty branch (no changes)",
            Self::AmendMessageOnly => "Just edit the commit message",
        }
    }
}

/// Present the picker when nothing is staged and the working tree has
/// changes. Returns the chosen action. `non_tty_hint` is the actionable
/// message shown when stderr isn't a TTY (e.g. `"Use git add or stax modify -a"`).
pub fn prompt_action(
    workdir: &Path,
    continue_label: ContinueLabel,
    non_tty_hint: &str,
) -> Result<StagingAction> {
    if !Term::stderr().is_term() {
        bail!("No files staged. {}", non_tty_hint);
    }

    let change_count = count_uncommitted_changes(workdir);
    let stage_all_label = if change_count > 0 {
        format!("Stage all changes ({} files modified)", change_count)
    } else {
        "Stage all changes".to_string()
    };

    let options = [
        stage_all_label.as_str(),
        "Select changes to commit (--patch)",
        continue_label.as_str(),
        "Abort",
    ];

    let choice = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("No files staged. What would you like to do?")
        .items(options.as_slice())
        .default(0)
        .interact()?;

    Ok(match choice {
        0 => StagingAction::All,
        1 => StagingAction::Patch,
        2 => StagingAction::Continue,
        _ => StagingAction::Abort,
    })
}

/// Run `git add -A` to stage tracked, modified, and untracked files.
pub fn stage_all(workdir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(workdir)
        .status()
        .context("Failed to run git add -A")?;
    if !status.success() {
        bail!("Failed to stage changes");
    }
    Ok(())
}

/// Run `git add --patch` interactively, inheriting stdio so the user can
/// drive the hunk selector. Returns when git exits; callers should then
/// re-check whether anything was staged via [`is_staging_area_empty`].
pub fn stage_patch(workdir: &Path) -> Result<()> {
    let status = Command::new("git")
        .args(["add", "--patch"])
        .current_dir(workdir)
        .status()
        .context("Failed to run git add --patch")?;
    if !status.success() {
        bail!("git add --patch exited with an error");
    }
    Ok(())
}

/// True when the index has no changes relative to HEAD.
pub fn is_staging_area_empty(workdir: &Path) -> Result<bool> {
    let status = Command::new("git")
        .args(["diff", "--cached", "--quiet"])
        .current_dir(workdir)
        .status()
        .context("Failed to check staged changes")?;
    Ok(status.success())
}

/// Count files with uncommitted changes (staged + unstaged + untracked).
pub fn count_uncommitted_changes(workdir: &Path) -> usize {
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

/// True when the working tree has any uncommitted changes at all.
pub fn has_uncommitted_changes(workdir: &Path) -> bool {
    Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workdir)
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Print the dimmed notice shown when the user picks `--patch` but exits
/// without selecting any hunks (index still empty).
pub fn print_patch_empty_notice() {
    println!(
        "{}",
        "No hunks staged. Aborted — re-run and pick hunks, or stage via `git add` first.".dimmed()
    );
}
