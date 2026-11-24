use crate::engine::Stack;
use crate::errors::ConflictStopped;
use crate::git::{GitRepo, local_branch_exists_in};
use anyhow::{Result, bail};
use colored::Colorize;

pub(crate) struct StackPlan {
    pub(crate) root: String,
    pub(crate) branches: Vec<String>,
}

pub(crate) fn collect_stack_plans(stack: &Stack) -> Vec<StackPlan> {
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

pub(crate) fn root_of(stack: &Stack, branch: &str) -> Option<String> {
    if branch == stack.trunk || !stack.branches.contains_key(branch) {
        return None;
    }
    let mut ancestors = stack.ancestors(branch);
    ancestors.retain(|a| a != &stack.trunk);
    Some(ancestors.pop().unwrap_or_else(|| branch.to_string()))
}

pub(crate) fn print_stack_plans(stack: &Stack, plans: &[StackPlan]) {
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

pub(crate) fn print_progress(
    plans: &[StackPlan],
    completed: &[String],
    stopped_at: &str,
    rerun_command: &str,
) {
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
        "  Resolve the conflict, run `st continue`, then re-run `{rerun_command}`. \
         Finished stacks are no-ops on the re-run."
    );
}

pub(crate) fn ensure_clean_tree(repo: &GitRepo, auto_stash_pop: bool, command: &str) -> Result<()> {
    if !auto_stash_pop && repo.is_dirty()? {
        bail!(
            "Working tree has uncommitted changes.\n\
             `{command}` restacks every stack, so it needs a clean tree.\n\
             Commit or stash your changes, or re-run with --auto-stash-pop."
        );
    }
    Ok(())
}

pub(crate) enum LoopOutcome {
    Finished,
    PausedForRebase,
}

pub(crate) fn restack_each_stack(
    repo: &GitRepo,
    plans: &[StackPlan],
    quiet: bool,
    auto_stash_pop: bool,
    rerun_command: &str,
    mut after_stack: impl FnMut(&StackPlan) -> Result<()>,
) -> Result<LoopOutcome> {
    let mut completed: Vec<String> = Vec::new();
    for plan in plans {
        if let Err(error) =
            crate::commands::restack::run_stack_containing(repo, &plan.root, quiet, auto_stash_pop)
        {
            print_progress(plans, &completed, &plan.root, rerun_command);
            return if error.downcast_ref::<ConflictStopped>().is_some() {
                Err(error)
            } else {
                Err(error.context(format!("failed to restack stack '{}'", plan.root)))
            };
        }
        if repo.rebase_in_progress()? {
            print_progress(plans, &completed, &plan.root, rerun_command);
            return Ok(LoopOutcome::PausedForRebase);
        }

        after_stack(plan)?;
        completed.push(plan.root.clone());
    }
    Ok(LoopOutcome::Finished)
}

pub(crate) fn restore_original_branch(
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
