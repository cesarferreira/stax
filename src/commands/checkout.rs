use crate::config::Config;
use crate::engine::Stack;
use crate::git::{refs, GitRepo};
use crate::remote;
use anyhow::Result;
use colored::{Color, Colorize};
use console::truncate_str;
use crossterm::terminal;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};
use std::collections::HashSet;

// Colors for different columns (matching status.rs)
const COLUMN_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Green,
    Color::Magenta,
    Color::Blue,
    Color::BrightCyan,
    Color::BrightGreen,
    Color::BrightMagenta,
    Color::BrightBlue,
];

/// Represents a branch in the display with its column position
struct DisplayBranch {
    name: String,
    column: usize,
}

struct CheckoutRow {
    branch: String,
    display: String,
}

pub fn run(branch: Option<String>, trunk: bool, parent: bool, child: Option<usize>) -> Result<()> {
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
                let trunk_children: Vec<String> =
                    trunk_info.map(|b| b.children.clone()).unwrap_or_default();

                if trunk_children.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let rows = build_checkout_rows(&stack, &repo, &current)?;

                if rows.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let items: Vec<String> = rows.iter().map(|r| r.display.clone()).collect();
                let branch_names: Vec<String> = rows.iter().map(|r| r.branch.clone()).collect();

                let mut theme = ColorfulTheme::default();
                theme.active_item_style = console::Style::new()
                    .for_stderr()
                    .black()
                    .on_white()
                    .bold();
                theme.active_item_prefix = console::style("▶".to_string())
                    .for_stderr()
                    .black()
                    .on_white()
                    .bold();
                theme.inactive_item_prefix =
                    console::style(" ".to_string()).for_stderr();

                let term = console::Term::stderr();
                if term.is_term() {
                    let _ = term.clear_screen();
                    let _ = term.move_cursor_to(0, 0);
                }

                let default_index = branch_names
                    .iter()
                    .position(|name| name == &current)
                    .unwrap_or(0);

                let selection = FuzzySelect::with_theme(&theme)
                    .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                    .items(&items)
                    .default(default_index)
                    .highlight_matches(false) // Disabled - conflicts with ANSI colors
                    .interact()?;

                branch_names[selection].clone()
            }
        }
    };

    if target == current {
        println!("Already on '{}'", target);
    } else {
        // Save current branch as previous before switching
        if let Err(e) = refs::write_prev_branch(repo.inner(), &current) {
            eprintln!("Warning: failed to save previous branch: {}", e);
        }
        repo.checkout(&target)?;
        println!("Switched to branch '{}'", target);
    }

    Ok(())
}

fn build_checkout_rows(stack: &Stack, repo: &GitRepo, current: &str) -> Result<Vec<CheckoutRow>> {
    let workdir = repo.workdir()?;
    let config = Config::load()?;
    let remote_branches = remote::get_remote_branches(workdir, config.remote_name())
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();

    let trunk_info = stack.branches.get(&stack.trunk);
    let trunk_children: Vec<String> = trunk_info
        .map(|b| b.children.clone())
        .unwrap_or_default()
        .into_iter()
        .collect();

    if trunk_children.is_empty() {
        return Ok(Vec::new());
    }

    let mut display_branches: Vec<DisplayBranch> = Vec::new();
    let mut max_column = 0;
    let mut sorted_trunk_children = trunk_children;
    sorted_trunk_children.sort();

    for (i, root) in sorted_trunk_children.iter().enumerate() {
        collect_display_branches_with_nesting(
            stack,
            root,
            i,
            &mut display_branches,
            &mut max_column,
        );
    }

    let tree_target_width = (max_column + 1) * 2;
    let max_width = terminal_width().saturating_sub(1);
    let mut rows = Vec::new();

    for (i, db) in display_branches.iter().enumerate() {
        let branch = &db.name;
        let is_current = branch == current;
        let entry = stack.branches.get(branch);
        let parent = entry.and_then(|b| b.parent.as_deref());
        let (ahead, behind) = parent
            .and_then(|p| repo.commits_ahead_behind(p, branch).ok())
            .unwrap_or((0, 0));
        let needs_restack = entry.map(|b| b.needs_restack).unwrap_or(false);
        let has_pr = entry.and_then(|b| b.pr_number).is_some();
        let has_remote = remote_branches.contains(branch) || has_pr;

        let prev_branch_col = if i > 0 {
            Some(display_branches[i - 1].column)
        } else {
            None
        };
        let needs_corner = prev_branch_col.is_some_and(|pc| pc > db.column);

        let mut tree = String::new();
        let mut visual_width = 0;
        for col in 0..=db.column {
            let col_color = COLUMN_COLORS[col % COLUMN_COLORS.len()];
            if col == db.column {
                let circle = if is_current { "◉" } else { "○" };
                tree.push_str(&format!("{}", circle.color(col_color)));
                visual_width += 1;
                if needs_corner {
                    tree.push_str(&format!("{}", "─┘".color(col_color)));
                    visual_width += 2;
                }
            } else {
                tree.push_str(&format!("{} ", "│".color(col_color)));
                visual_width += 2;
            }
        }

        while visual_width < tree_target_width {
            tree.push(' ');
            visual_width += 1;
        }

        let mut info_str = String::new();
        info_str.push(' ');
        if has_remote {
            info_str.push_str(&format!("{} ", "☁️".bright_blue()));
        } else {
            info_str.push_str("   ");
        }

        let branch_color = COLUMN_COLORS[db.column % COLUMN_COLORS.len()];
        if is_current {
            info_str.push_str(&format!("{}", branch.color(branch_color).bold()));
        } else {
            info_str.push_str(&format!("{}", branch.color(branch_color)));
        }

        if ahead > 0 || behind > 0 {
            if behind > 0 {
                info_str.push_str(&format!(" {}", format!("{} behind", behind).red()));
            }
            if ahead > 0 {
                info_str.push_str(&format!(" {}", format!("{} ahead", ahead).green()));
            }
        }
        if needs_restack {
            info_str.push_str(&format!(" {}", "(needs restack)".bright_yellow()));
        }

        let display = truncate_display(&format!("{}{}", tree, info_str), max_width);
        rows.push(CheckoutRow {
            branch: branch.clone(),
            display,
        });
    }

    // Render trunk row (matches status.rs)
    let is_trunk_current = stack.trunk == current;
    let trunk_child_max_col = if sorted_trunk_children.is_empty() {
        0
    } else {
        sorted_trunk_children.len() - 1
    };

    let mut trunk_tree = String::new();
    let mut trunk_visual_width = 0;
    let trunk_circle = if is_trunk_current { "◉" } else { "○" };
    let trunk_color = COLUMN_COLORS[0];
    trunk_tree.push_str(&format!("{}", trunk_circle.color(trunk_color)));
    trunk_visual_width += 1;

    if trunk_child_max_col >= 1 {
        for col in 1..=trunk_child_max_col {
            let col_color = COLUMN_COLORS[col % COLUMN_COLORS.len()];
            if col < trunk_child_max_col {
                trunk_tree.push_str(&format!("{}", "─┴".color(col_color)));
            } else {
                trunk_tree.push_str(&format!("{}", "─┘".color(col_color)));
            }
            trunk_visual_width += 2;
        }
    }

    while trunk_visual_width < tree_target_width {
        trunk_tree.push(' ');
        trunk_visual_width += 1;
    }

    let mut trunk_info = String::new();
    trunk_info.push(' ');
    if remote_branches.contains(&stack.trunk) {
        trunk_info.push_str(&format!("{} ", "☁️".bright_blue()));
    } else {
        trunk_info.push_str("   ");
    }
    if is_trunk_current {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color).bold()));
    } else {
        trunk_info.push_str(&format!("{}", stack.trunk.color(trunk_color)));
    }

    let (ahead, behind) = repo
        .commits_ahead_behind(&format!("{}/{}", config.remote_name(), stack.trunk), &stack.trunk)
        .unwrap_or((0, 0));
    if ahead > 0 || behind > 0 {
        if behind > 0 {
            trunk_info.push_str(&format!(" {}", format!("{} behind", behind).red()));
        }
        if ahead > 0 {
            trunk_info.push_str(&format!(" {}", format!("{} ahead", ahead).green()));
        }
    }

    let trunk_display = truncate_display(&format!("{}{}", trunk_tree, trunk_info), max_width);
    rows.push(CheckoutRow {
        branch: stack.trunk.clone(),
        display: trunk_display,
    });

    Ok(rows)
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    truncate_str(text, max_width, "...").into_owned()
}

fn terminal_width() -> usize {
    terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(120)
        .max(20)
}

/// Collect branches with proper nesting for branches that have multiple children
/// fp-style: children sorted alphabetically, each child gets column + index
fn collect_display_branches_with_nesting(
    stack: &Stack,
    branch: &str,
    base_column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    collect_recursive(stack, branch, base_column, result, max_column);
}

fn collect_recursive(
    stack: &Stack,
    branch: &str,
    column: usize,
    result: &mut Vec<DisplayBranch>,
    max_column: &mut usize,
) {
    *max_column = (*max_column).max(column);

    if let Some(info) = stack.branches.get(branch) {
        let mut children: Vec<&String> = info.children.iter().collect();

        if !children.is_empty() {
            children.sort();
            for (i, child) in children.iter().enumerate() {
                collect_recursive(stack, child, column + i, result, max_column);
            }
        }
    }

    result.push(DisplayBranch {
        name: branch.to_string(),
        column,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    fn test_stack() -> Stack {
        // main (trunk)
        // ├── auth
        // │   └── auth-api
        // │       └── auth-ui
        // └── hotfix
        let mut branches: HashMap<String, StackBranch> = HashMap::new();

        branches.insert(
            "auth".to_string(),
            StackBranch {
                name: "auth".to_string(),
                parent: Some("main".to_string()),
                children: vec!["auth-api".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "auth-api".to_string(),
            StackBranch {
                name: "auth-api".to_string(),
                parent: Some("auth".to_string()),
                children: vec!["auth-ui".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "auth-ui".to_string(),
            StackBranch {
                name: "auth-ui".to_string(),
                parent: Some("auth-api".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "hotfix".to_string(),
            StackBranch {
                name: "hotfix".to_string(),
                parent: Some("main".to_string()),
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                children: vec!["auth".to_string(), "hotfix".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn test_display_branch_order() {
        let stack = test_stack();
        let mut display_branches = Vec::new();
        let mut max_column = 0;
        let mut roots = vec!["auth".to_string(), "hotfix".to_string()];
        roots.sort();
        for (i, root) in roots.iter().enumerate() {
            collect_display_branches_with_nesting(
                &stack,
                root,
                i,
                &mut display_branches,
                &mut max_column,
            );
        }
        let names: Vec<_> = display_branches.iter().map(|b| b.name.as_str()).collect();
        assert_eq!(names, vec!["auth-ui", "auth-api", "auth", "hotfix"]);
    }

    #[test]
    fn test_truncate_display_caps_width() {
        let text = "• very-very-long-branch-name  stack  +12/-3  #123 ⟳";
        let truncated = truncate_display(text, 16);
        assert!(console::measure_text_width(&truncated) <= 16);
        assert!(truncated.ends_with("..."));
    }
}
