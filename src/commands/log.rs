use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};
use std::collections::HashSet;
use std::process::Command;

// Colors for different depths (matching status.rs)
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
    let workdir = repo.workdir()?;

    if stack.branches.len() <= 1 {
        println!("{}", "No tracked branches in stack.".dimmed());
        println!(
            "Use {} to start tracking branches.",
            "stax branch track".cyan()
        );
        return Ok(());
    }

    let remote_branches = get_remote_branches(workdir);
    let repo_url = get_repo_url(workdir);

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

    let tree_target_width = (max_column + 1) * 2;

    // Render each branch
    for (i, db) in display_branches.iter().enumerate() {
        let branch = &db.name;
        let info = stack.branches.get(branch);
        let is_current = branch == &current;
        let has_remote = remote_branches.contains(branch);
        let color = DEPTH_COLORS[db.column % DEPTH_COLORS.len()];

        // Check if there are branches at column X below this row
        let has_below_at_col = |col: usize| -> bool {
            if col == 0 && db.column > 0 {
                true
            } else {
                display_branches[i + 1..].iter().any(|b| b.column == col)
            }
        };

        // Check if we need a corner connector
        let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
        let needs_corner = prev_branch_col.map_or(false, |pc| pc > db.column);

        // Build tree graphics
        let mut tree = String::new();
        let mut visual_width = 0;

        for col in 0..=db.column {
            if col == db.column {
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(color)));
                visual_width += 1;

                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(color)));
                    visual_width += 2;
                }
            } else {
                if has_below_at_col(col) {
                    let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
                    tree.push_str(&format!("{} ", "│".color(line_color)));
                } else {
                    tree.push_str("  ");
                }
                visual_width += 2;
            }
        }

        // Pad to consistent width
        while visual_width < tree_target_width {
            tree.push(' ');
            visual_width += 1;
        }

        // Build info part
        let mut info_str = String::new();
        info_str.push(' ');

        // Remote indicator
        if has_remote {
            info_str.push_str(&format!("{} ", "☁".bright_blue()));
        }

        // Branch name
        if is_current {
            info_str.push_str(&format!("{}", branch.bold()));
        } else {
            info_str.push_str(branch);
        }

        // Commits ahead/behind
        if let Some(branch_info) = info {
            if let Some(parent) = &branch_info.parent {
                if let Some((ahead, behind)) = get_commits_ahead_behind(workdir, parent, branch) {
                    let mut commit_info = String::new();
                    if ahead > 0 {
                        commit_info.push_str(&format!(" +{}", ahead));
                    }
                    if behind > 0 {
                        commit_info.push_str(&format!(" -{}", behind));
                    }
                    if !commit_info.is_empty() {
                        if is_current {
                            info_str.push_str(&format!("{}", commit_info.bright_green()));
                        } else {
                            info_str.push_str(&format!("{}", commit_info.dimmed()));
                        }
                    }
                }
            }
        }

        // PR info with link
        if let Some(branch_info) = info {
            if let Some(pr) = branch_info.pr_number {
                if let Some(ref url) = repo_url {
                    info_str.push_str(&format!("{}", format!(" PR #{} {}/pull/{}", pr, url, pr).bright_magenta()));
                } else {
                    info_str.push_str(&format!("{}", format!(" PR #{}", pr).bright_magenta()));
                }
            }
            if branch_info.needs_restack {
                info_str.push_str(&format!("{}", " ↻ needs restack".bright_yellow()));
            }
        }

        println!("{}{}", tree, info_str);

        // Show commits for this branch
        let parent = info.and_then(|i| i.parent.as_deref());
        if let Ok(commits) = repo.branch_commits(branch, parent) {
            // Build detail tree prefix (vertical lines for columns below)
            let detail_prefix = build_detail_prefix(&display_branches, i, tree_target_width, max_column);

            // Show age
            if let Ok(age) = repo.branch_age(branch) {
                println!("{}   {}", detail_prefix, age.dimmed());
            }

            // Show commits
            for commit in commits.iter().take(3) {
                println!(
                    "{}   {} {}",
                    detail_prefix,
                    commit.short_hash.bright_yellow(),
                    commit.message.white()
                );
            }
        }
    }

    // Render trunk with corner connector
    let is_trunk_current = stack.trunk == current;
    let trunk_color = DEPTH_COLORS[0];

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;

    trunk_tree.push_str(&format!("{}", "○".color(trunk_color)));
    trunk_visual_width += 1;

    if max_column >= 1 {
        trunk_tree.push_str(&format!("{}", "─┘".color(trunk_color)));
        trunk_visual_width += 2;
    }

    while trunk_visual_width < tree_target_width {
        trunk_tree.push(' ');
        trunk_visual_width += 1;
    }

    let mut trunk_info_str = String::new();
    trunk_info_str.push(' ');
    if remote_branches.contains(&stack.trunk) {
        trunk_info_str.push_str(&format!("{} ", "☁".bright_blue()));
    }
    if is_trunk_current {
        trunk_info_str.push_str(&format!("{}", stack.trunk.bold()));
    } else {
        trunk_info_str.push_str(&stack.trunk);
    }

    println!("{}{}", trunk_tree, trunk_info_str);

    // Trunk details
    let trunk_detail_prefix = " ".repeat(tree_target_width);
    if let Ok(age) = repo.branch_age(&stack.trunk) {
        println!("{}   {}", trunk_detail_prefix, age.dimmed());
    }
    if let Ok(commits) = repo.branch_commits(&stack.trunk, None) {
        for commit in commits.iter().take(3) {
            println!(
                "{}   {} {}",
                trunk_detail_prefix,
                commit.short_hash.bright_yellow(),
                commit.message.white()
            );
        }
    }

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

fn build_detail_prefix(display_branches: &[DisplayBranch], current_idx: usize, tree_target_width: usize, _max_column: usize) -> String {
    let current_col = display_branches[current_idx].column;
    let mut prefix = String::new();
    let mut visual_width = 0;

    // Check which columns have branches below (same logic as main tree)
    let has_below_at_col = |col: usize| -> bool {
        if col == 0 {
            // Column 0 always connects to trunk (trunk is always at the bottom)
            true
        } else {
            display_branches[current_idx + 1..].iter().any(|b| b.column == col)
        }
    };

    for col in 0..=current_col {
        if has_below_at_col(col) {
            let line_color = DEPTH_COLORS[col % DEPTH_COLORS.len()];
            prefix.push_str(&format!("{} ", "│".color(line_color)));
        } else {
            prefix.push_str("  ");
        }
        visual_width += 2;
    }

    while visual_width < tree_target_width {
        prefix.push(' ');
        visual_width += 1;
    }

    prefix
}

/// Collect branches for display at a fixed column (for isolated chains)
fn collect_display_branches(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
) {
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
            let mut children_with_sizes: Vec<(&String, usize)> = children
                .iter()
                .map(|c| (c, count_chain_size(stack, c)))
                .collect();

            children_with_sizes.sort_by(|a, b| {
                b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0))
            });

            let main_child = children_with_sizes[0].0;
            let side_children: Vec<&String> = children_with_sizes[1..].iter().map(|(c, _)| *c).collect();

            collect_display_branches_with_nesting(stack, main_child, column, result, max_column);

            for side in &side_children {
                collect_display_branches_with_nesting(stack, side, column + 1, result, max_column);
            }
        } else if children.len() == 1 {
            collect_display_branches_with_nesting(stack, &children[0], column, result, max_column);
        }
    }

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

fn get_repo_url(workdir: &std::path::Path) -> Option<String> {
    let output = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(workdir)
        .output()
        .ok()?;

    if output.status.success() {
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Convert SSH URL to HTTPS URL for display
        let url = if url.starts_with("git@github.com:") {
            url.replace("git@github.com:", "https://github.com/")
                .trim_end_matches(".git")
                .to_string()
        } else if url.starts_with("https://") {
            url.trim_end_matches(".git").to_string()
        } else {
            return None;
        };
        Some(url)
    } else {
        None
    }
}

fn get_commits_ahead_behind(workdir: &std::path::Path, parent: &str, branch: &str) -> Option<(usize, usize)> {
    // Commits ahead: parent..branch
    let ahead_output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", parent, branch)])
        .current_dir(workdir)
        .output()
        .ok()?;

    let ahead = if ahead_output.status.success() {
        String::from_utf8_lossy(&ahead_output.stdout)
            .trim()
            .parse()
            .ok()?
    } else {
        0
    };

    // Commits behind: branch..parent
    let behind_output = Command::new("git")
        .args(["rev-list", "--count", &format!("{}..{}", branch, parent)])
        .current_dir(workdir)
        .output()
        .ok()?;

    let behind = if behind_output.status.success() {
        String::from_utf8_lossy(&behind_output.stdout)
            .trim()
            .parse()
            .ok()?
    } else {
        0
    };

    Some((ahead, behind))
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
