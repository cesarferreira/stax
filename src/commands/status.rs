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

struct BranchInfo {
    name: String,
    column: usize,      // Which column this branch is in
    color: Color,
    is_current: bool,
    has_remote: bool,
    commits_ahead: Option<usize>,
    pr_number: Option<u64>,
    needs_restack: bool,
    has_children: bool, // Does this branch have children below it?
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

    // Get remote branches
    let remote_branches = get_remote_branches(repo.workdir()?);

    // Collect all stacks (each direct child of trunk starts a stack)
    let trunk_info = stack.branches.get(&stack.trunk);
    let mut trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();
    trunk_children.sort();

    // Assign colors and columns to stacks
    let mut branch_colors: HashMap<String, Color> = HashMap::new();
    let mut branch_columns: HashMap<String, usize> = HashMap::new();

    for (idx, root) in trunk_children.iter().enumerate() {
        let color = STACK_COLORS[idx % STACK_COLORS.len()];
        assign_stack_info(&stack, root, idx, color, &mut branch_columns, &mut branch_colors);
    }

    let total_columns = trunk_children.len();

    // Collect all branches in display order (leaves first, then parents)
    let mut display_order: Vec<BranchInfo> = Vec::new();
    for root in &trunk_children {
        collect_branch_info(
            &stack,
            root,
            &current,
            &remote_branches,
            &branch_columns,
            &branch_colors,
            repo.workdir()?,
            &mut display_order,
        );
    }

    // Track which columns have active lines (branches still to be rendered)
    let mut active_columns: HashSet<usize> = HashSet::new();
    for bi in &display_order {
        if bi.has_children {
            active_columns.insert(bi.column);
        }
    }

    // Render each branch
    for bi in &display_order {
        // Build the column indicators
        let mut line = String::new();

        for col in 0..total_columns {
            if col == bi.column {
                // This is our branch's column - show the node
                let indicator = if bi.is_current { "◉" } else { "○" };
                if bi.is_current {
                    line.push_str(&format!("{}", indicator.color(bi.color).bold()));
                } else {
                    line.push_str(&format!("{}", indicator.color(bi.color)));
                }
            } else if active_columns.contains(&col) {
                // Another stack has active branches - show line
                let col_color = STACK_COLORS[col % STACK_COLORS.len()];
                line.push_str(&format!("{}", "│".color(col_color)));
            } else {
                line.push(' ');
            }
        }

        // Add spacing and branch name
        line.push_str("  ");

        // Remote icon
        if bi.has_remote {
            line.push_str(&format!("{}", "☁ ".bright_blue()));
        }

        // Branch name
        if bi.is_current {
            line.push_str(&format!("{}", bi.name.bold()));
        } else {
            line.push_str(&bi.name);
        }

        // Commits ahead
        if let Some(n) = bi.commits_ahead {
            if n > 0 {
                if bi.is_current {
                    line.push_str(&format!("{}", format!(" +{}", n).bright_green()));
                } else {
                    line.push_str(&format!("{}", format!(" +{}", n).dimmed()));
                }
            }
        }

        // PR number
        if let Some(pr) = bi.pr_number {
            line.push_str(&format!("{}", format!(" PR #{}", pr).bright_magenta()));
        }

        // Restack warning
        if bi.needs_restack {
            line.push_str(&format!("{}", " ↻".bright_yellow()));
        }

        println!("{}", line);

        // If this branch has no children, its column is no longer active
        if !bi.has_children {
            active_columns.remove(&bi.column);
        }
    }

    // Render trunk
    let is_trunk_current = stack.trunk == current;
    let mut trunk_line = String::new();

    // Show remaining active columns converging to trunk
    for col in 0..total_columns {
        if active_columns.contains(&col) || col == 0 {
            let col_color = STACK_COLORS[col % STACK_COLORS.len()];
            trunk_line.push_str(&format!("{}", "○".color(col_color)));
        } else {
            trunk_line.push(' ');
        }
    }

    trunk_line.push_str("  ");
    if remote_branches.contains(&stack.trunk) {
        trunk_line.push_str(&format!("{}", "☁ ".bright_blue()));
    }

    if is_trunk_current {
        trunk_line.push_str(&format!("{}", stack.trunk.bold()));
    } else {
        trunk_line.push_str(&stack.trunk);
    }

    println!("{}", trunk_line);

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

fn assign_stack_info(
    stack: &Stack,
    branch: &str,
    column: usize,
    color: Color,
    columns: &mut HashMap<String, usize>,
    colors: &mut HashMap<String, Color>,
) {
    columns.insert(branch.to_string(), column);
    colors.insert(branch.to_string(), color);

    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            assign_stack_info(stack, child, column, color, columns, colors);
        }
    }
}

fn collect_branch_info(
    stack: &Stack,
    branch: &str,
    current: &str,
    remote_branches: &HashSet<String>,
    columns: &HashMap<String, usize>,
    colors: &HashMap<String, Color>,
    workdir: &std::path::Path,
    result: &mut Vec<BranchInfo>,
) {
    let info = stack.branches.get(branch);
    let has_children = info.map(|i| !i.children.is_empty()).unwrap_or(false);

    // First collect children (leaves first)
    if let Some(info) = info {
        for child in &info.children {
            collect_branch_info(stack, child, current, remote_branches, columns, colors, workdir, result);
        }
    }

    // Get commits ahead
    let commits_ahead = info
        .and_then(|i| i.parent.as_ref())
        .and_then(|parent| get_commits_ahead(workdir, parent, branch));

    // Then add this branch
    result.push(BranchInfo {
        name: branch.to_string(),
        column: *columns.get(branch).unwrap_or(&0),
        color: *colors.get(branch).unwrap_or(&Color::White),
        is_current: branch == current,
        has_remote: remote_branches.contains(branch),
        commits_ahead,
        pr_number: info.and_then(|i| i.pr_number),
        needs_restack: info.map(|i| i.needs_restack).unwrap_or(false),
        has_children,
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
