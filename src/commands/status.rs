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

    // Collect all stacks (each direct child of trunk starts a stack)
    let trunk_info = stack.branches.get(&stack.trunk);
    let mut trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();
    trunk_children.sort();

    // Find which stack contains the current branch
    let current_stack_root = find_stack_containing(&stack, &trunk_children, &current);

    // Render each stack
    for stack_root in trunk_children.iter() {
        let is_current_stack = current_stack_root.as_ref() == Some(stack_root);
        render_stack(&stack, stack_root, &current, is_current_stack);
    }

    // Render trunk
    let is_current = stack.trunk == current;
    let indicator = "○";

    // Show connector line if there's a current stack
    if current_stack_root.is_some() {
        print!("{}", indicator.bright_blue());
        print!("{}", "─┘".bright_black());
    } else {
        print!("{}", indicator.bright_blue());
        print!("{}", "┘".bright_black());
        print!(" ");
    }
    print!(" ");
    if is_current {
        println!("{}", stack.trunk.bright_green().bold());
    } else {
        println!("{}", stack.trunk.bright_blue().bold());
    }

    // Show hint if restack needed
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!();
        println!(
            "{}",
            format!("!  {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase the stack.", "stax rs".bright_cyan());
    }

    Ok(())
}

fn render_stack(stack: &Stack, branch: &str, current: &str, is_current_stack: bool) {
    // Collect all branches in this stack (linear chain)
    let mut branches = Vec::new();
    collect_stack_branches(stack, branch, &mut branches);

    // Render from leaf to root (top to bottom)
    for b in branches.iter() {
        let is_current = *b == current;

        // Indicator
        let indicator = if is_current { "◉" } else { "○" };
        let indicator_colored = if is_current {
            indicator.bright_green().bold()
        } else {
            indicator.bright_cyan()
        };

        // Branch name
        let name_colored = if is_current {
            b.bright_green().bold()
        } else {
            b.bright_cyan()
        };

        // Status badges
        let mut badges = String::new();
        if let Some(info) = stack.branches.get(*b) {
            if info.needs_restack {
                badges.push_str(&" [needs restack]".bright_yellow().to_string());
            }
            if let Some(pr) = info.pr_number {
                badges.push_str(&format!(" PR #{}", pr).bright_magenta().to_string());
            }
        }

        // Print with left margin: │ for current stack, space for others
        if is_current_stack {
            print!("{}", "│".bright_blue());
        } else {
            print!(" ");
        }
        println!(" {} {}{}", indicator_colored, name_colored, badges);
    }
}

fn collect_stack_branches<'a>(stack: &'a Stack, branch: &'a str, result: &mut Vec<&'a str>) {
    // First collect children (to get leaves first)
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            collect_stack_branches(stack, child, result);
        }
    }
    // Then add this branch
    result.push(branch);
}

fn find_stack_containing(stack: &Stack, stack_roots: &[String], current: &str) -> Option<String> {
    for root in stack_roots {
        if branch_is_in_stack(stack, root, current) {
            return Some(root.clone());
        }
    }
    None
}

fn branch_is_in_stack(stack: &Stack, root: &str, target: &str) -> bool {
    if root == target {
        return true;
    }
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            if branch_is_in_stack(stack, child, target) {
                return true;
            }
        }
    }
    false
}
