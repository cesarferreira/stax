use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};
use std::collections::{HashMap, HashSet};
use std::process::Command;

// Colors for different branches (cycle through these)
const BRANCH_COLORS: &[Color] = &[
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::Cyan,
    Color::Blue,
    Color::BrightRed,
    Color::BrightYellow,
    Color::BrightGreen,
];

struct BranchDisplay {
    name: String,
    column: usize,
    color: Color,
    is_current: bool,
    has_remote: bool,
    commits_ahead: Option<usize>,
    pr_number: Option<u64>,
    needs_restack: bool,
    depth: usize, // depth from trunk (for sorting)
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

    // Get trunk's direct children
    let trunk_info = stack.branches.get(&stack.trunk);
    let mut trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default();
    trunk_children.sort();

    if trunk_children.is_empty() {
        println!("{}", "No tracked branches in stack.".dimmed());
        return Ok(());
    }

    // Assign columns using DFS - each branch gets a column based on tree position
    // First child inherits parent column, additional children get new columns
    let mut branch_columns: HashMap<String, usize> = HashMap::new();
    let mut branch_colors: HashMap<String, Color> = HashMap::new();
    let mut next_column: usize = 0;

    for root in &trunk_children {
        assign_columns_dfs(
            &stack,
            root,
            &mut next_column,
            &mut branch_columns,
            &mut branch_colors,
            true, // first child of trunk
        );
    }

    let max_column = next_column;

    // Collect branches with depth info
    let mut display_list: Vec<BranchDisplay> = Vec::new();
    collect_branches_with_depth(
        &stack,
        &stack.trunk,
        0,
        &current,
        &remote_branches,
        &branch_columns,
        &branch_colors,
        repo.workdir()?,
        &mut display_list,
    );

    // Sort: higher column first, then by depth descending (leaves first)
    display_list.sort_by(|a, b| {
        match b.column.cmp(&a.column) {
            std::cmp::Ordering::Equal => b.depth.cmp(&a.depth),
            other => other,
        }
    });

    // Track active columns (need vertical lines)
    let mut active_columns: HashSet<usize> = HashSet::new();

    // Render each branch
    for bd in &display_list {
        // Build tree graphics
        let mut tree_part = String::new();

        for col in 0..max_column {
            if col == bd.column {
                let circle = if bd.is_current { "◉" } else { "○" };
                tree_part.push_str(&format!("{}", circle.color(bd.color)));
            } else if active_columns.contains(&col) {
                // Find the color for this column
                let color = find_color_for_column(&branch_columns, &branch_colors, col);
                tree_part.push_str(&format!("{}", "│".color(color)));
            } else {
                tree_part.push(' ');
            }
        }

        // After rendering, update active columns
        // Add this column (it continues down toward trunk)
        active_columns.insert(bd.column);

        // Build info part
        let mut info_part = String::new();
        info_part.push_str("  ");

        if bd.has_remote {
            info_part.push_str(&format!("{}", "☁ ".bright_blue()));
        }

        if bd.is_current {
            info_part.push_str(&format!("{}", bd.name.bold()));
        } else {
            info_part.push_str(&bd.name);
        }

        if let Some(n) = bd.commits_ahead {
            if n > 0 {
                let commit_str = format!(" +{}", n);
                if bd.is_current {
                    info_part.push_str(&format!("{}", commit_str.bright_green()));
                } else {
                    info_part.push_str(&format!("{}", commit_str.dimmed()));
                }
            }
        }

        if let Some(pr) = bd.pr_number {
            info_part.push_str(&format!("{}", format!(" PR #{}", pr).bright_magenta()));
        }

        if bd.needs_restack {
            info_part.push_str(&format!("{}", " ↻".bright_yellow()));
        }

        println!("{}{}", tree_part, info_part);
    }

    // Render trunk
    let mut trunk_tree = String::new();
    for col in 0..max_column {
        if active_columns.contains(&col) {
            // Find color for this column
            let color = find_color_for_column(&branch_columns, &branch_colors, col);
            trunk_tree.push_str(&format!("{}", "○".color(color)));
        } else {
            trunk_tree.push(' ');
        }
    }

    let is_trunk_current = stack.trunk == current;
    let mut trunk_info = String::new();
    trunk_info.push_str("  ");
    if remote_branches.contains(&stack.trunk) {
        trunk_info.push_str(&format!("{}", "☁ ".bright_blue()));
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

fn find_color_for_column(
    columns: &HashMap<String, usize>,
    colors: &HashMap<String, Color>,
    col: usize,
) -> Color {
    for (name, &c) in columns {
        if c == col {
            if let Some(&color) = colors.get(name) {
                return color;
            }
        }
    }
    Color::White
}

/// Assign columns using DFS traversal
/// Each branch gets its own column, spreading to the right
fn assign_columns_dfs(
    stack: &Stack,
    branch: &str,
    next_column: &mut usize,
    columns: &mut HashMap<String, usize>,
    colors: &mut HashMap<String, Color>,
    _is_first_child: bool,
) {
    // Each branch gets its own column, spreading right
    let my_column = *next_column;
    *next_column += 1;

    columns.insert(branch.to_string(), my_column);
    colors.insert(branch.to_string(), BRANCH_COLORS[my_column % BRANCH_COLORS.len()]);

    // Process children in sorted order
    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<_> = info.children.iter().collect();
        children.sort();

        for child in children {
            assign_columns_dfs(stack, child, next_column, columns, colors, false);
        }
    }
}

/// Collect branches with depth information
#[allow(clippy::too_many_arguments)]
fn collect_branches_with_depth(
    stack: &Stack,
    branch: &str,
    depth: usize,
    current: &str,
    remote_branches: &HashSet<String>,
    columns: &HashMap<String, usize>,
    colors: &HashMap<String, Color>,
    workdir: &std::path::Path,
    result: &mut Vec<BranchDisplay>,
) {
    // Skip trunk itself (we render it separately)
    if branch == stack.trunk {
        if let Some(info) = stack.branches.get(branch) {
            for child in &info.children {
                collect_branches_with_depth(
                    stack, child, 1, current, remote_branches,
                    columns, colors, workdir, result,
                );
            }
        }
        return;
    }

    let info = stack.branches.get(branch);

    // Get commits ahead
    let commits_ahead = info
        .and_then(|i| i.parent.as_ref())
        .and_then(|parent| get_commits_ahead(workdir, parent, branch));

    result.push(BranchDisplay {
        name: branch.to_string(),
        column: *columns.get(branch).unwrap_or(&0),
        color: *colors.get(branch).unwrap_or(&Color::White),
        is_current: branch == current,
        has_remote: remote_branches.contains(branch),
        commits_ahead,
        pr_number: info.and_then(|i| i.pr_number),
        needs_restack: info.map(|i| i.needs_restack).unwrap_or(false),
        depth,
    });

    // Recurse into children
    if let Some(info) = info {
        for child in &info.children {
            collect_branches_with_depth(
                stack, child, depth + 1, current, remote_branches,
                columns, colors, workdir, result,
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::{Stack, StackBranch};

    fn make_test_stack(trunk: &str, branches: Vec<(&str, Option<&str>, Vec<&str>)>) -> Stack {
        let mut branch_map: HashMap<String, StackBranch> = HashMap::new();

        for (name, parent, children) in branches {
            branch_map.insert(
                name.to_string(),
                StackBranch {
                    name: name.to_string(),
                    parent: parent.map(|p| p.to_string()),
                    children: children.iter().map(|c| c.to_string()).collect(),
                    needs_restack: false,
                    pr_number: None,
                },
            );
        }

        Stack {
            branches: branch_map,
            trunk: trunk.to_string(),
        }
    }

    fn get_column_assignments(stack: &Stack) -> HashMap<String, usize> {
        let trunk_info = stack.branches.get(&stack.trunk);
        let mut trunk_children: Vec<String> = trunk_info
            .map(|b| b.children.clone())
            .unwrap_or_default();
        trunk_children.sort();

        let mut columns: HashMap<String, usize> = HashMap::new();
        let mut colors: HashMap<String, Color> = HashMap::new();
        let mut next_column: usize = 0;

        for root in &trunk_children {
            assign_columns_dfs(stack, root, &mut next_column, &mut columns, &mut colors, true);
        }

        columns
    }

    #[test]
    fn test_linear_chain_columns() {
        // main -> a -> b -> c
        // Each branch should get its own column (0, 1, 2)
        let stack = make_test_stack(
            "main",
            vec![
                ("main", None, vec!["a"]),
                ("a", Some("main"), vec!["b"]),
                ("b", Some("a"), vec!["c"]),
                ("c", Some("b"), vec![]),
            ],
        );

        let columns = get_column_assignments(&stack);

        // Each branch gets incrementing columns
        assert_eq!(columns.get("a"), Some(&0));
        assert_eq!(columns.get("b"), Some(&1));
        assert_eq!(columns.get("c"), Some(&2));
    }

    #[test]
    fn test_two_roots_columns() {
        // main -> stack1 -> s1child
        // main -> stack2
        let stack = make_test_stack(
            "main",
            vec![
                ("main", None, vec!["stack1", "stack2"]),
                ("stack1", Some("main"), vec!["s1child"]),
                ("s1child", Some("stack1"), vec![]),
                ("stack2", Some("main"), vec![]),
            ],
        );

        let columns = get_column_assignments(&stack);

        // DFS order: stack1 (0), s1child (1), stack2 (2)
        assert_eq!(columns.get("stack1"), Some(&0));
        assert_eq!(columns.get("s1child"), Some(&1));
        assert_eq!(columns.get("stack2"), Some(&2));
    }

    #[test]
    fn test_complex_tree_columns() {
        // Simulating fp ls structure:
        // main -> old-feature1 -> old-feature2 -> old-feature3
        // main -> debug-test
        // main -> stack1 -> stack2
        // main -> test-debug -> test-debug2
        let stack = make_test_stack(
            "main",
            vec![
                ("main", None, vec!["debug-test", "old-feature1", "stack1", "test-debug"]),
                ("old-feature1", Some("main"), vec!["old-feature2"]),
                ("old-feature2", Some("old-feature1"), vec!["old-feature3"]),
                ("old-feature3", Some("old-feature2"), vec![]),
                ("debug-test", Some("main"), vec![]),
                ("stack1", Some("main"), vec!["stack2"]),
                ("stack2", Some("stack1"), vec![]),
                ("test-debug", Some("main"), vec!["test-debug2"]),
                ("test-debug2", Some("test-debug"), vec![]),
            ],
        );

        let columns = get_column_assignments(&stack);

        // DFS order (sorted children): debug-test, old-feature1/2/3, stack1/2, test-debug/2
        // debug-test: 0
        // old-feature1: 1, old-feature2: 2, old-feature3: 3
        // stack1: 4, stack2: 5
        // test-debug: 6, test-debug2: 7
        assert_eq!(columns.get("debug-test"), Some(&0));
        assert_eq!(columns.get("old-feature1"), Some(&1));
        assert_eq!(columns.get("old-feature2"), Some(&2));
        assert_eq!(columns.get("old-feature3"), Some(&3));
        assert_eq!(columns.get("stack1"), Some(&4));
        assert_eq!(columns.get("stack2"), Some(&5));
        assert_eq!(columns.get("test-debug"), Some(&6));
        assert_eq!(columns.get("test-debug2"), Some(&7));
    }
}
