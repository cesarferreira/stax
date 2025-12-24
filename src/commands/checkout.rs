use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

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

            // Build items with tree structure
            let mut items = Vec::new();
            let mut branch_names = Vec::new();
            collect_branch_items(&stack, &stack.trunk, &current, 0, &mut items, &mut branch_names);

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

fn collect_branch_items(
    stack: &Stack,
    branch: &str,
    current: &str,
    depth: usize,
    items: &mut Vec<String>,
    branch_names: &mut Vec<String>,
) {
    let branch_info = stack.branches.get(branch);
    let is_current = branch == current;

    // Get children and process them first (so leaves are at top)
    let children: Vec<String> = branch_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    for child in children.iter().rev() {
        collect_branch_items(stack, child, current, depth + 1, items, branch_names);
    }

    // Build display string with ASCII pipe for consistent alignment
    let indent: String = (0..depth).map(|_| "|   ").collect();
    let indicator = if is_current { "*" } else { "o" };

    let mut display = format!("{}{} {}", indent, indicator, branch);

    if let Some(info) = branch_info {
        if info.needs_restack {
            display.push_str(" [needs restack]");
        }
        if let Some(pr) = info.pr_number {
            display.push_str(&format!(" PR #{}", pr));
        }
    }

    if is_current {
        display.push_str(" <");
    }

    items.push(display);
    branch_names.push(branch.to_string());
}
