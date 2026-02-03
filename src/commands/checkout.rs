use crate::engine::Stack;
use crate::git::{refs, GitRepo};
use anyhow::Result;
use crossterm::terminal;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

struct RowData {
    branch: String,
    depth: usize,
    stack_root: String,
    delta: Option<(usize, usize)>,
    pr_number: Option<u64>,
    needs_restack: bool,
    is_current: bool,
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

                let rows = build_checkout_row_data(&stack, &current, |parent, branch| {
                    repo.commits_ahead_behind(parent, branch).ok()
                });
                let rows = format_checkout_rows(&rows);

                if rows.is_empty() {
                    println!("No branches found.");
                    return Ok(());
                }

                let items: Vec<String> = rows.iter().map(|r| r.display.clone()).collect();
                let branch_names: Vec<String> = rows.iter().map(|r| r.branch.clone()).collect();

                let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                    .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                    .items(&items)
                    .default(0)
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

fn branch_prefix(depth: usize, is_current: bool) -> String {
    let mut prefix = String::new();
    if depth > 0 {
        for _ in 0..depth {
            prefix.push_str("│ ");
        }
    }
    let glyph = if is_current {
        "●"
    } else if depth == 0 {
        "•"
    } else {
        "○"
    };
    prefix.push_str(glyph);
    prefix.push(' ');
    prefix
}

fn build_checkout_row_data<F>(stack: &Stack, current: &str, ahead_behind: F) -> Vec<RowData>
where
    F: Fn(&str, &str) -> Option<(usize, usize)>,
{
    let mut rows = Vec::new();
    let trunk = stack.trunk.as_str();

    let trunk_children = stack
        .branches
        .get(trunk)
        .map(|b| b.children.clone())
        .unwrap_or_default();

    if trunk_children.is_empty() {
        return rows;
    }

    let mut roots = trunk_children;
    roots.sort();

    if let Some(current_root) = stack_root_for(stack, current) {
        if let Some(pos) = roots.iter().position(|r| r == &current_root) {
            let root = roots.remove(pos);
            roots.insert(0, root);
        }
    }

    for root in roots {
        collect_stack_rows(
            stack,
            &root,
            0,
            current,
            &root,
            &ahead_behind,
            &mut rows,
        );
    }

    if stack.branches.contains_key(trunk) {
        rows.push(RowData {
            branch: trunk.to_string(),
            depth: 0,
            stack_root: "trunk".to_string(),
            delta: None,
            pr_number: None,
            needs_restack: false,
            is_current: current == trunk,
        });
    }

    rows
}

fn stack_root_for(stack: &Stack, branch: &str) -> Option<String> {
    if branch == stack.trunk {
        return None;
    }

    let mut current = branch.to_string();
    loop {
        let info = stack.branches.get(&current)?;
        match info.parent.as_deref() {
            Some(parent) if parent == stack.trunk => return Some(current),
            Some(parent) => {
                if !stack.branches.contains_key(parent) {
                    return Some(current);
                }
                current = parent.to_string();
            }
            None => return None,
        }
    }
}

fn collect_stack_rows<F>(
    stack: &Stack,
    branch: &str,
    depth: usize,
    current: &str,
    stack_root: &str,
    ahead_behind: &F,
    rows: &mut Vec<RowData>,
) where
    F: Fn(&str, &str) -> Option<(usize, usize)>,
{
    let Some(info) = stack.branches.get(branch) else {
        return;
    };

    let delta = info
        .parent
        .as_deref()
        .and_then(|parent| ahead_behind(parent, branch));

    rows.push(RowData {
        branch: info.name.clone(),
        depth,
        stack_root: stack_root.to_string(),
        delta,
        pr_number: info.pr_number,
        needs_restack: info.needs_restack,
        is_current: branch == current,
    });

    let mut children = info.children.clone();
    children.sort();
    for child in children {
        collect_stack_rows(
            stack,
            &child,
            depth + 1,
            current,
            stack_root,
            ahead_behind,
            rows,
        );
    }
}

fn format_checkout_rows(rows: &[RowData]) -> Vec<CheckoutRow> {
    let mut branch_width = 0;
    let mut stack_width = 0;
    let mut delta_width = 0;
    let mut pr_width = 0;
    let max_width = terminal_width().saturating_sub(1);
    let mut columns: Vec<(String, String, String, String, String)> = Vec::new();

    for row in rows {
        let branch_text = format!("{}{}", branch_prefix(row.depth, row.is_current), row.branch);
        let stack_text = row.stack_root.clone();
        let delta_text = match row.delta {
            Some((ahead, behind)) => format!("+{}/-{}", ahead, behind),
            None => "—".to_string(),
        };

        let mut pr_text = match row.pr_number {
            Some(number) => format!("#{}", number),
            None => "—".to_string(),
        };
        if row.needs_restack {
            pr_text.push_str(" ⟳");
        }

        branch_width = branch_width.max(display_width(&branch_text));
        stack_width = stack_width.max(display_width(&stack_text));
        delta_width = delta_width.max(display_width(&delta_text));
        pr_width = pr_width.max(display_width(&pr_text));

        columns.push((branch_text, stack_text, delta_text, pr_text, row.branch.clone()));
    }

    columns
        .into_iter()
        .map(|(branch_text, stack_text, delta_text, pr_text, branch_name)| {
            let display = format!(
                "{}  {}  {}  {}",
                pad_to_width(&branch_text, branch_width),
                pad_to_width(&stack_text, stack_width),
                pad_to_width(&delta_text, delta_width),
                pad_to_width(&pr_text, pr_width),
            );
            let display = truncate_display(&display, max_width);
            CheckoutRow {
                branch: branch_name,
                display,
            }
        })
        .collect()
}

fn pad_to_width(text: &str, width: usize) -> String {
    let mut padded = text.to_string();
    let padding = width.saturating_sub(display_width(text));
    if padding > 0 {
        padded.push_str(&" ".repeat(padding));
    }
    padded
}

fn display_width(s: &str) -> usize {
    s.chars().map(char_width).sum()
}

fn truncate_display(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if display_width(text) <= max_width {
        return text.to_string();
    }
    if max_width <= 3 {
        return ".".repeat(max_width);
    }

    let mut out = String::new();
    let mut used = 0;
    let limit = max_width - 3;
    for ch in text.chars() {
        let w = char_width(ch);
        if used + w > limit {
            break;
        }
        out.push(ch);
        used += w;
    }
    out.push_str("...");
    out
}

fn terminal_width() -> usize {
    terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(120)
        .max(20)
}

fn char_width(c: char) -> usize {
    match c {
        '\x00'..='\x1f' | '\x7f' => 0,
        '\x20'..='\x7e' => 1,
        '─' | '│' | '┌' | '┐' | '└' | '┘' | '├' | '┤' | '┬' | '┴' | '┼' | '╭' | '╮' | '╯'
        | '╰' | '║' | '═' | '•' | '○' | '●' | '⟳' => 1,
        _ => 2,
    }
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
    fn test_branch_prefix_depths() {
        assert_eq!(branch_prefix(0, false), "• ");
        assert_eq!(branch_prefix(1, false), "│ ○ ");
        assert_eq!(branch_prefix(2, false), "│ │ ○ ");
        assert_eq!(branch_prefix(1, true), "│ ● ");
    }

    #[test]
    fn test_checkout_row_order_current_stack_first() {
        let stack = test_stack();
        let rows = build_checkout_row_data(&stack, "auth-ui", |_p, _b| Some((0, 0)));
        let names: Vec<_> = rows.iter().map(|r| r.branch.as_str()).collect();
        assert_eq!(names, vec!["auth", "auth-api", "auth-ui", "hotfix", "main"]);
    }

    #[test]
    fn test_truncate_display_caps_width() {
        let text = "• very-very-long-branch-name  stack  +12/-3  #123 ⟳";
        let truncated = truncate_display(text, 16);
        assert!(display_width(&truncated) <= 16);
        assert!(truncated.ends_with("..."));
    }
}
