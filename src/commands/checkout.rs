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

            // Build items with fp-style tree structure
            let mut items = Vec::new();
            let mut branch_names = Vec::new();

            // Get trunk children (each starts a stack)
            let trunk_info = stack.branches.get(&stack.trunk);
            let mut trunk_children: Vec<String> = trunk_info
                .map(|b| b.children.clone())
                .unwrap_or_default();
            trunk_children.sort();

            // Find which stack contains the current branch
            let current_stack_root = find_stack_containing(&stack, &trunk_children, &current);

            // Collect branches from each stack
            for stack_root in trunk_children.iter() {
                let is_current_stack = current_stack_root.as_ref() == Some(stack_root);
                collect_stack_items(&stack, stack_root, &current, is_current_stack, &mut items, &mut branch_names);
            }

            // Add trunk
            let display = format!("○─┘  {}", stack.trunk);
            items.push(display);
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

fn collect_stack_items(
    stack: &Stack,
    branch: &str,
    current: &str,
    is_current_stack: bool,
    items: &mut Vec<String>,
    branch_names: &mut Vec<String>,
) {
    // Collect all branches in this stack
    let mut branches = Vec::new();
    collect_stack_branches(stack, branch, &mut branches);

    // Render from leaf to root
    for b in branches.iter() {
        let is_current = *b == current;
        let indicator = if is_current { "◉" } else { "○" };

        // fp style: "○    name" for non-current, "│ ○  name" for current
        let mut display = if is_current_stack {
            format!("│ {}  {}", indicator, b)
        } else {
            format!("{}    {}", indicator, b)
        };

        if let Some(info) = stack.branches.get(*b) {
            if info.needs_restack {
                display.push_str(" [needs restack]");
            }
            if let Some(pr) = info.pr_number {
                display.push_str(&format!(" PR #{}", pr));
            }
        }

        items.push(display);
        branch_names.push(b.to_string());
    }
}

fn collect_stack_branches<'a>(stack: &'a Stack, branch: &'a str, result: &mut Vec<&'a str>) {
    if let Some(info) = stack.branches.get(branch) {
        for child in &info.children {
            collect_stack_branches(stack, child, result);
        }
    }
    result.push(branch);
}

fn find_stack_containing(stack: &Stack, stack_roots: &[String], current: &str) -> Option<String> {
    for root in stack_roots {
        if branch_is_in_stack(stack, root, current) {
            return Some(root.clone());
        }
    }
    None
}

fn branch_is_in_stack(stack: &Stack, root: &str, target: &str) -> bool {
    if root == target {
        return true;
    }
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            if branch_is_in_stack(stack, child, target) {
                return true;
            }
        }
    }
    false
}
