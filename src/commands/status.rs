use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};

// Colors for different stacks (cycle through these)
const STACK_COLORS: &[Color] = &[
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::Cyan,
    Color::Blue,
    Color::BrightRed,
    Color::BrightYellow,
    Color::BrightGreen,
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

    // Assign colors to stacks
    let mut stack_colors: std::collections::HashMap<String, Color> = std::collections::HashMap::new();
    for (idx, root) in trunk_children.iter().enumerate() {
        let color = STACK_COLORS[idx % STACK_COLORS.len()];
        assign_stack_color(&stack, root, color, &mut stack_colors);
    }

    // Collect all branches in display order (leaves first, then parents)
    let mut display_order: Vec<(String, usize)> = Vec::new(); // (branch, depth)
    for root in &trunk_children {
        collect_display_order(&stack, root, 0, &mut display_order);
    }

    // Render
    for (branch, depth) in &display_order {
        let is_current = branch == &current;
        let color = stack_colors.get(branch).copied().unwrap_or(Color::White);

        // Build the line
        let indent = "  ".repeat(*depth);
        let indicator = if is_current { "◉" } else { "○" };

        // Get branch info
        let info = stack.branches.get(branch);
        let pr_badge = info
            .and_then(|i| i.pr_number)
            .map(|pr| format!(" PR #{}", pr))
            .unwrap_or_default();

        let restack_badge = if info.map(|i| i.needs_restack).unwrap_or(false) {
            " !"
        } else {
            ""
        };

        // Print line
        if is_current {
            println!(
                "{}{}  {}{}{}",
                indent,
                indicator.color(color).bold(),
                branch.bold(),
                pr_badge.bright_magenta(),
                restack_badge.bright_yellow()
            );
        } else {
            println!(
                "{}{}  {}{}{}",
                indent,
                indicator.color(color),
                branch,
                pr_badge.bright_magenta(),
                restack_badge.bright_yellow()
            );
        }
    }

    // Render trunk
    let is_trunk_current = stack.trunk == current;
    if is_trunk_current {
        println!("{}  {}", "○".white(), stack.trunk.bold());
    } else {
        println!("{}  {}", "○".white(), stack.trunk);
    }

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

fn assign_stack_color(
    stack: &Stack,
    branch: &str,
    color: Color,
    colors: &mut std::collections::HashMap<String, Color>,
) {
    colors.insert(branch.to_string(), color);
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            assign_stack_color(stack, child, color, colors);
        }
    }
}

fn collect_display_order(
    stack: &Stack,
    branch: &str,
    depth: usize,
    result: &mut Vec<(String, usize)>,
) {
    // First collect children (leaves first)
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            collect_display_order(stack, child, depth + 1, result);
        }
    }
    // Then add this branch
    result.push((branch.to_string(), depth));
}
