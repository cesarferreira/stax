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

            // Get ALL branches in display order (leaves first, trunk last)
            let stack_branches = stack.all_branches_display_order();

            if stack_branches.is_empty() {
                println!("No branches found.");
                return Ok(());
            }

            // Build display items with tree structure (like fp bco)
            let total = stack_branches.len();
            let items: Vec<String> = stack_branches
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let is_current = b == &current;
                    let is_last = i == total - 1;

                    // Build the prefix based on position (matching fp style)
                    let prefix = if is_last {
                        // Trunk (bottom) - corner piece
                        if is_current {
                            "◉─┘ "
                        } else {
                            "○─┘ "
                        }
                    } else if i == 0 {
                        // Top (leaf)
                        if is_current {
                            "◉   "
                        } else {
                            "○   "
                        }
                    } else {
                        // Middle - vertical connector with circle
                        if is_current {
                            "│ ◉ "
                        } else {
                            "│ ○ "
                        }
                    };

                    let mut display = format!("{}{}", prefix, b);

                    // Add tracked branch info
                    if let Some(info) = stack.branches.get(b) {
                        if info.needs_restack {
                            display.push_str(" (needs restack)");
                        }
                        if let Some(pr) = info.pr_number {
                            display.push_str(&format!(" #{}", pr));
                        }
                    }

                    display
                })
                .collect();

            let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
                .with_prompt("Checkout a branch (autocomplete or arrow keys)")
                .items(&items)
                .default(0)
                .highlight_matches(true)
                .interact()?;

            stack_branches[selection].clone()
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
