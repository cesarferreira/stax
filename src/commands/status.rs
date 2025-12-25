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

/// Represents a branch in the display with its column position
struct DisplayBranch {
    name: String,
    column: usize,
}

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

    // Get trunk children and build display list with proper tree structure
    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    if trunk_children.is_empty() {
        println!("{}", "No tracked branches in stack.".dimmed());
        return Ok(());
    }

    // Find the largest chain to display at column 1
    let mut largest_chain_root: Option<String> = None;
    let mut largest_chain_size = 0;
    for chain_root in &trunk_children {
        let size = count_chain_size(&stack, chain_root);
        if size > largest_chain_size {
            largest_chain_size = size;
            largest_chain_root = Some(chain_root.clone());
        }
    }

    // Build display list: isolated branches first (column 0), then the main chain (column 1+)
    let mut display_branches: Vec<DisplayBranch> = Vec::new();

    // Add isolated chains (not the largest) at column 0
    for chain_root in &trunk_children {
        if largest_chain_root.as_ref() != Some(chain_root) {
            collect_display_branches(&stack, chain_root, 0, &mut display_branches);
        }
    }

    // Add the largest chain at column 1 (with proper nested columns)
    let mut max_column = 0;
    if let Some(ref root) = largest_chain_root {
        if largest_chain_size > 1 {
            collect_display_branches_with_nesting(&stack, root, 1, &mut display_branches, &mut max_column);
        } else {
            // Single branch chain, show at column 0
            collect_display_branches(&stack, root, 0, &mut display_branches);
        }
    }

    // Render each branch
    for (i, db) in display_branches.iter().enumerate() {
        let branch = &db.name;
        let info = stack.branches.get(branch);
        let is_current = branch == &current;
        let has_remote = remote_branches.contains(branch);
        let color = DEPTH_COLORS[db.column % DEPTH_COLORS.len()];

        // Check if there are branches at column X below this row (for vertical lines)
        // Column 0 is always "active" for non-column-0 branches because it connects to trunk
        let has_below_at_col = |col: usize| -> bool {
            if col == 0 && db.column > 0 {
                // Column 0 connects to trunk via the corner connector
                true
            } else {
                display_branches[i + 1..].iter().any(|b| b.column == col)
            }
        };

        // Check if we need a corner connector - this happens when the PREVIOUS branch was at a higher column
        // The corner shows that a side branch joins back to this level
        let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
        let needs_corner = prev_branch_col.map_or(false, |pc| pc > db.column);

        // Build tree graphics
        let mut tree = String::new();

        // Draw columns 0 to max_column
        for col in 0..=max_column {
            if col == db.column {
                // This is our column - draw circle
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(color)));

                // Check if we need corner connector (side branch ending)
                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(color)));
                } else {
                    tree.push(' ');
                }
            } else if col < db.column {
                // Columns to our left - draw vertical line if there are branches at this column below
                if has_below_at_col(col) {
                    let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
                    tree.push_str(&format!("{} ", "│".color(line_color)));
                } else {
                    tree.push_str("  ");
                }
            } else {
                // Columns to our right - just space
                tree.push_str("  ");
            }
        }

        // Build info part
        let mut info_str = String::new();
        info_str.push(' '); // Space after tree

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
    trunk_tree.push_str(&format!("{}", "○".color(trunk_color)));

    // Corner connector to the main chain (column 1) if it exists
    if max_column >= 1 {
        trunk_tree.push_str(&format!("{}", "─┘".color(trunk_color)));
        for _ in 2..=max_column {
            trunk_tree.push_str("  ");
        }
    }
    trunk_tree.push(' ');

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

/// Collect branches for display at a fixed column (for isolated chains)
fn collect_display_branches(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
) {
    // First collect all descendants (depth-first, children before parent)
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            collect_display_branches(stack, child, column, result);
        }
    }
    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
}

/// Collect branches with proper nesting for branches that have multiple children
/// Order: main child's subtree -> side branches -> current branch
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let children = &info.children;

        if children.len() > 1 {
            // Multiple children - find the "main" child (one with largest subtree, or first alphabetically if tied)
            let mut children_with_sizes: Vec<(&String, usize)> = children
                .iter()
                .map(|c| (c, count_chain_size(stack, c)))
                .collect();

            // Sort by size descending, then alphabetically for ties
            children_with_sizes.sort_by(|a, b| {
                b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0))
            });

            let main_child = children_with_sizes[0].0;
            let side_children: Vec<&String> = children_with_sizes[1..].iter().map(|(c, _)| *c).collect();

            // 1. Process main child first (continues at same column)
            collect_display_branches_with_nesting(stack, main_child, column, result, max_column);

            // 2. Process side branches at column + 1 (shown after main child's subtree)
            for side in &side_children {
                collect_display_branches_with_nesting(stack, side, column + 1, result, max_column);
            }
        } else if children.len() == 1 {
            // Single child - continues at same column
            collect_display_branches_with_nesting(stack, &children[0], column, result, max_column);
        }
    }

    // 3. Add current branch after all children are processed
    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
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

fn count_chain_size(stack: &Stack, root: &str) -> usize {
    let mut count = 1;
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            count += count_chain_size(stack, child);
        }
    }
    count
}
