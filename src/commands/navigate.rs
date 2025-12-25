use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Select};

/// Move up the stack (to a child branch)
pub fn up(index: Option<usize>) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get children of current branch
    let children: Vec<String> = stack
        .branches
        .get(&current)
        .map(|b| b.children.clone())
        .unwrap_or_default();

    if children.is_empty() {
        println!(
            "{}",
            "Already at the top of the stack (no child branches).".dimmed()
        );
        return Ok(());
    }

    let target = if let Some(idx) = index {
        if idx == 0 || idx > children.len() {
            anyhow::bail!(
                "Child index {} out of range (1-{})",
                idx,
                children.len()
            );
        }
        children[idx - 1].clone()
    } else if children.len() == 1 {
        children[0].clone()
    } else {
        // Multiple children - let user choose
        let selection = Select::with_theme(&ColorfulTheme::default())
            .with_prompt("Multiple child branches - select one")
            .items(&children)
            .default(0)
            .interact()?;
        children[selection].clone()
    };

    repo.checkout(&target)?;
    println!("Switched to branch '{}'", target.bright_cyan());

    Ok(())
}

/// Move down the stack (to parent branch)
pub fn down() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get parent of current branch
    let parent = stack
        .branches
        .get(&current)
        .and_then(|b| b.parent.clone());

    match parent {
        Some(target) => {
            repo.checkout(&target)?;
            println!("Switched to branch '{}'", target.bright_cyan());
        }
        None => {
            if current == stack.trunk {
                println!(
                    "{}",
                    "Already at the bottom of the stack (on trunk).".dimmed()
                );
            } else {
                bail!("Branch '{}' has no tracked parent.", current);
            }
        }
    }

    Ok(())
}
