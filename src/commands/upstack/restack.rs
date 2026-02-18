use crate::config::Config;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::Result;
use colored::Colorize;

pub fn run(auto_stash_pop: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let current = repo.current_branch()?;
    let stack = Stack::load(&repo)?;

    // Scope is current branch + descendants (excluding trunk); evaluate
    // restack status live per branch while walking this order.
    let mut upstack = vec![current.clone()];
    upstack.extend(stack.descendants(&current));
    upstack.retain(|b| b != &stack.trunk);

    let branches_to_restack = branches_needing_restack(&stack, &upstack);

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
                    "  Tip: '{}' itself needs restack. Run {} to include it.",
                    current,
                    "stax restack".cyan()
                );
            }
        } else {
            println!("{}", "✓ Upstack is up to date, nothing to restack.".green());
        }
        return Ok(());
    }

    let branch_word = if upstack.len() == 1 {
        "branch"
    } else {
        "branches"
    };
    println!(
        "Restacking up to {} {}...",
        upstack.len().to_string().cyan(),
        branch_word
    );

    // Begin transaction
    let mut tx = Transaction::begin(OpKind::UpstackRestack, &repo, false)?;
    tx.plan_branches(&repo, &upstack)?;
    let summary = PlanSummary {
        branches_to_rebase: upstack.len(),
        branches_to_push: 0,
        description: vec![format!(
            "Upstack restack up to {} {}",
            upstack.len(),
            branch_word
        )],
    };
    tx::print_plan(tx.kind(), &summary, false);
    tx.set_plan_summary(summary);
    tx.snapshot()?;

    for branch in &upstack {
        let live_stack = Stack::load(&repo)?;
        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|br| br.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        let meta = match BranchMetadata::read(repo.inner(), branch)? {
            Some(m) => m,
            None => continue,
        };

        println!(
            "  {} onto {}",
            branch.white(),
            meta.parent_branch_name.blue()
        );

        match repo.rebase_branch_onto(branch, &meta.parent_branch_name, auto_stash_pop)? {
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
                tx.finish_err("Rebase conflict", Some("rebase"), Some(branch))?;

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
