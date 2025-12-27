use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::{Color, Colorize};
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use std::collections::HashMap;

// Colors for different stacks (same as status.rs)
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

struct DisplayBranch {
    name: String,
    column: usize,
    stack_index: usize,
}

pub fn run(
    branch: Option<String>,
    trunk: bool,
    parent: bool,
    child: Option<usize>,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;

    if branch.is_some() && (trunk || parent || child.is_some()) {
        anyhow::bail!("Cannot combine explicit branch with --trunk/--parent/--child");
    }

    let target = if trunk || parent || child.is_some() {
        let stack = Stack::load(&repo)?;
        if trunk {
            stack.trunk.clone()
        } else if parent {
            let parent_branch = stack
                .branches
                .get(&current)
                .and_then(|b| b.parent.clone())
                .ok_or_else(|| anyhow::anyhow!("Branch '{}' has no tracked parent.", current))?;
            parent_branch
        } else {
            let children: Vec<String> = stack
                .branches
                .get(&current)
                .map(|b| b.children.clone())
                .unwrap_or_default();

            if children.is_empty() {
                anyhow::bail!("Branch '{}' has no tracked children.", current);
            }

            let idx = child.unwrap_or(1);
            if idx == 0 || idx > children.len() {
                anyhow::bail!("Child index {} out of range (1-{})", idx, children.len());
            }
            children[idx - 1].clone()
        }
    } else {
        match branch {
            Some(b) => b,
            None => {
                let stack = Stack::load(&repo)?;
                let _workdir = repo.workdir()?;

                if stack.branches.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                // Get trunk children (each starts a chain)
                let trunk_info = stack.branches.get(&stack.trunk);
                let trunk_children: Vec<String> = trunk_info
                    .map(|b| b.children.clone())
                    .unwrap_or_default();

                if trunk_children.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                // Build display list: each trunk child gets its own column, stacked left to right
                let mut display_branches: Vec<DisplayBranch> = Vec::new();
                let mut max_column = 0;
                let mut sorted_trunk_children = trunk_children;
                // Sort trunk children alphabetically (like fp)
                sorted_trunk_children.sort();

                // Each trunk child gets column = index (first at 0, second at 1, etc.)
                // stack_index tracks which stack each branch belongs to for coloring
                for (i, root) in sorted_trunk_children.iter().enumerate() {
                    collect_display_branches_with_nesting(
                        &stack,
                        root,
                        i,  // column
                        i,  // stack_index
                        &mut display_branches,
                        &mut max_column,
                    );
                }

                // Build display items with proper alignment
                let mut items: Vec<String> = Vec::new();
                let mut branch_names: Vec<String> = Vec::new();
                let tree_target_width = (max_column + 1) * 2;

                // Build column -> stack_index mapping for vertical line coloring
                let mut column_stack: HashMap<usize, usize> = HashMap::new();
                for db in &display_branches {
                    column_stack.insert(db.column, db.stack_index);
                }

                for (i, db) in display_branches.iter().enumerate() {
                    let is_current = db.name == current;
                    let color = DEPTH_COLORS[db.stack_index % DEPTH_COLORS.len()];

                    // Check if we need a corner connector
                    let prev_branch_col =
                        if i > 0 { Some(display_branches[i - 1].column) } else { None };
                    let needs_corner = prev_branch_col.is_some_and(|pc| pc > db.column);

                    // Build tree graphics with stack colors
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
                            // Use stack color for vertical lines
                            let stack_idx = column_stack.get(&col).copied().unwrap_or(col);
                            let line_color = DEPTH_COLORS[stack_idx % DEPTH_COLORS.len()];
                            tree.push_str(&format!("{} ", "│".color(line_color)));
                            visual_width += 2;
                        }
                    }

                    // Pad to consistent width
                    while visual_width < tree_target_width {
                        tree.push(' ');
                        visual_width += 1;
                    }

                    // Build full display string (branch name stays white)
                    let mut display = format!("{} {}", tree, db.name);

                    if let Some(info) = stack.branches.get(&db.name) {
                        if let Some(parent) = info.parent.as_deref() {
                            if let Ok((ahead, behind)) =
                                repo.commits_ahead_behind(parent, &db.name)
                            {
                                if ahead > 0 {
                                    display.push_str(&format!(" {}↑", ahead));
                                }
                                if behind > 0 {
                                    display.push_str(&format!(" {}↓", behind));
                                }
                            }
                        }

                        if info.needs_restack {
                            display.push_str(" ⟳");
                        }
                    }

                    items.push(display);
                    branch_names.push(db.name.clone());
                }

                // Add trunk with matching style (trunk uses first color)
                let is_trunk_current = stack.trunk == current;
                let trunk_color = DEPTH_COLORS[0];
                let mut trunk_tree = String::new();
                let mut trunk_visual_width = 0;
                let trunk_circle = if is_trunk_current { "◉" } else { "○" };

                trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
                trunk_visual_width += 1;

                // fp-style: ○─┘ for 1 col, ○─┴─┘ for 2, ○─┴─┴─┘ for 3, etc.
                if max_column >= 1 {
                    for col in 1..=max_column {
                        if col < max_column {
                            trunk_tree.push_str(&format!("{}", "─┴".color(trunk_color)));
                        } else {
                            trunk_tree.push_str(&format!("{}", "─┘".color(trunk_color)));
                        }
                        trunk_visual_width += 2;
                    }
                }

                while trunk_visual_width < tree_target_width {
                    trunk_tree.push(' ');
                    trunk_visual_width += 1;
                }

                let trunk_display = format!("{} {}", trunk_tree, stack.trunk);
                items.push(trunk_display);
                branch_names.push(stack.trunk.clone());

                if items.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                    .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                    .items(&items)
                    .default(0)
                    .highlight_matches(false)  // Disabled - conflicts with ANSI colors
                    .interact()?;

                branch_names[selection].clone()
            }
        }
    };

    if target == repo.current_branch()? {
        println!("Already on '{}'", target);
    } else {
        repo.checkout(&target)?;
        println!("Switched to branch '{}'", target);
    }

    Ok(())
}

/// fp-style: children sorted alphabetically, each child gets column + index
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    base_column: usize,
    stack_index: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    collect_recursive(stack, branch, base_column, stack_index, result, max_column);
}

fn collect_recursive(
    stack: &Stack,
    branch: &str,
    column: usize,
    stack_index: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<&String> = info.children.iter().collect();

        if !children.is_empty() {
            // Sort children alphabetically (like fp)
            children.sort();

            // Each child gets column + index: first child at same column, second at +1, etc.
            for (i, child) in children.iter().enumerate() {
                collect_recursive(stack, child, column + i, stack_index, result, max_column);
            }
        }
    }

    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
        stack_index,
    });
}
