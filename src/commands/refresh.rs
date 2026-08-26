use crate::commands;
use crate::engine::Stack;
use crate::errors::ConflictStopped;
use crate::git::{GitRepo, local_branch_exists_in};
use anyhow::{Context, Result, bail};
use colored::Colorize;
use dialoguer::{Confirm, theme::ColorfulTheme};
use std::io::IsTerminal;

#[allow(clippy::too_many_arguments)]
pub fn run(
    no_pr: bool,
    no_submit: bool,
    force: bool,
    safe: bool,
    verbose: bool,
    yes: bool,
    no_prompt: bool,
    auto_stash_pop: bool,
    all_stacks: bool,
) -> Result<()> {
    if all_stacks {
        return run_all_stacks(
            no_pr,
            no_submit,
            force,
            safe,
            verbose,
            yes,
            no_prompt,
            auto_stash_pop,
        );
    }

    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;
    let submit_fetch_refs = if no_submit {
        Vec::new()
    } else {
        stack
            .current_stack(&original)
            .into_iter()
            .filter(|branch| branch != &stack.trunk)
            .collect::<Vec<_>>()
    };

    let step3 = if no_submit {
        "Skip push and PR updates (--no-submit)"
    } else if no_pr {
        "Push branches without updating PRs"
    } else {
        "Push branches and update PRs"
    };
    println!("{} {}", "▸".cyan().bold(), "Updating stack".bold());
    println!("  {} Sync trunk", "├─".dimmed());
    println!(
        "  {} Restack current stack onto updated parents",
        "├─".dimmed()
    );
    println!("  {} {}", "└─".dimmed(), step3);

    commands::sync::run(
        true,  // restack
        false, // full
        false, // delete_merged
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        commands::sync::StashPolicy::Prompt,
        false, // json
        &submit_fetch_refs,
        true, // refresh/update is an explicit workflow — no Sync plan prompt
    )?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if no_submit {
        return restore_original_branch(&repo, &workdir, &original);
    }

    commands::submit::run(
        commands::submit::SubmitScope::Stack,
        commands::submit::SubmitOptions {
            no_pr,
            prefetched: true,
            yes,
            no_prompt,
            verbose,
            ..Default::default()
        },
    )?;

    restore_original_branch(&repo, &workdir, &original)
}

fn restore_original_branch(
    repo: &GitRepo,
    workdir: &std::path::Path,
    original: &str,
) -> Result<()> {
    if !repo.rebase_in_progress()?
        && repo.current_branch()? != original
        && local_branch_exists_in(workdir, original)
    {
        repo.checkout(original)?;
    }

    Ok(())
}

struct StackPlan {
    root: String,
    branches: Vec<String>,
}

fn collect_stack_plans(stack: &Stack) -> Vec<StackPlan> {
    let mut roots = stack.children(&stack.trunk);
    roots.sort();
    roots
        .into_iter()
        .filter(|root| root != &stack.trunk)
        .map(|root| {
            let mut descendants = stack.descendants(&root);
            descendants.sort();
            let mut branches = vec![root.clone()];
            branches.extend(descendants);
            StackPlan { root, branches }
        })
        .collect()
}

fn root_of(stack: &Stack, branch: &str) -> Option<String> {
    if branch == stack.trunk || !stack.branches.contains_key(branch) {
        return None;
    }
    let mut ancestors = stack.ancestors(branch);
    ancestors.retain(|a| a != &stack.trunk);
    Some(ancestors.pop().unwrap_or_else(|| branch.to_string()))
}

fn print_all_stacks_header(no_pr: bool, no_submit: bool) {
    let step3 = if no_submit {
        "Skip push and PR updates (--no-submit)"
    } else if no_pr {
        "Push branches without updating PRs"
    } else {
        "Push branches and update PRs"
    };
    println!("{} {}", "▸".cyan().bold(), "Updating all stacks".bold());
    println!("  {} Sync trunk (once)", "├─".dimmed());
    println!(
        "  {} Restack every stack onto updated parents",
        "├─".dimmed()
    );
    println!("  {} {}", "└─".dimmed(), step3);
}

fn print_all_stacks_plan(stack: &Stack, plans: &[StackPlan]) {
    for (index, plan) in plans.iter().enumerate() {
        let count = plan.branches.len();
        let noun = if count == 1 { "branch" } else { "branches" };
        println!("  {}. {}  ({count} {noun})", index + 1, plan.root);
        for branch in &plan.branches {
            let pr = stack
                .branches
                .get(branch)
                .and_then(|b| b.pr_number)
                .map(|number| format!("#{number}"))
                .unwrap_or_else(|| "(no PR)".to_string());
            println!("     {} {}", branch, pr.dimmed());
        }
    }
}

fn confirm_all_stacks(plans: &[StackPlan], yes: bool, force: bool) -> Result<bool> {
    if yes || force || !std::io::stdin().is_terminal() {
        return Ok(true);
    }
    Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Refresh {} stacks?", plans.len()))
        .default(true)
        .interact()
        .map_err(Into::into)
}

fn print_all_stacks_progress(plans: &[StackPlan], completed: &[String], stopped_at: &str) {
    let remaining: Vec<&str> = plans
        .iter()
        .map(|plan| plan.root.as_str())
        .filter(|root| *root != stopped_at && !completed.iter().any(|c| c == root))
        .collect();
    println!();
    println!(
        "{} {}",
        "✗".red(),
        format!("Stopped at '{stopped_at}'").bold()
    );
    if completed.is_empty() {
        println!("  Done: (none)");
    } else {
        println!("  Done: {}", completed.join(", "));
    }
    if remaining.is_empty() {
        println!("  Remaining: (none)");
    } else {
        println!("  Remaining: {}", remaining.join(", "));
    }
    println!(
        "  Resolve the conflict, run `st continue`, then re-run `st refresh --all-stacks`. \
         Finished stacks are no-ops on the re-run."
    );
}

#[allow(clippy::too_many_arguments)]
fn run_all_stacks(
    no_pr: bool,
    no_submit: bool,
    force: bool,
    safe: bool,
    verbose: bool,
    yes: bool,
    no_prompt: bool,
    auto_stash_pop: bool,
) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;
    let workdir = repo.workdir()?.to_path_buf();
    let stack = Stack::load(&repo)?;

    let plans = collect_stack_plans(&stack);
    if plans.is_empty() {
        println!("{}", "No tracked stacks to refresh.".yellow());
        return Ok(());
    }

    if !auto_stash_pop && repo.is_dirty()? {
        bail!(
            "Working tree has uncommitted changes.\n\
             `st refresh --all-stacks` restacks every stack, so it needs a clean tree.\n\
             Commit or stash your changes, or re-run with --auto-stash-pop."
        );
    }

    print_all_stacks_header(no_pr, no_submit);
    print_all_stacks_plan(&stack, &plans);

    if !confirm_all_stacks(&plans, yes, force)? {
        println!("Aborted.");
        return Ok(());
    }

    let submit_fetch_refs: Vec<String> = if no_submit {
        Vec::new()
    } else {
        plans
            .iter()
            .flat_map(|p| p.branches.iter().cloned())
            .collect()
    };

    let sync_root = root_of(&stack, &original);
    if let Err(error) = commands::sync::run(
        true,  // restack
        false, // full
        false, // delete_merged
        false, // delete_upstream_gone
        force,
        safe,
        false, // continue
        false, // quiet
        verbose,
        auto_stash_pop,
        commands::sync::StashPolicy::Prompt,
        false, // json
        &submit_fetch_refs,
        true, // refresh/update is an explicit workflow — no Sync plan prompt
    ) {
        if let Some(root) = sync_root.as_deref() {
            print_all_stacks_progress(&plans, &[], root);
        }
        return Err(error);
    }
    if repo.rebase_in_progress()? {
        if let Some(root) = sync_root.as_deref() {
            print_all_stacks_progress(&plans, &[], root);
        }
        return Ok(());
    }

    let live_stack = Stack::load(&repo)?;
    let live_plans = collect_stack_plans(&live_stack);

    let mut completed: Vec<String> = Vec::new();
    for plan in &live_plans {
        if let Err(error) =
            commands::restack::run_stack_containing(&repo, &plan.root, false, auto_stash_pop)
        {
            print_all_stacks_progress(&live_plans, &completed, &plan.root);
            return if error.downcast_ref::<ConflictStopped>().is_some() {
                Err(error)
            } else {
                Err(error.context(format!("failed to restack stack '{}'", plan.root)))
            };
        }
        if repo.rebase_in_progress()? {
            print_all_stacks_progress(&live_plans, &completed, &plan.root);
            return Ok(());
        }

        if !no_submit {
            repo.checkout(&plan.root).with_context(|| {
                format!(
                    "could not check out '{}' to submit its stack \
                     (is it checked out in another worktree?)",
                    plan.root
                )
            })?;
            commands::submit::run(
                commands::submit::SubmitScope::Stack,
                commands::submit::SubmitOptions {
                    no_pr,
                    prefetched: true,
                    yes,
                    no_prompt,
                    verbose,
                    ..Default::default()
                },
            )
            .with_context(|| format!("submit failed for stack '{}'", plan.root))?;
        }
        completed.push(plan.root.clone());
    }

    restore_original_branch(&repo, &workdir, &original)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::stack::StackBranch;
    use std::collections::HashMap;

    fn create_test_stack() -> Stack {
        // main (trunk)
        //  ├── feature-a
        //  │   └── feature-a-1
        //  │       └── feature-a-2
        //  └── feature-b
        let mut branches = HashMap::new();

        branches.insert(
            "main".to_string(),
            StackBranch {
                name: "main".to_string(),
                parent: None,
                parent_revision: None,
                children: vec!["feature-a".to_string(), "feature-b".to_string()],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "feature-a".to_string(),
            StackBranch {
                name: "feature-a".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("abc".to_string()),
                children: vec!["feature-a-1".to_string()],
                needs_restack: false,
                pr_number: Some(1),
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "feature-a-1".to_string(),
            StackBranch {
                name: "feature-a-1".to_string(),
                parent: Some("feature-a".to_string()),
                parent_revision: Some("def".to_string()),
                children: vec!["feature-a-2".to_string()],
                needs_restack: false,
                pr_number: Some(2),
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "feature-a-2".to_string(),
            StackBranch {
                name: "feature-a-2".to_string(),
                parent: Some("feature-a-1".to_string()),
                parent_revision: Some("ghi".to_string()),
                children: Vec::new(),
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        );
        branches.insert(
            "feature-b".to_string(),
            StackBranch {
                name: "feature-b".to_string(),
                parent: Some("main".to_string()),
                parent_revision: Some("jkl".to_string()),
                children: Vec::new(),
                needs_restack: false,
                pr_number: Some(3),
                pr_state: None,
                pr_is_draft: None,
            },
        );

        Stack {
            branches,
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn collect_stack_plans_returns_sorted_roots_with_descendants() {
        let stack = create_test_stack();
        let plans = collect_stack_plans(&stack);
        let roots: Vec<&str> = plans.iter().map(|p| p.root.as_str()).collect();
        assert_eq!(roots, vec!["feature-a", "feature-b"]);
        assert_eq!(
            plans[0].branches,
            vec!["feature-a", "feature-a-1", "feature-a-2"]
        );
    }

    #[test]
    fn root_of_finds_stack_root() {
        let stack = create_test_stack();
        assert_eq!(
            root_of(&stack, "feature-a-2"),
            Some("feature-a".to_string())
        );
        assert_eq!(root_of(&stack, "feature-b"), Some("feature-b".to_string()));
        assert_eq!(root_of(&stack, "main"), None);
    }
}
