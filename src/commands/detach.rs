use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::Result;
use colored::Colorize;
use std::path::Path;
use std::process::Command;

pub fn run(branch: Option<String>, yes: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = stack.trunk.clone();
    let target = branch.unwrap_or_else(|| current.clone());

    if target == trunk {
        anyhow::bail!("Cannot detach the trunk branch.");
    }

    let branch_info = stack
        .branches
        .get(&target)
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' is not tracked by stax.", target))?;

    let parent = branch_info
        .parent
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Branch '{}' has no parent.", target))?;

    let children: Vec<String> = branch_info.children.clone();

    if !yes && !children.is_empty() {
        println!(
            "Detaching '{}' will reparent {} child branch(es) onto '{}':",
            target.cyan(),
            children.len(),
            parent.blue()
        );
        for child in &children {
            println!("  {} → {}", child.cyan(), parent.blue());
        }
        println!();
        let confirm = dialoguer::Confirm::new()
            .with_prompt("Continue?")
            .default(true)
            .interact()?;
        if !confirm {
            println!("Aborted.");
            return Ok(());
        }
    }

    // Begin transaction. detach never moves any branch HEAD -- the only mutation
    // is each branch's `refs/branch-metadata/<branch>` blob -- so the metadata
    // refs MUST be planned/recorded or `stax undo` restores nothing (issue #835).
    let mut tx = Transaction::begin(OpKind::Detach, &repo, false)?;
    tx.plan_branch(&repo, &target)?;
    tx.plan_metadata_ref(&repo, &target)?;
    for child in &children {
        tx.plan_branch(&repo, child)?;
        tx.plan_metadata_ref(&repo, child)?;
    }
    tx.snapshot()?;

    let workdir = repo.workdir()?;

    // Reparent children to detached branch's parent
    let parent_rev = repo.branch_commit(&parent)?;
    for child in &children {
        let child_meta = BranchMetadata::read(repo.inner(), child)?;
        if let Some(meta) = child_meta {
            // The merge-base with the new parent is the real boundary; fall back
            // to the parent tip only when it is genuinely in `child`'s ancestry,
            // else keep the previously recorded boundary (see #830).
            let merge_base = repo.merge_base(&parent, child).ok();
            let parent_branch_revision = repo.resolve_child_parent_boundary(
                child,
                &[merge_base.as_deref(), Some(parent_rev.as_str())],
                &meta.parent_branch_revision,
            );
            let updated = BranchMetadata {
                parent_branch_name: parent.clone(),
                parent_branch_revision,
                ..meta
            };
            updated.write(repo.inner(), child)?;
        }
        tx.record_after(&repo, child)?;
        tx.record_known_metadata_after(
            child,
            ref_oid(workdir, &format!("refs/branch-metadata/{}", child)).as_deref(),
        );
    }

    // Set detached branch's parent to trunk
    let trunk_rev = repo.branch_commit(&trunk)?;
    let trunk_merge_base = repo.merge_base(&trunk, &target).ok();
    let existing = BranchMetadata::read(repo.inner(), &target)?;
    let updated = if let Some(meta) = existing {
        let parent_branch_revision = repo.resolve_child_parent_boundary(
            &target,
            &[trunk_merge_base.as_deref(), Some(trunk_rev.as_str())],
            &meta.parent_branch_revision,
        );
        BranchMetadata {
            parent_branch_name: trunk.clone(),
            parent_branch_revision,
            ..meta
        }
    } else {
        BranchMetadata::new(
            &trunk,
            trunk_merge_base.as_deref().unwrap_or(trunk_rev.as_str()),
        )
    };
    updated.write(repo.inner(), &target)?;
    tx.record_after(&repo, &target)?;
    tx.record_known_metadata_after(
        &target,
        ref_oid(workdir, &format!("refs/branch-metadata/{}", target)).as_deref(),
    );

    tx.finish_ok()?;

    println!(
        "Detached '{}' from its stack. It now branches off '{}'.",
        target.green(),
        trunk.blue()
    );

    if !children.is_empty() {
        println!("Reparented:");
        for child in &children {
            println!("  {} → {}", child.cyan(), parent.blue());
        }
    }

    println!(
        "{}",
        "Run `stax restack` to rebase affected branches.".yellow()
    );

    Ok(())
}

/// Resolve a ref to its OID via a git subprocess; `None` when absent.
/// Branch metadata refs are written with `git update-ref` (see
/// `git::refs::write_metadata`), which libgit2's cached refdb may not observe
/// within the same process — mirrors `split.rs`/`branch/create.rs`'s `ref_oid`.
fn ref_oid(workdir: &Path, refname: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "--verify", refname])
        .current_dir(workdir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
