use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::Result;
use colored::Colorize;
use dialoguer::{theme::ColorfulTheme, Confirm};

pub fn run(all: bool, r#continue: bool, quiet: bool, auto_stash_pop: bool) -> Result<()> {
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
        if auto_stash_pop {
            stashed = repo.stash_push()?;
            if stashed && !quiet {
                println!("{}", "✓ Stashed working tree changes.".green());
            }
        } else if quiet {
            anyhow::bail!("Working tree is dirty. Please stash or commit changes first.");
        } else {
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
    }

    // Determine the operation scope once, then evaluate restack status live per branch.
    let mut scope_branches: Vec<String> = if all {
        stack
            .branches
            .keys()
            .filter(|b| *b != &stack.trunk)
            .cloned()
            .collect()
    } else {
        // Current stack: ancestors + current + descendants, excluding trunk.
        stack
            .current_stack(&current)
            .into_iter()
            .filter(|b| b != &stack.trunk)
            .collect()
    };

    if all {
        // Parent-first ordering minimizes repeated rebases across unrelated stacks.
        scope_branches.sort_by(|a, b| {
            stack
                .ancestors(a)
                .len()
                .cmp(&stack.ancestors(b).len())
                .then_with(|| a.cmp(b))
        });
    }

    let branches_to_restack = branches_needing_restack(&stack, &scope_branches);

    if branches_to_restack.is_empty() {
        if !quiet {
            println!("{}", "✓ Stack is up to date, nothing to restack.".green());
        }
        if stashed {
            repo.stash_pop()?;
        }
        return Ok(());
    }

    let branch_word = if scope_branches.len() == 1 {
        "branch"
    } else {
        "branches"
    };
    if !quiet {
        println!(
            "Restacking up to {} {}...",
            scope_branches.len().to_string().cyan(),
            branch_word
        );
    }

    // Begin transaction
    let mut tx = Transaction::begin(OpKind::Restack, &repo, quiet)?;
    tx.plan_branches(&repo, &scope_branches)?;
    let summary = PlanSummary {
        branches_to_rebase: scope_branches.len(),
        branches_to_push: 0,
        description: vec![format!(
            "Restack up to {} {}",
            scope_branches.len(),
            branch_word
        )],
    };
    tx::print_plan(tx.kind(), &summary, quiet);
    tx.set_plan_summary(summary);
    tx.snapshot()?;

    let mut summary: Vec<(String, String)> = Vec::new();

    for branch in &scope_branches {
        let live_stack = Stack::load(&repo)?;
        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|br| br.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        // Get metadata
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        if !quiet {
            println!(
                "  {} onto {}",
                branch.white(),
                meta.parent_branch_name.blue()
            );
        }

        // Rebase onto parent in the branch's own worktree when needed.
        match repo.rebase_branch_onto(branch, &meta.parent_branch_name, auto_stash_pop)? {
            RebaseResult::Success => {
                // Update metadata with new parent revision
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), branch)?;

                // Record the after-OID for this branch
                tx.record_after(&repo, branch)?;

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

                // Finish transaction with error
                tx.finish_err("Rebase conflict", Some("rebase"), Some(branch))?;

                return Ok(());
            }
        }
    }

    // Return to original branch
    repo.checkout(&current)?;

    // Finish transaction successfully
    tx.finish_ok()?;

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

fn branches_needing_restack(stack: &Stack, scope: &[String]) -> Vec<String> {
    scope
        .iter()
        .filter(|branch| {
            stack
                .branches
                .get(*branch)
                .map(|b| b.needs_restack)
                .unwrap_or(false)
        })
        .cloned()
        .collect()
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
        format!(
            "Found {} merged {}:",
            merged.len(),
            if merged.len() == 1 {
                "branch"
            } else {
                "branches"
            }
        )
        .dimmed()
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

            println!(
                "  {} {}",
                "✓".green(),
                format!("Deleted {}", branch).dimmed()
            );
        } else {
            println!(
                "  {} {}",
                "○".dimmed(),
                format!("Skipped {}", branch).dimmed()
            );
        }
    }

    Ok(())
}
