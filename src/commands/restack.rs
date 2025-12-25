use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

pub fn run(all: bool, r#continue: bool, quiet: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    if r#continue {
        crate::commands::continue_cmd::run()?;
        if repo.rebase_in_progress()? {
            return Ok(());
        }
    }

    let mut stashed = false;
    if repo.is_dirty()? {
        if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        }

        let stash = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt("Working tree has uncommitted changes. Stash them before restack?")
            .default(true)
            .interact()?;

        if stash {
            stashed = repo.stash_push()?;
            println!("{}", "✓ Stashed working tree changes.".green());
        } else {
            println!("{}", "Aborted.".red());
            return Ok(());
        }
    }

    // Determine which branches to restack
    let branches_to_restack: Vec<String> = if all {
        stack.needs_restack()
    } else {
        // Just the current branch's stack
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| {
                stack
                    .branches
                    .get(b)
                    .map(|br| br.needs_restack)
                    .unwrap_or(false)
            })
            .collect()
    };

    if branches_to_restack.is_empty() {
        if !quiet {
            println!("{}", "✓ Stack is up to date, nothing to restack.".green());
        }
        if stashed {
            repo.stash_pop()?;
        }
        return Ok(());
    }

    if !quiet {
        println!(
            "Restacking {} branch(es)...",
            branches_to_restack.len().to_string().cyan()
        );
    }

    let mut summary: Vec<(String, String)> = Vec::new();

    for branch in &branches_to_restack {
        // Get metadata
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        if !quiet {
            println!("  {} onto {}", branch.white(), meta.parent_branch_name.blue());
        }

        // Checkout the branch
        repo.checkout(branch)?;

        // Rebase onto parent
        match repo.rebase(&meta.parent_branch_name)? {
            RebaseResult::Success => {
                // Update metadata with new parent revision
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), branch)?;
                if !quiet {
                    println!("    {}", "✓ done".green());
                }
                summary.push((branch.clone(), "ok".to_string()));
            }
            RebaseResult::Conflict => {
                if !quiet {
                    println!("    {}", "✗ conflict".red());
                    println!();
                    println!("{}", "Resolve conflicts and run:".yellow());
                    println!("  {}", "stax continue".cyan());
                    println!("  {}", "stax restack --continue".cyan());
                }
                if stashed && !quiet {
                    println!("{}", "Stash kept to avoid conflicts.".yellow());
                }
                summary.push((branch.clone(), "conflict".to_string()));
                return Ok(());
            }
        }
    }

    // Return to original branch
    repo.checkout(&current)?;

    if !quiet {
        println!();
        println!("{}", "✓ Stack restacked successfully!".green());
    }

    if !quiet && !summary.is_empty() {
        println!();
        println!("{}", "Restack summary:".dimmed());
        for (branch, status) in &summary {
            let symbol = if status == "ok" { "✓" } else { "✗" };
            println!("  {} {} {}", symbol, branch, status);
        }
    }

    // Check for merged branches and offer to delete them
    cleanup_merged_branches(&repo, quiet)?;

    if stashed {
        repo.stash_pop()?;
        if !quiet {
            println!("{}", "✓ Restored stashed changes.".green());
        }
    }

    Ok(())
}

/// Check for merged branches and prompt to delete each one
fn cleanup_merged_branches(repo: &GitRepo, quiet: bool) -> Result<()> {
    if quiet {
        return Ok(());
    }

    let merged = repo.merged_branches()?;

    if merged.is_empty() {
        return Ok(());
    }

    println!();
    println!(
        "{}",
        format!("Found {} merged branch(es):", merged.len()).dimmed()
    );

    for branch in &merged {
        let confirm = Confirm::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Delete '{}'?", branch.yellow()))
            .default(true)
            .interact()?;

        if confirm {
            // Delete the branch
            repo.delete_branch(branch, true)?;

            // Delete metadata if it exists
            let _ = BranchMetadata::delete(repo.inner(), branch);

            println!("  {} {}", "✓".green(), format!("Deleted {}", branch).dimmed());
        } else {
            println!("  {} {}", "○".dimmed(), format!("Skipped {}", branch).dimmed());
        }
    }

    Ok(())
}
