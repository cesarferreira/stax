use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

pub fn run(yes: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;

    if current == stack.trunk {
        anyhow::bail!("Cannot reorder from trunk. Checkout a stacked branch first.");
    }

    // Get the current stack (excluding trunk)
    let full_stack = stack.current_stack(&current);
    let branches: Vec<String> = full_stack
        .into_iter()
        .filter(|b| *b != stack.trunk)
        .collect();

    if branches.len() <= 1 {
        println!(
            "{}",
            "Only one branch in this stack. Nothing to reorder.".yellow()
        );
        return Ok(());
    }

    println!("{}", "Current stack order (bottom to top):".bold());
    for (i, b) in branches.iter().enumerate() {
        let marker = if *b == current { " (current)" } else { "" };
        println!("  {}. {}{}", i + 1, b.cyan(), marker.dimmed());
    }
    println!();

    // Interactive reorder loop
    let mut new_order = branches.clone();

    loop {
        println!("{}", "Pick a branch to move:".bold());
        let items: Vec<String> = new_order
            .iter()
            .enumerate()
            .map(|(i, b)| format!("{}. {}", i + 1, b))
            .collect();

        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select branch")
            .items(&items)
            .default(0)
            .interact_opt()?;

        let Some(from_idx) = selection else {
            println!("Cancelled.");
            return Ok(());
        };

        let branch_name = new_order.remove(from_idx);

        println!("Where to place '{}'?", branch_name.cyan());
        let positions: Vec<String> = (0..=new_order.len())
            .map(|i| {
                if i == 0 {
                    "Position 1 (bottom of stack)".to_string()
                } else if i == new_order.len() {
                    format!("Position {} (top of stack)", i + 1)
                } else {
                    format!("Position {} (after {})", i + 1, new_order[i - 1])
                }
            })
            .collect();

        let to_selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Select position")
            .items(&positions)
            .default(from_idx.min(new_order.len()))
            .interact_opt()?;

        let Some(to_idx) = to_selection else {
            new_order.insert(from_idx, branch_name);
            println!("Cancelled move.");
            continue;
        };

        new_order.insert(to_idx, branch_name);

        println!();
        println!("{}", "New order:".bold());
        for (i, b) in new_order.iter().enumerate() {
            println!("  {}. {}", i + 1, b.cyan());
        }
        println!();

        if new_order == branches {
            println!("{}", "Order unchanged.".yellow());
            return Ok(());
        }

        let done = if yes {
            true
        } else {
            let action = Select::with_theme(&ColorfulTheme::default())
                .with_prompt("What next?")
                .items(["Apply this order", "Move another branch", "Cancel"])
                .default(0)
                .interact()?;
            match action {
                0 => true,
                1 => continue,
                _ => {
                    println!("Cancelled.");
                    return Ok(());
                }
            }
        };

        if done {
            break;
        }
    }

    // Apply the new order by updating parent pointers
    let mut tx = Transaction::begin(OpKind::Reorder, &repo, false)?;
    for b in &new_order {
        tx.plan_branch(&repo, b)?;
    }
    tx.snapshot()?;

    let trunk = stack.trunk.clone();
    for (i, branch_name) in new_order.iter().enumerate() {
        let new_parent = if i == 0 {
            trunk.clone()
        } else {
            new_order[i - 1].clone()
        };

        let parent_rev = repo.branch_commit(&new_parent)?;
        let merge_base = repo
            .merge_base(&new_parent, branch_name)
            .unwrap_or_else(|_| parent_rev.clone());

        let existing = BranchMetadata::read(repo.inner(), branch_name)?;
        let updated = if let Some(meta) = existing {
            BranchMetadata {
                parent_branch_name: new_parent,
                parent_branch_revision: merge_base,
                ..meta
            }
        } else {
            BranchMetadata::new(&new_parent, &merge_base)
        };
        updated.write(repo.inner(), branch_name)?;

        tx.record_after(&repo, branch_name)?;
    }

    tx.finish_ok()?;

    println!("{}", "Stack reordered.".green());
    println!(
        "{}",
        "Run `stax restack --all` to rebase branches in the new order.".yellow()
    );

    Ok(())
}
