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
            collect_branch_items(&stack, &stack.trunk, &current, &mut Vec::new(), &mut items, &mut branch_names);

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
    pipes: &mut Vec<bool>,
    items: &mut Vec<String>,
    branch_names: &mut Vec<String>,
) {
    let branch_info = stack.branches.get(branch);
    let is_current = branch == current;

    // Get children and process them first (so leaves are at top)
    let children: Vec<String> = branch_info
        .map(|b| b.children.clone())
        .unwrap_or_default();

    for (i, child) in children.iter().rev().enumerate() {
        let is_last_child = i == children.len() - 1;
        pipes.push(!is_last_child);
        collect_branch_items(stack, child, current, pipes, items, branch_names);
        pipes.pop();
    }

    // Build prefix from pipes
    let prefix: String = pipes.iter().map(|&has_pipe| if has_pipe { "|   " } else { "    " }).collect();
    let indicator = if is_current { "*" } else { "o" };

    let mut display = format!("{}{} {}", prefix, indicator, branch);

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
