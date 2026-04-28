//! Fold the current branch into its parent (`gt fold` parity).
//!
//! Default mode collapses the *current* branch into the *parent* branch:
//! the parent ref is force-updated to the current branch's tip (commits are
//! preserved, not squashed), the current branch ref is deleted, descendants
//! of the current branch are re-parented onto the parent, and any
//! "siblings" (other children of the parent) are rebased onto the new
//! parent tip.
//!
//! `--keep` mode keeps the *current* branch's name as the surviving ref;
//! the parent ref is deleted instead, and the current branch's metadata is
//! updated to point at the grandparent.
//!
//! In both modes the surviving ref ends up at the same SHA (the current
//! branch's tip), so descendants of the current branch only need a metadata
//! re-parent. Siblings need an actual rebase because their previous base
//! (the old parent tip) is no longer the tip of any tracked branch.

use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::{bail, Context, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

pub fn run(keep_branch: bool, skip_confirm: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        bail!("Cannot fold trunk. Checkout a stacked branch first.");
    }

    if repo.is_dirty()? {
        bail!("Working tree has uncommitted changes. Commit or stash them before folding.");
    }

    if repo.rebase_in_progress()? {
        bail!("A rebase is in progress. Run `stax continue` or `stax abort` first.");
    }

    let current_meta = BranchMetadata::read(repo.inner(), &current)?.with_context(|| {
        format!(
            "Branch '{}' is not tracked. Run `stax branch track` first.",
            current
        )
    })?;
    let parent = current_meta.parent_branch_name.clone();

    if parent == stack.trunk {
        println!(
            "{}",
            "Cannot fold into trunk. Use `stax submit` to merge the branch into trunk via a PR."
                .yellow()
        );
        return Ok(());
    }

    let parent_meta = BranchMetadata::read(repo.inner(), &parent)?.with_context(|| {
        format!(
            "Parent branch '{}' is not tracked, so its parent cannot be determined.",
            parent
        )
    })?;
    let grandparent = parent_meta.parent_branch_name.clone();
    let grandparent_revision = parent_meta.parent_branch_revision.clone();
    // Capture PR numbers up front so the orphaned-PR hint at the end of the
    // function doesn't need to outlive the metadata moves below.
    let current_pr = current_meta.pr_info.as_ref().map(|p| p.number);
    let parent_pr = parent_meta.pr_info.as_ref().map(|p| p.number);

    let kids: Vec<String> = stack.children(&current);
    let siblings: Vec<String> = stack
        .children(&parent)
        .into_iter()
        .filter(|name| name != &current)
        .collect();

    let current_tip = repo
        .branch_commit(&current)
        .with_context(|| format!("Could not resolve commit for '{}'", current))?;
    let old_parent_tip = repo
        .branch_commit(&parent)
        .with_context(|| format!("Could not resolve commit for '{}'", parent))?;

    let (commits_folded_in, _) = repo
        .commits_ahead_behind(&parent, &current)
        .unwrap_or((0, 0));

    if commits_folded_in == 0 && kids.is_empty() && siblings.is_empty() {
        println!(
            "{}",
            "Nothing to fold: branch has no commits ahead of parent and no descendants/siblings."
                .yellow()
        );
        return Ok(());
    }

    let (survivor, discarded) = if keep_branch {
        (current.clone(), parent.clone())
    } else {
        (parent.clone(), current.clone())
    };

    println!(
        "Fold plan: collapse '{}' into '{}'",
        current.cyan(),
        parent.cyan()
    );
    println!(
        "  {} {} commit(s) preserved on '{}'",
        "▸".dimmed(),
        commits_folded_in.to_string().cyan(),
        survivor.green()
    );
    println!("  {} '{}' will be deleted", "▸".dimmed(), discarded.red());
    if !kids.is_empty() {
        println!(
            "  {} {} child branch(es) re-parented onto '{}': {}",
            "▸".dimmed(),
            kids.len().to_string().cyan(),
            survivor.green(),
            kids.join(", ").dimmed()
        );
    }
    if !siblings.is_empty() {
        println!(
            "  {} {} sibling branch(es) will be rebased onto '{}': {}",
            "▸".dimmed(),
            siblings.len().to_string().cyan(),
            survivor.green(),
            siblings.join(", ").dimmed()
        );
    }
    println!();

    if !skip_confirm {
        let confirmed = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Fold '{}' into '{}'?", current, parent))
            .default(true)
            .interact()?;
        if !confirmed {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
    }

    let mut tx = Transaction::begin(OpKind::Fold, &repo, false)?;
    tx.plan_branch(&repo, &parent)?;
    tx.plan_branch(&repo, &current)?;
    for sibling in &siblings {
        tx.plan_branch(&repo, sibling)?;
    }
    tx.plan_metadata_ref(&repo, &parent)?;
    tx.plan_metadata_ref(&repo, &current)?;
    for kid in &kids {
        tx.plan_metadata_ref(&repo, kid)?;
    }
    for sibling in &siblings {
        tx.plan_metadata_ref(&repo, sibling)?;
    }
    tx.snapshot()?;

    if keep_branch {
        // delete_branch (vs raw delete_ref) carries the worktree-checkout
        // safety check, which matters if `parent` is checked out elsewhere.
        repo.delete_branch(&parent, true)
            .with_context(|| format!("Failed to delete parent branch '{}'", parent))?;
    } else {
        repo.checkout(&parent)
            .with_context(|| format!("Failed to checkout '{}'", parent))?;
        repo.update_ref(&format!("refs/heads/{}", parent), &current_tip)
            .with_context(|| format!("Failed to fast-forward '{}' to {}", parent, current_tip))?;
        repo.reset_hard(&current_tip)
            .with_context(|| format!("Failed to reset working tree to {}", current_tip))?;
        repo.delete_branch(&current, true)
            .with_context(|| format!("Failed to delete '{}'", current))?;
    }
    // After deletion, `branch_commit(discarded)` errors — record_after surfaces
    // that as Err which we intentionally swallow; the receipt entry stays at
    // oid_after=None, which is exactly what undo needs to recreate the ref.
    tx.record_after(&repo, &parent).ok();
    tx.record_after(&repo, &current).ok();

    if !keep_branch {
        for kid in &kids {
            let meta = BranchMetadata::read(repo.inner(), kid)?.with_context(|| {
                format!(
                    "Child branch '{}' is missing metadata; cannot reparent.",
                    kid
                )
            })?;
            let updated = BranchMetadata {
                parent_branch_name: parent.clone(),
                ..meta
            };
            updated.write(repo.inner(), kid)?;
            tx.record_metadata_ref_after(&repo, kid)?;
        }
    } else {
        let updated = BranchMetadata {
            parent_branch_name: grandparent.clone(),
            parent_branch_revision: grandparent_revision.clone(),
            ..current_meta
        };
        updated.write(repo.inner(), &current)?;
        tx.record_metadata_ref_after(&repo, &current)?;
    }

    BranchMetadata::delete(repo.inner(), &discarded)
        .with_context(|| format!("Failed to delete metadata for '{}'", discarded))?;
    tx.record_metadata_ref_after(&repo, &discarded)?;

    let mut completed_siblings: Vec<String> = Vec::new();
    for sibling in &siblings {
        let result = repo
            .rebase_branch_onto_with_provenance(sibling, &survivor, &old_parent_tip, false)
            .with_context(|| format!("Failed to rebase sibling '{}'", sibling))?;

        match result {
            RebaseResult::Success => {
                tx.push_completed_branch(sibling);
                tx.record_after(&repo, sibling)?;
                if let Some(meta) = BranchMetadata::read(repo.inner(), sibling)? {
                    let updated = BranchMetadata {
                        parent_branch_name: survivor.clone(),
                        // Survivor's tip equals current_tip (we either force-
                        // updated parent → current_tip, or kept current as-is).
                        parent_branch_revision: current_tip.clone(),
                        ..meta
                    };
                    updated.write(repo.inner(), sibling)?;
                }
                tx.record_metadata_ref_after(&repo, sibling)?;
                completed_siblings.push(sibling.clone());
            }
            RebaseResult::Conflict => {
                let _ = repo.rebase_abort();
                // Don't repeat the "stax undo" hint — tx.finish_err prints it.
                let msg = format!(
                    "Conflict rebasing sibling '{}' onto '{}'. The fold's structural \
                     changes are applied; {} sibling(s) rebased before the conflict.",
                    sibling,
                    survivor,
                    completed_siblings.len()
                );
                tx.finish_err(&msg, Some("rebase"), Some(sibling))?;
                bail!("{}", msg);
            }
        }
    }

    tx.finish_ok()?;

    if repo.current_branch()? != survivor {
        let _ = repo.checkout(&survivor);
    }

    println!();
    println!(
        "{} Folded '{}' into '{}'.",
        "✓".green().bold(),
        current.cyan(),
        parent.cyan()
    );
    println!("  Surviving branch: {}", survivor.green().bold());

    let discarded_pr = if keep_branch { parent_pr } else { current_pr };
    if let Some(pr_number) = discarded_pr {
        println!();
        println!(
            "{} '{}' had PR #{}. Close it manually with: {}",
            "ⓘ".blue(),
            discarded.dimmed(),
            pr_number,
            format!("gh pr close {}", pr_number).cyan()
        );
    }

    Ok(())
}
