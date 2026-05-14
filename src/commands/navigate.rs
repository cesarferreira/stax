use crate::engine::Stack;
use crate::git::{checkout_branch_in, local_branch_exists_in, refs, GitRepo};
use anyhow::{bail, Result};
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

/// Move up the stack (to child branches)
/// If count > 1, moves up multiple branches
pub fn up(count: Option<usize>) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let mut current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let steps = count.unwrap_or(1);

    if steps == 0 {
        return Ok(());
    }

    for _ in 0..steps {
        // Get children of current branch
        let children: Vec<String> = stack
            .branches
            .get(&current)
            .map(|b| b.children.clone())
            .unwrap_or_default();

        if children.is_empty() {
            if current == repo.current_branch()? {
                println!(
                    "{}",
                    "Already at the top of the stack (no child branches).".dimmed()
                );
                return Ok(());
            } else {
                // We moved some steps but can't go further
                break;
            }
        }

        current = if children.len() == 1 {
            children[0].clone()
        } else {
            // Multiple children - let user choose
            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Multiple child branches - select one")
                .items(&children)
                .default(0)
                .interact()?;
            children[selection].clone()
        };
    }

    let previous = repo.current_branch()?;
    drop(repo);
    switch_branch(&workdir, &previous, &current)?;
    println!("Switched to branch '{}'", current.bright_cyan());

    Ok(())
}

/// Move down the stack (to parent branches)
/// If count > 1, moves down multiple branches
pub fn down(count: Option<usize>) -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let mut current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;
    let steps = count.unwrap_or(1);

    if steps == 0 {
        return Ok(());
    }

    for _ in 0..steps {
        // Get parent of current branch
        let parent = stack.branches.get(&current).and_then(|b| b.parent.clone());

        match parent {
            Some(p) => {
                current = p;
            }
            None => {
                if current == repo.current_branch()? {
                    if current == stack.trunk {
                        println!(
                            "{}",
                            "Already at the bottom of the stack (on trunk).".dimmed()
                        );
                    } else {
                        bail!("Branch '{}' has no tracked parent.", current);
                    }
                    return Ok(());
                }
                // We moved some steps but can't go further
                break;
            }
        }
    }

    let previous = repo.current_branch()?;
    drop(repo);
    switch_branch(&workdir, &previous, &current)?;
    println!("Switched to branch '{}'", current.bright_cyan());

    Ok(())
}

/// Move to the top of the stack (the tip/leaf branch)
pub fn top() -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let mut current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    loop {
        let children: Vec<String> = stack
            .branches
            .get(&current)
            .map(|b| b.children.clone())
            .unwrap_or_default();

        if children.is_empty() {
            break;
        }

        current = if children.len() == 1 {
            children[0].clone()
        } else {
            // Multiple children - let user choose
            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Multiple child branches - select one")
                .items(&children)
                .default(0)
                .interact()?;
            children[selection].clone()
        };
    }

    let original = repo.current_branch()?;
    if current == original {
        println!("{}", "Already at the top of the stack.".dimmed());
        return Ok(());
    }

    drop(repo);
    switch_branch(&workdir, &original, &current)?;
    println!("Switched to branch '{}'", current.bright_cyan());

    Ok(())
}

/// Move to the bottom of the stack (first branch above trunk)
pub fn bottom() -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    drop(repo);
    bottom_with_loaded_stack(&stack, &workdir, &current)
}

pub(crate) fn bottom_with_loaded_stack(
    stack: &Stack,
    workdir: &std::path::Path,
    current: &str,
) -> Result<()> {
    // Get the current stack and find the bottom (first branch above trunk)
    let current_stack = stack.current_stack(current);

    // Find the first branch that's not trunk
    let bottom_branch = current_stack.iter().find(|b| *b != &stack.trunk);

    match bottom_branch {
        Some(target) => {
            if target == current {
                println!("{}", "Already at the bottom of the stack.".dimmed());
                return Ok(());
            }
            switch_branch(workdir, current, target)?;
            println!("Switched to branch '{}'", target.bright_cyan());
        }
        None => {
            println!(
                "{}",
                "No branches above trunk in the current stack.".dimmed()
            );
        }
    }

    Ok(())
}

/// Switch to the previous branch (like git checkout -)
pub fn prev() -> Result<()> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    let current = repo.current_branch()?;

    let prev_branch = refs::read_prev_branch(repo.inner())?;

    match prev_branch {
        Some(target) => {
            if target == current {
                println!(
                    "{}",
                    "Previous branch is the same as current branch.".dimmed()
                );
                return Ok(());
            }

            // Verify the branch still exists
            if !local_branch_exists_in(&workdir, &target) {
                bail!("Previous branch '{}' no longer exists.", target);
            }

            drop(repo);
            switch_branch(&workdir, &current, &target)?;
            println!("Switched to branch '{}'", target.bright_cyan());
        }
        None => {
            println!(
                "{}",
                "No previous branch recorded. Use checkout, up, down, etc. first.".dimmed()
            );
        }
    }

    Ok(())
}

fn switch_branch(workdir: &std::path::Path, previous: &str, target: &str) -> Result<()> {
    let _ = refs::write_prev_branch_at(workdir, previous);
    checkout_branch_in(workdir, target)
}
