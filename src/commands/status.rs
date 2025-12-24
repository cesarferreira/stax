use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};
use std::collections::{HashMap, HashSet};
use std::process::Command;

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

    // Get remote branches
    let remote_branches = get_remote_branches(repo.workdir()?);

    // Collect all stacks (each direct child of trunk starts a stack)
    let trunk_info = stack.branches.get(&stack.trunk);
    let mut trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();
    trunk_children.sort();

    // Assign colors to stacks
    let mut stack_colors: HashMap<String, Color> = HashMap::new();
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

        // Check if on remote
        let remote_icon = if remote_branches.contains(branch) {
            "☁ "
        } else {
            ""
        };

        // Get commits ahead of parent
        let commits_info = if let Some(parent) = info.and_then(|i| i.parent.as_ref()) {
            match get_commits_ahead(repo.workdir()?, parent, branch) {
                Some(0) => "".to_string(),
                Some(n) => format!(" +{}", n),
                None => "".to_string(),
            }
        } else {
            "".to_string()
        };

        // PR badge
        let pr_badge = info
            .and_then(|i| i.pr_number)
            .map(|pr| format!(" PR #{}", pr))
            .unwrap_or_default();

        // Restack badge
        let restack_badge = if info.map(|i| i.needs_restack).unwrap_or(false) {
            " ↻"
        } else {
            ""
        };

        // Print line
        if is_current {
            println!(
                "{}{}  {}{}{}{}{}",
                indent,
                indicator.color(color).bold(),
                remote_icon.bright_blue(),
                branch.bold(),
                commits_info.bright_green(),
                pr_badge.bright_magenta(),
                restack_badge.bright_yellow()
            );
        } else {
            println!(
                "{}{}  {}{}{}{}{}",
                indent,
                indicator.color(color),
                remote_icon.bright_blue(),
                branch,
                commits_info.dimmed(),
                pr_badge.bright_magenta(),
                restack_badge.bright_yellow()
            );
        }
    }

    // Render trunk
    let is_trunk_current = stack.trunk == current;
    let trunk_remote = if remote_branches.contains(&stack.trunk) {
        "☁ "
    } else {
        ""
    };

    if is_trunk_current {
        println!(
            "{}  {}{}",
            "○".white(),
            trunk_remote.bright_blue(),
            stack.trunk.bold()
        );
    } else {
        println!(
            "{}  {}{}",
            "○".white(),
            trunk_remote.bright_blue(),
            stack.trunk
        );
    }

    // Show hint if restack needed
    let needs_restack = stack.needs_restack();
    if !needs_restack.is_empty() {
        println!();
        println!(
            "{}",
            format!("↻ {} branch(es) need restacking", needs_restack.len()).bright_yellow()
        );
        println!("Run {} to rebase.", "stax rs --restack".bright_cyan());
    }

    Ok(())
}

fn get_remote_branches(workdir: &std::path::Path) -> HashSet<String> {
    let output = Command::new("git")
        .args(["branch", "-r", "--format=%(refname:short)"])
        .current_dir(workdir)
        .output();

    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|s| s.trim().strip_prefix("origin/"))
            .map(|s| s.to_string())
            .collect(),
        Err(_) => HashSet::new(),
    }
}

fn get_commits_ahead(workdir: &std::path::Path, parent: &str, branch: &str) -> Option<usize> {
    let output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", parent, branch)])
        .current_dir(workdir)
        .output()
        .ok()?;

    if output.status.success() {
        String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()
    } else {
        None
    }
}

fn assign_stack_color(
    stack: &Stack,
    branch: &str,
    color: Color,
    colors: &mut HashMap<String, Color>,
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
