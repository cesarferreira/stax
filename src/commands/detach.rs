use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::Result;
use colored::Colorize;

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

    // Begin transaction
    let mut tx = Transaction::begin(OpKind::Detach, &repo, false)?;
    tx.plan_branch(&repo, &target)?;
    for child in &children {
        tx.plan_branch(&repo, child)?;
    }
    tx.snapshot()?;

    // Reparent children to detached branch's parent
    for child in &children {
        let child_meta = BranchMetadata::read(repo.inner(), child)?;
        if let Some(meta) = child_meta {
            let parent_rev = repo.branch_commit(&parent)?;
            let merge_base = repo
                .merge_base(&parent, child)
                .unwrap_or_else(|_| parent_rev.clone());
            let updated = BranchMetadata {
                parent_branch_name: parent.clone(),
                parent_branch_revision: merge_base,
                ..meta
            };
            updated.write(repo.inner(), child)?;
        }
        tx.record_after(&repo, child)?;
    }

    // Set detached branch's parent to trunk
    let trunk_rev = repo.branch_commit(&trunk)?;
    let merge_base = repo
        .merge_base(&trunk, &target)
        .unwrap_or_else(|_| trunk_rev.clone());
    let existing = BranchMetadata::read(repo.inner(), &target)?;
    let updated = if let Some(meta) = existing {
        BranchMetadata {
            parent_branch_name: trunk.clone(),
            parent_branch_revision: merge_base,
            ..meta
        }
    } else {
        BranchMetadata::new(&trunk, &merge_base)
    };
    updated.write(repo.inner(), &target)?;
    tx.record_after(&repo, &target)?;

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
