use crate::engine::Stack;
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
    } else if stack_filter.is_none() && !stack.branches.contains_key(&target) {
        true
    } else {
        false
    };

    if !show_all && !stack.branches.contains_key(&target) {
        anyhow::bail!("Branch '{}' is not tracked in the stack.", target);
    }

    let branches: Vec<String> = if show_all {
        let mut list: Vec<String> = stack
            .branches
            .keys()
            .cloned()
            .filter(|b| b != &stack.trunk)
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
        println!("{}", "No tracked branches to diff.".dimmed());
        return Ok(());
    }

    for branch in &branches {
        let info = stack.branches.get(branch);
        let parent = info.and_then(|b| b.parent.clone());
        let needs_restack = info.map(|b| b.needs_restack).unwrap_or(false);

        let Some(parent) = parent else {
            continue;
        };

        let restack_marker = if needs_restack { " ↻" } else { "" };
        println!(
            "\n{} {}{}",
            "Diff".cyan(),
            format!("{}..{}", parent, branch).bold(),
            restack_marker.yellow()
        );

        let output = Command::new("git")
            .args(["diff", "--stat", &format!("{}..{}", parent, branch)])
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
