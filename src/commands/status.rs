use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    if stack.branches.len() <= 1 {
        println!("{}", "No tracked branches in stack.".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
        return Ok(());
    }

    println!();

    // Render tree starting from trunk
    render_branch_tree(&stack, &stack.trunk, &current, 0);

    println!();

    // Show hint if restack needed
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!(
            "{}",
            format!("!  {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase the stack.", "stax rs".bright_cyan());
        println!();
    }

    Ok(())
}

fn render_branch_tree(stack: &Stack, branch: &str, current: &str, depth: usize) {
    let branch_info = stack.branches.get(branch);
    let is_current = branch == current;
    let is_trunk = branch == &stack.trunk;

    // Get children
    let children: Vec<String> = branch_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    // Render children first (so leaves are at top)
    for child in children.iter().rev() {
        render_branch_tree(stack, child, current, depth + 1);
    }

    // Simple depth-based indentation (4 spaces per level)
    let indent = "    ".repeat(depth);

    // Branch indicator
    let indicator = if is_current { "*" } else { "o" };
    let indicator_colored = if is_current {
        indicator.bright_green().bold()
    } else if is_trunk {
        indicator.bright_blue()
    } else {
        indicator.bright_cyan()
    };

    // Branch name with colors
    let name_colored = if is_current {
        branch.bright_green().bold()
    } else if is_trunk {
        branch.bright_blue().bold()
    } else {
        branch.bright_cyan()
    };

    // Status badges
    let mut badges = String::new();

    if is_current {
        badges.push_str(&" <".bright_green().to_string());
    }

    if let Some(b) = branch_info {
        if b.needs_restack {
            badges.push_str(&" [needs restack]".bright_yellow().to_string());
        }
        if let Some(pr) = b.pr_number {
            badges.push_str(&format!(" PR #{}", pr).bright_magenta().to_string());
        }
    }

    // Render this branch
    println!("{}{} {}{}", indent, indicator_colored, name_colored, badges);
}
