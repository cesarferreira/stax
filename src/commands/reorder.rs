use crate::application::{NoopOperationReporter, RepositorySession};
use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;
use dialoguer::{Select, theme::ColorfulTheme};

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

    RepositorySession::open(repo.workdir()?)?.reorder_stack(
        &branches,
        &new_order,
        false,
        &mut NoopOperationReporter,
    )?;

    println!("{}", "Stack reordered.".green());

    Ok(())
}
