use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};
use std::collections::HashSet;
use std::process::Command;

// Colors for different depths
const DEPTH_COLORS: &[Color] = &[
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

    let remote_branches = get_remote_branches(repo.workdir()?);

    // Build display list: collect all branches with their depth from trunk
    let mut branches_with_depth: Vec<(String, usize)> = Vec::new();
    collect_branches_with_depth(&stack, &stack.trunk, 0, &mut branches_with_depth);

    // Remove trunk (we'll render it separately)
    branches_with_depth.retain(|(name, _)| name != &stack.trunk);

    if branches_with_depth.is_empty() {
        println!("{}", "No tracked branches in stack.".dimmed());
        return Ok(());
    }

    // Sort by depth descending (leaves first = deepest at top)
    branches_with_depth.sort_by(|a, b| b.1.cmp(&a.1));

    // In fp style:
    // - First branch (leaf/top): circle at column 0, no vertical line
    // - Middle branches: vertical line at column 0, circle at column 1
    // - Trunk: circle at column 0 with corner connector to column 1

    let has_branches = !branches_with_depth.is_empty();

    // Render each branch
    for (i, (branch, _depth)) in branches_with_depth.iter().enumerate() {
        let info = stack.branches.get(branch);
        let is_current = branch == &current;
        let has_remote = remote_branches.contains(branch);
        let is_first = i == 0;

        let color = if is_first {
            DEPTH_COLORS[0]
        } else {
            DEPTH_COLORS[1 % DEPTH_COLORS.len()]
        };

        // Build tree graphics
        let mut tree = String::new();

        if is_first {
            // First branch (leaf): circle at column 0, no vertical line
            let circle = if is_current { "◉" } else { "○" };
            tree.push_str(&format!("{}", circle.color(color)));
            tree.push_str("    "); // 4 spaces to align (total: 5 chars before name)
        } else {
            // Middle branches: vertical line at column 0, circle at column 1
            tree.push_str(&format!("{} ", "│".color(DEPTH_COLORS[0])));
            let circle = if is_current { "◉" } else { "○" };
            tree.push_str(&format!("{}", circle.color(color)));
            tree.push_str("  "); // 2 spaces to align (total: 5 chars before name)
        }

        // Build info part
        let mut info_str = String::new();

        if has_remote {
            info_str.push_str(&format!("{} ", "☁".bright_blue()));
        }

        if is_current {
            info_str.push_str(&format!("{}", branch.bold()));
        } else {
            info_str.push_str(branch);
        }

        // Commits ahead
        if let Some(branch_info) = info {
            if let Some(parent) = &branch_info.parent {
                if let Some(n) = get_commits_ahead(repo.workdir()?, parent, branch) {
                    if n > 0 {
                        let commit_str = format!(" +{}", n);
                        if is_current {
                            info_str.push_str(&format!("{}", commit_str.bright_green()));
                        } else {
                            info_str.push_str(&format!("{}", commit_str.dimmed()));
                        }
                    }
                }
            }
        }

        if let Some(branch_info) = info {
            if let Some(pr) = branch_info.pr_number {
                info_str.push_str(&format!("{}", format!(" PR #{}", pr).bright_magenta()));
            }
            if branch_info.needs_restack {
                info_str.push_str(&format!("{}", " ↻".bright_yellow()));
            }
        }

        println!("{}{}", tree, info_str);
    }

    // Render trunk with corner connector
    let is_trunk_current = stack.trunk == current;
    let trunk_color = DEPTH_COLORS[0];

    let mut trunk_tree = String::new();

    // Add the trunk circle
    trunk_tree.push_str(&format!("{}", "○".color(trunk_color)));

    // Add corner connector if there are branches above
    if has_branches {
        // Draw horizontal line and corner to connect to column 1
        trunk_tree.push_str(&format!("{}", "─".color(trunk_color)));
        trunk_tree.push_str(&format!("{}", "┘".color(trunk_color)));
        trunk_tree.push_str("  "); // 2 spaces to align (total: 5 chars before name)
    } else {
        trunk_tree.push_str("    "); // 4 spaces to align (total: 5 chars before name)
    }

    let mut trunk_info = String::new();
    if remote_branches.contains(&stack.trunk) {
        trunk_info.push_str(&format!("{} ", "☁".bright_blue()));
    }
    if is_trunk_current {
        trunk_info.push_str(&format!("{}", stack.trunk.bold()));
    } else {
        trunk_info.push_str(&stack.trunk);
    }

    println!("{}{}", trunk_tree, trunk_info);

    // Show restack hint
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

fn collect_branches_with_depth(
    stack: &Stack,
    branch: &str,
    depth: usize,
    result: &mut Vec<(String, usize)>,
) {
    result.push((branch.to_string(), depth));

    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            collect_branches_with_depth(stack, child, depth + 1, result);
        }
    }
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
