use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use std::process::Command;

pub fn run(stack_filter: Option<String>, all: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let workdir = repo.workdir()?;

    let target = stack_filter.clone().unwrap_or_else(|| current.clone());
    let show_all = if all {
        true
    } else {
        stack_filter.is_none() && !stack.branches.contains_key(&target)
    };

    if !show_all && !stack.branches.contains_key(&target) {
        anyhow::bail!("Branch '{}' is not tracked in the stack.", target);
    }

    let branches: Vec<String> = if show_all {
        let mut list: Vec<String> = stack
            .branches
            .keys()
            .filter(|&b| b != &stack.trunk)
            .cloned()
            .collect();
        list.sort();
        list
    } else {
        stack
            .current_stack(&target)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    };

    if branches.is_empty() {
        println!("{}", "No tracked branches to range-diff.".dimmed());
        return Ok(());
    }

    for branch in &branches {
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(meta) => meta,
            None => continue,
        };

        let needs_restack = meta.needs_restack(repo.inner()).unwrap_or(false);
        if !needs_restack {
            println!(
                "\n{} {}",
                "Range-diff".cyan(),
                format!("{}..{}", meta.parent_branch_name, branch).bold()
            );
            println!("{}", "  (up to date)".dimmed());
            continue;
        }

        let current_parent = repo.branch_commit(&meta.parent_branch_name)?;

        println!(
            "\n{} {}",
            "Range-diff".cyan(),
            format!("{}..{}", meta.parent_branch_name, branch).bold()
        );

        let output = Command::new("git")
            .args([
                "range-diff",
                &format!("{}..{}", meta.parent_branch_revision, branch),
                &format!("{}..{}", current_parent, branch),
            ])
            .current_dir(workdir)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                println!("{}", "  (no changes)".dimmed());
            } else {
                for line in stdout.lines() {
                    println!("  {}", line);
                }
            }
        }
    }

    let aggregate_stack = if show_all {
        stack.current_stack(&current)
    } else {
        stack.current_stack(&target)
    };

    let top = aggregate_stack
        .iter()
        .rev()
        .find(|b| *b != &stack.trunk)
        .cloned();

    if let Some(top) = top {
        println!("\n{}", "Aggregate stack diff".cyan());
        let output = Command::new("git")
            .args(["diff", "--stat", &format!("{}..{}", stack.trunk, top)])
            .current_dir(workdir)
            .output()?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.trim().is_empty() {
                println!("{}", "  (no changes)".dimmed());
            } else {
                for line in stdout.lines() {
                    println!("  {}", line);
                }
            }
        }
    }

    Ok(())
}
