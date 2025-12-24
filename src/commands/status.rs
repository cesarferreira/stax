use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};

struct BranchDisplay {
    name: String,
    depth: usize,
    color: Color,
    is_current: bool,
    pr_number: Option<u64>,
    needs_restack: bool,
}

// Colors for different stacks (cycle through these)
const STACK_COLORS: &[Color] = &[
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::Cyan,
    Color::Blue,
    Color::Red,
    Color::BrightYellow,
    Color::BrightGreen,
    Color::BrightMagenta,
    Color::BrightCyan,
];

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

    // Build a list of all branches with their depth and color
    let mut all_branches: Vec<BranchDisplay> = Vec::new();

    // Process each stack with its own color
    for (stack_idx, root) in trunk_children.iter().enumerate() {
        let color = STACK_COLORS[stack_idx % STACK_COLORS.len()];
        collect_branches_with_depth(&stack, root, 0, color, &current, &mut all_branches);
    }

    // Find max depth to calculate column widths
    let max_depth = all_branches.iter().map(|b| b.depth).max().unwrap_or(0);

    // Render each branch
    for branch in &all_branches {
        // Build the tree lines
        let mut line = String::new();

        // Add connector lines for depth
        for d in 0..=max_depth {
            if d < branch.depth {
                // Add colored vertical line
                line.push_str(&format!("{}", "│".color(branch.color)));
            } else if d == branch.depth {
                // Add the node indicator
                let indicator = if branch.is_current { "◉" } else { "○" };
                if branch.is_current {
                    line.push_str(&format!("{}", indicator.color(branch.color).bold()));
                } else {
                    line.push_str(&format!("{}", indicator.color(branch.color)));
                }
            } else {
                line.push(' ');
            }
        }

        // Add spacing and branch name
        line.push_str("  ");

        // Branch name with highlighting
        let name = if branch.is_current {
            format!("{}", branch.name.bold())
        } else {
            branch.name.clone()
        };
        line.push_str(&name);

        // Add PR badge if present
        if let Some(pr) = branch.pr_number {
            line.push_str(&format!(" {}", format!("PR #{}", pr).bright_magenta()));
        }

        // Add restack warning
        if branch.needs_restack {
            line.push_str(&format!(" {}", "!".bright_yellow()));
        }

        println!("{}", line);
    }

    // Render trunk at the bottom
    let is_trunk_current = stack.trunk == current;
    let trunk_display = if is_trunk_current {
        format!("{}", stack.trunk.bright_green().bold())
    } else {
        format!("{}", stack.trunk.white())
    };

    // Build trunk line with connectors
    let mut trunk_line = String::new();
    for _ in 0..max_depth {
        trunk_line.push(' ');
    }
    trunk_line.push_str(&format!("{}  {}", "○".white(), trunk_display));
    println!("{}", trunk_line);

    // Show hint if restack needed
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!();
        println!(
            "{}",
            format!("! {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase.", "stax rs --restack".bright_cyan());
    }

    Ok(())
}

fn collect_branches_with_depth(
    stack: &Stack,
    branch: &str,
    depth: usize,
    color: Color,
    current: &str,
    result: &mut Vec<BranchDisplay>,
) {
    let info = stack.branches.get(branch);
    let is_current = branch == current;
    let pr_number = info.and_then(|i| i.pr_number);
    let needs_restack = info.map(|i| i.needs_restack).unwrap_or(false);

    // First, recurse into children (leaves first)
    if let Some(info) = info {
        for child in &info.children {
            collect_branches_with_depth(stack, child, depth + 1, color, current, result);
        }
    }

    // Then add this branch
    result.push(BranchDisplay {
        name: branch.to_string(),
        depth,
        color,
        is_current,
        pr_number,
        needs_restack,
    });
}
