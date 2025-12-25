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

            // Get trunk children (each starts a chain)
            let trunk_info = stack.branches.get(&stack.trunk);
            let mut trunk_children: Vec<String> = trunk_info
                .map(|b| b.children.clone())
                .unwrap_or_default();
            trunk_children.sort();

            // Find the largest chain - only it gets vertical lines
            let mut largest_chain_root: Option<String> = None;
            let mut largest_chain_size = 0;
            for chain_root in trunk_children.iter() {
                let chain_size = count_chain_size(&stack, chain_root);
                if chain_size > largest_chain_size {
                    largest_chain_size = chain_size;
                    largest_chain_root = Some(chain_root.clone());
                }
            }

            // Collect branches from each chain
            for chain_root in trunk_children.iter() {
                // Only the largest chain (with 2+ branches) gets vertical lines
                let is_largest_multi = largest_chain_root.as_ref() == Some(chain_root) && largest_chain_size > 1;
                collect_stack_items(&stack, chain_root, &current, is_largest_multi, &mut items, &mut branch_names);
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
    is_multi_branch_chain: bool,
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

        // fp style: multi-branch chains at column 1 with vertical line, isolated branches at column 0
        let mut display = if is_multi_branch_chain {
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

fn count_chain_size(stack: &Stack, root: &str) -> usize {
    let mut count = 1;
    if let Some(info) = stack.branches.get(root) {
        for child in &info.children {
            count += count_chain_size(stack, child);
        }
    }
    count
}
