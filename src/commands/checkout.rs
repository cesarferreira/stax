use crate::engine::Stack;
use crate::git::GitRepo;
use anyhow::Result;
use dialoguer::{theme::ColorfulTheme, FuzzySelect};

pub fn run(branch: Option<String>) -> Result<()> {
    let repo = GitRepo::open()?;

    let target = match branch {
        Some(b) => b,
        None => {
            // Interactive selection with fuzzy finder
            let stack = Stack::load(&repo)?;
            let current = repo.current_branch()?;
            let trunk = repo.trunk_branch()?;

            // Get ALL local branches
            let mut all_branches = repo.list_branches()?;
            all_branches.sort();

            // Move trunk to the end, current to top
            all_branches.retain(|b| b != &trunk && b != &current);

            // Put current first if it exists
            let mut branches = vec![];
            if all_branches.iter().any(|b| b == &current) || current != trunk {
                branches.push(current.clone());
            }
            branches.extend(all_branches);
            branches.push(trunk.clone());

            if branches.is_empty() {
                println!("No branches found.");
                return Ok(());
            }

            // Build display items with indicators
            let items: Vec<String> = branches
                .iter()
                .map(|b| {
                    let mut display = b.clone();

                    // Add tracked branch info
                    if let Some(info) = stack.branches.get(b) {
                        if info.needs_restack {
                            display.push_str(" ⚠ restack");
                        }
                        if let Some(pr) = info.pr_number {
                            display.push_str(&format!(" PR#{}", pr));
                        }
                    }

                    // Mark current branch
                    if b == &current {
                        display.push_str(" ◀ current");
                    }

                    // Mark trunk
                    if b == &trunk {
                        display.push_str(" (trunk)");
                    }

                    display
                })
                .collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Switch to branch (type to filter)")
                .items(&items)
                .default(0)
                .highlight_matches(true)
                .interact()?;

            branches[selection].clone()
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
