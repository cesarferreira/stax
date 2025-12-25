use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use std::process::Command;

struct DisplayBranch {
    name: String,
    column: usize,
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
                let workdir = repo.workdir()?;

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
                let mut next_column = 0;
                let mut sorted_trunk_children = trunk_children;
                sorted_trunk_children.sort_by(|a, b| {
                    let size_a = count_chain_size(&stack, a);
                    let size_b = count_chain_size(&stack, b);
                    size_b.cmp(&size_a).then_with(|| a.cmp(b))
                });

                for root in &sorted_trunk_children {
                    collect_display_branches_with_nesting(
                        &stack,
                        root,
                        next_column,
                        &mut display_branches,
                        &mut max_column,
                    );
                    next_column = max_column + 1;
                }

                // Build display items with proper alignment
                let mut items: Vec<String> = Vec::new();
                let mut branch_names: Vec<String> = Vec::new();
                let tree_target_width = (max_column + 1) * 2;

                for (i, db) in display_branches.iter().enumerate() {
                    let is_current = db.name == current;

                    // Check if we need a corner connector
                    let prev_branch_col =
                        if i > 0 { Some(display_branches[i - 1].column) } else { None };
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
                            tree.push_str("│ ");
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
                        if let Some(parent) = info.parent.as_deref() {
                            if let Some((ahead, behind)) =
                                get_commits_ahead_behind(workdir, parent, &db.name)
                            {
                                if ahead > 0 || behind > 0 {
                                    let mut parts = Vec::new();
                                    if ahead > 0 {
                                        parts.push(format!("{} ahead", ahead));
                                    }
                                    if behind > 0 {
                                        parts.push(format!("{} behind", behind));
                                    }
                                    display.push_str(&format!(" ({})", parts.join(", ")));
                                }
                            }
                        }

                        if info.needs_restack {
                            display.push_str(" ↻");
                        }
                        if let Some(pr) = info.pr_number {
                            let mut pr_text = format!(" PR #{}", pr);
                            if let Some(ref state) = info.pr_state {
                                pr_text.push_str(&format!(" {}", state.to_lowercase()));
                            }
                            display.push_str(&pr_text);
                        }
                    }

                    items.push(display);
                    branch_names.push(db.name.clone());
                }

                // Add trunk with matching style
                let is_trunk_current = stack.trunk == current;
                let mut trunk_tree = String::new();
                let mut trunk_visual_width = 0;
                let trunk_circle = if is_trunk_current { "◉" } else { "○" };

                trunk_tree.push_str(trunk_circle);
                trunk_visual_width += 1;

                if max_column >= 1 {
                    for col in 1..=max_column {
                        if col < max_column {
                            trunk_tree.push_str("─┴");
                        } else {
                            trunk_tree.push_str("─┘");
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
                    .highlight_matches(true)
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

fn get_commits_ahead_behind(
    workdir: &std::path::Path,
    parent: &str,
    branch: &str,
) -> Option<(usize, usize)> {
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
