use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get the stack for current branch
    let stack_branches = stack.current_stack(&current);

    if stack_branches.len() <= 1 {
        // Only trunk or untracked branch
        println!("{}", "No tracked branches in stack.".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
        return Ok(());
    }

    println!();
    for (i, branch_name) in stack_branches.iter().enumerate() {
        let branch = stack.branches.get(branch_name);
        let is_current = branch_name == &current;
        let is_trunk = branch_name == &stack.trunk;

        // Build the line
        let mut line = String::new();

        // Tree structure
        if i == 0 {
            line.push_str("  ");
        } else {
            line.push_str("│ ");
        }

        // Branch indicator
        if is_current {
            line.push_str("◉ ");
        } else if is_trunk {
            line.push_str("○ ");
        } else {
            line.push_str("○ ");
        }

        // Branch name
        let name_display = if is_current {
            branch_name.green().bold().to_string()
        } else if is_trunk {
            branch_name.blue().to_string()
        } else {
            branch_name.white().to_string()
        };
        line.push_str(&name_display);

        // Status indicators
        if let Some(b) = branch {
            if b.needs_restack {
                line.push_str(&" (needs restack)".yellow().to_string());
            }
            if let Some(pr) = b.pr_number {
                line.push_str(&format!(" #{}", pr).dimmed().to_string());
            }
        }

        // Current branch marker
        if is_current {
            line.push_str(&" ← you are here".dimmed().to_string());
        }

        println!("{}", line);

        // Draw connector to next
        if i < stack_branches.len() - 1 && i > 0 {
            println!("│");
        } else if i == 0 && stack_branches.len() > 1 {
            println!("│");
        }
    }
    println!();

    // Show hint if restack needed
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!(
            "{}",
            format!("⚠ {} branch(es) need restacking", needs_restack.len()).yellow()
        );
        println!("Run {} to rebase the stack.", "stax rs".cyan());
        println!();
    }

    Ok(())
}
