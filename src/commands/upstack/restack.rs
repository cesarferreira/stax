use crate::commands::restack_conflict::{print_restack_conflict, RestackConflictContext};
use crate::config::Config;
use crate::engine::{restack_preflight, BranchMetadata, Stack};
use crate::errors::ConflictStopped;
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
    tx.set_auto_stash_pop(auto_stash_pop);
    tx.snapshot()?;

    let mut completed_branches = Vec::new();

    // Load the stack once and update in-memory after each rebase.
    let mut live_stack = Stack::load(&repo)?;

    for branch in &upstack {
        let needs_restack = live_stack
            .branches
            .get(branch)
            .map(|br| br.needs_restack)
            .unwrap_or(false);
        if !needs_restack {
            continue;
        }

        let (parent_branch_name, parent_branch_revision) = match live_stack.branches.get(branch) {
            Some(br) if br.parent.is_some() && br.parent_revision.is_some() => (
                br.parent.clone().unwrap(),
                br.parent_revision.clone().unwrap(),
            ),
            _ => match BranchMetadata::read(repo.inner(), branch)? {
                Some(m) => (m.parent_branch_name, m.parent_branch_revision),
                None => continue,
            },
        };

        println!("  {} onto {}", branch.white(), parent_branch_name.blue());

        let preflight_config = Config::load().unwrap_or_default();
        let rebase_upstream = restack_preflight::choose_rebase_upstream(
            &repo,
            &preflight_config,
            branch,
            &parent_branch_name,
            &parent_branch_revision,
            false,
        );

        match repo.rebase_branch_onto_with_provenance(
            branch,
            &parent_branch_name,
            &rebase_upstream,
            auto_stash_pop,
        )? {
            RebaseResult::Success => {
                let new_parent_rev = repo.branch_commit(&parent_branch_name)?;
                let updated_meta = BranchMetadata {
                    parent_branch_name: parent_branch_name.clone(),
                    parent_branch_revision: new_parent_rev.clone(),
                    pr_info: live_stack.branches.get(branch).and_then(|br| {
                        br.pr_number.map(|n| crate::engine::PrInfo {
                            number: n,
                            state: br.pr_state.clone().unwrap_or_default(),
                            is_draft: br.pr_is_draft,
                        })
                    }),
                };
                updated_meta.write(repo.inner(), branch)?;

                // Update in-memory stack
                if let Some(br) = live_stack.branches.get_mut(branch) {
                    br.needs_restack = false;
                    br.parent_revision = Some(new_parent_rev.clone());
                }
                let children: Vec<String> = live_stack
                    .branches
                    .get(branch)
                    .map(|br| br.children.clone())
                    .unwrap_or_default();
                for child in &children {
                    if let Some(child_br) = live_stack.branches.get_mut(child) {
                        child_br.needs_restack = child_br
                            .parent_revision
                            .as_deref()
                            .map(|rev| rev != new_parent_rev.as_str())
                            .unwrap_or(true);
                    }
                }

                // Record the after-OID for this branch
                tx.record_after(&repo, branch)?;
                tx.push_completed_branch(branch);

                completed_branches.push(branch.clone());
                println!("    {}", "✓ done".green());
            }
            RebaseResult::Conflict => {
                println!("    {}", "✗ conflict".red());
                let conflict_stack = live_stack.current_stack(branch);
                print_restack_conflict(
                    &repo,
                    &RestackConflictContext {
                        branch,
                        parent_branch: &parent_branch_name,
                        completed_branches: &completed_branches,
                        remaining_branches: upstack
                            .iter()
                            .position(|candidate| candidate == branch)
                            .map(|index| upstack.len().saturating_sub(index + 1))
                            .unwrap_or(0),
                        continue_commands: &["stax resolve", "stax continue"],
                        stack_branches: &conflict_stack,
                    },
                );

                // Finish transaction with error
                tx.finish_err("Rebase conflict", Some("rebase"), Some(branch))?;

                return Err(ConflictStopped.into());
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
