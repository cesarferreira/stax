use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get ALL branches in display order (leaves first, trunk last)
    let stack_branches = stack.all_branches_display_order();

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
    let total = stack_branches.len();
    for (i, branch_name) in stack_branches.iter().enumerate() {
        let branch = stack.branches.get(branch_name);
        let is_current = branch_name == &current;
        let is_trunk = branch_name == &stack.trunk;
        let is_last = i == total - 1;

        // Build the prefix based on position
        let prefix = if is_last {
            // Trunk (bottom) - corner piece
            if is_current {
                "◉─┘ ".to_string()
            } else {
                "○─┘ ".to_string()
            }
        } else if i == 0 {
            // Top (leaf)
            if is_current {
                "◉   ".to_string()
            } else {
                "○   ".to_string()
            }
        } else {
            // Middle - vertical connector with circle
            if is_current {
                "│ ◉ ".to_string()
            } else {
                "│ ○ ".to_string()
            }
        };

        // Branch name with color
        let name_display = if is_current {
            branch_name.green().bold().to_string()
        } else if is_trunk {
            branch_name.blue().to_string()
        } else {
            branch_name.cyan().to_string()
        };

        // Status indicators
        let mut status = String::new();
        if let Some(b) = branch {
            if b.needs_restack {
                status.push_str(&" (needs restack)".yellow().to_string());
            }
            if let Some(pr) = b.pr_number {
                status.push_str(&format!(" #{}", pr).dimmed().to_string());
            }
        }

        println!("{}{}{}", prefix, name_display, status);
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
