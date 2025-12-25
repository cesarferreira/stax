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

    // Group branches by their chain (each trunk child starts a chain)
    // Only the LARGEST chain shows vertical lines, others are at column 0
    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    // Find all chains and identify the largest one
    let mut largest_chain: Vec<String> = Vec::new();
    for chain_root in &trunk_children {
        let chain_branches = collect_chain_branches(&stack, chain_root);
        if chain_branches.len() > largest_chain.len() {
            largest_chain = chain_branches;
        }
    }

    // Only the largest chain (if it has multiple branches) gets vertical lines
    let multi_branch_chains: HashSet<String> = if largest_chain.len() > 1 {
        largest_chain.into_iter().collect()
    } else {
        HashSet::new()
    };

    // Separate into multi-branch chain vs isolated branches
    let (mut in_chain, mut isolated): (Vec<_>, Vec<_>) = branches_with_depth
        .into_iter()
        .partition(|(name, _)| multi_branch_chains.contains(name));

    // Sort both by depth descending
    in_chain.sort_by(|a, b| b.1.cmp(&a.1));
    isolated.sort_by(|a, b| b.1.cmp(&a.1));

    // Display order: [isolated branches] then [multi-branch chain branches]
    let display_order: Vec<(String, usize, bool)> = isolated
        .into_iter()
        .map(|(name, depth)| (name, depth, false)) // false = isolated
        .chain(in_chain.into_iter().map(|(name, depth)| (name, depth, true))) // true = in chain
        .collect();

    let has_chain_branches = display_order.iter().any(|(_, _, in_chain)| *in_chain);

    // Render each branch
    for (branch, _depth, in_chain) in &display_order {
        let info = stack.branches.get(branch);
        let is_current = branch == &current;
        let has_remote = remote_branches.contains(branch);

        let color = if *in_chain {
            DEPTH_COLORS[1 % DEPTH_COLORS.len()]
        } else {
            DEPTH_COLORS[0]
        };

        // Build tree graphics
        let mut tree = String::new();

        if *in_chain {
            // Branches in multi-branch chain: vertical line at column 0, circle at column 1
            tree.push_str(&format!("{} ", "│".color(DEPTH_COLORS[0])));
            let circle = if is_current { "◉" } else { "○" };
            tree.push_str(&format!("{}", circle.color(color)));
            tree.push_str("  "); // 2 spaces to align (total: 5 chars before name)
        } else {
            // Isolated branches: circle at column 0, no vertical line
            let circle = if is_current { "◉" } else { "○" };
            tree.push_str(&format!("{}", circle.color(color)));
            tree.push_str("    "); // 4 spaces to align (total: 5 chars before name)
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

    // Add corner connector if there are chain branches above
    if has_chain_branches {
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

fn collect_chain_branches(stack: &Stack, branch: &str) -> Vec<String> {
    let mut result = vec![branch.to_string()];
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            result.extend(collect_chain_branches(stack, child));
        }
    }
    result
}
