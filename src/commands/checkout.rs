use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

struct DisplayBranch {
    name: String,
    column: usize,
}

pub fn run(branch: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;

    let target = match branch {
        Some(b) => b,
        None => {
            let stack = Stack::load(&repo)?;
            let current = repo.current_branch()?;

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

            // Find the largest chain - only it gets vertical lines at column 1+
            let mut largest_chain_root: Option<String> = None;
            let mut largest_chain_size = 0;
            for chain_root in &trunk_children {
                let size = count_chain_size(&stack, chain_root);
                if size > largest_chain_size {
                    largest_chain_size = size;
                    largest_chain_root = Some(chain_root.clone());
                }
            }

            // Build display list with proper tree structure
            let mut display_branches: Vec<DisplayBranch> = Vec::new();
            let mut max_column = 0;

            // Add isolated chains (not the largest) at column 0
            for chain_root in &trunk_children {
                if largest_chain_root.as_ref() != Some(chain_root) {
                    collect_display_branches(&stack, chain_root, 0, &mut display_branches);
                }
            }

            // Add the largest chain at column 1 (with proper nested columns)
            if let Some(ref root) = largest_chain_root {
                if largest_chain_size > 1 {
                    collect_display_branches_with_nesting(&stack, root, 1, &mut display_branches, &mut max_column);
                } else {
                    collect_display_branches(&stack, root, 0, &mut display_branches);
                }
            }

            // Build display items with colors and proper alignment
            let mut items: Vec<String> = Vec::new();
            let mut branch_names: Vec<String> = Vec::new();
            let tree_target_width = (max_column + 1) * 2;

            for (i, db) in display_branches.iter().enumerate() {
                let is_current = db.name == current;

                // Check if there are branches at column X below this row
                let has_below_at_col = |col: usize| -> bool {
                    if col == 0 && db.column > 0 {
                        true // Column 0 connects to trunk
                    } else {
                        display_branches[i + 1..].iter().any(|b| b.column == col)
                    }
                };

                // Check if we need a corner connector
                let prev_branch_col = if i > 0 { Some(display_branches[i - 1].column) } else { None };
                let needs_corner = prev_branch_col.map_or(false, |pc| pc > db.column);

                // Build tree graphics (plain text for dialoguer compatibility)
                let mut tree = String::new();
                let mut visual_width = 0;

                for col in 0..=db.column {
                    if col == db.column {
                        let circle = if is_current { "◉" } else { "○" };
                        tree.push_str(circle);
                        visual_width += 1;

                        if needs_corner {
                            tree.push_str("─┘");
                            visual_width += 2;
                        }
                    } else {
                        if has_below_at_col(col) {
                            tree.push_str("│ ");
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

                // Build full display string
                let mut display = format!("{} {}", tree, db.name);

                if let Some(info) = stack.branches.get(&db.name) {
                    if info.needs_restack {
                        display.push_str(" ↻");
                    }
                    if let Some(pr) = info.pr_number {
                        display.push_str(&format!(" PR #{}", pr));
                    }
                }

                items.push(display);
                branch_names.push(db.name.clone());
            }

            // Add trunk with matching style
            let mut trunk_tree = String::new();
            let mut trunk_visual_width = 0;

            trunk_tree.push_str("○");
            trunk_visual_width += 1;

            if max_column >= 1 {
                trunk_tree.push_str("─┘");
                trunk_visual_width += 2;
            }

            while trunk_visual_width < tree_target_width {
                trunk_tree.push(' ');
                trunk_visual_width += 1;
            }

            items.push(format!("{} {}", trunk_tree, stack.trunk));
            branch_names.push(stack.trunk.clone());

            if items.is_empty() {
                println!("No branches found.");
                return Ok(());
            }

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                .items(&items)
                .default(0)
                .highlight_matches(true)
                .interact()?;

            branch_names[selection].clone()
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
            // Multiple children - find the "main" child (largest subtree)
            let mut children_with_sizes: Vec<(&String, usize)> = children
                .iter()
                .map(|c| (c, count_chain_size(stack, c)))
                .collect();

            children_with_sizes.sort_by(|a, b| {
                b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0))
            });

            let main_child = children_with_sizes[0].0;
            let side_children: Vec<&String> = children_with_sizes[1..].iter().map(|(c, _)| *c).collect();

            // Process main child first
            collect_display_branches_with_nesting(stack, main_child, column, result, max_column);

            // Process side branches at column + 1
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

fn count_chain_size(stack: &Stack, root: &str) -> usize {
    let mut count = 1;
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            count += count_chain_size(stack, child);
        }
    }
    count
}
