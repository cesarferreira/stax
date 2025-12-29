use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::Transaction;
use anyhow::Result;
use colored::Colorize;

pub fn run() -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Get current branch + descendants that need restacking
    let mut upstack = vec![current.clone()];
    upstack.extend(stack.descendants(&current));

    let branches_to_restack: Vec<String> = upstack
        .into_iter()
        .filter(|b| {
            stack
                .branches
                .get(b)
                .map(|br| br.needs_restack)
                .unwrap_or(false)
        })
        .collect();

    if branches_to_restack.is_empty() {
        // Check if the current branch itself needs restacking
        let current_needs_restack = stack
            .branches
            .get(&current)
            .map(|b| b.needs_restack)
            .unwrap_or(false);

        if current_needs_restack {
            println!("{}", "✓ No descendants need restacking.".green());
            let config = Config::load().unwrap_or_default();
            if config.ui.tips {
                println!(
                    "{}",
                    format!(
                        "  Tip: '{}' itself needs restack. Run {} to include it.",
                        current,
                        "stax restack".cyan()
                    )
                );
            }
        } else {
            println!("{}", "✓ Upstack is up to date, nothing to restack.".green());
        }
        return Ok(());
    }

    let branch_word = if branches_to_restack.len() == 1 { "branch" } else { "branches" };
    println!(
        "Restacking {} {}...",
        branches_to_restack.len().to_string().cyan(),
        branch_word
    );

    // Begin transaction
    let mut tx = Transaction::begin(OpKind::UpstackRestack, &repo, false)?;
    tx.plan_branches(&repo, &branches_to_restack)?;
    tx.set_plan_summary(PlanSummary {
        branches_to_rebase: branches_to_restack.len(),
        branches_to_push: 0,
        description: vec![format!("Upstack restack {} {}", branches_to_restack.len(), branch_word)],
    });
    tx.snapshot()?;

    for branch in &branches_to_restack {
        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        println!(
            "  {} onto {}",
            branch.white(),
            meta.parent_branch_name.blue()
        );

        repo.checkout(branch)?;

        match repo.rebase(&meta.parent_branch_name)? {
            RebaseResult::Success => {
                let new_parent_rev = repo.branch_commit(&meta.parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_revision: new_parent_rev,
                    ..meta
                };
                updated_meta.write(repo.inner(), branch)?;
                
                // Record the after-OID for this branch
                tx.record_after(&repo, branch)?;
                
                println!("    {}", "✓ done".green());
            }
            RebaseResult::Conflict => {
                println!("    {}", "✗ conflict".red());
                println!();
                println!("{}", "Resolve conflicts and run:".yellow());
                println!("  {}", "stax continue".cyan());
                
                // Finish transaction with error
                tx.finish_err(
                    "Rebase conflict",
                    Some("rebase"),
                    Some(branch),
                )?;
                
                return Ok(());
            }
        }
    }

    // Return to original branch
    repo.checkout(&current)?;

    // Finish transaction successfully
    tx.finish_ok()?;

    println!();
    println!("{}", "✓ Upstack restacked successfully!".green());

    Ok(())
}
