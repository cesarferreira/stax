use crate::application::{NoopOperationReporter, OperationSideEffects, RepositorySession};
use crate::engine::{Stack, build_parent_candidates};
use crate::git::GitRepo;
use anyhow::{Result, bail};
use colored::Colorize;
use dialoguer::FuzzySelect;
use dialoguer::theme::ColorfulTheme;
use std::collections::HashSet;

/// Reparent the current branch AND all its descendants onto a new parent.
/// The subtree structure is preserved -- only the root's parent changes.
/// Rebase is always performed so git history matches the new parent immediately.
pub fn run(target: Option<String>, auto_stash_pop: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let stack = Stack::load(&repo)?;
    let current = repo.current_branch()?;
    let trunk = repo.trunk_branch()?;

    if current == trunk {
        bail!("Cannot reparent trunk. Checkout a stacked branch first.");
    }

    let descendants = stack.descendants(&current);

    // Determine new parent
    let new_parent = match target {
        Some(t) => {
            if repo.branch_commit(&t).is_err() {
                bail!("Branch '{}' does not exist", t);
            }
            t
        }
        None => pick_parent_interactively(&repo, &stack, &current, &trunk, &descendants)?,
    };

    let receipt = RepositorySession::open(repo.workdir()?)?.move_subtree(
        &current,
        &new_parent,
        auto_stash_pop,
        &mut NoopOperationReporter,
    )?;
    if receipt.side_effects == OperationSideEffects::None {
        println!(
            "{}",
            format!(
                "'{}' is already parented onto '{}'. Nothing to do.",
                current, new_parent
            )
            .dimmed()
        );
        return Ok(());
    }

    // Collect the subtree for display
    let mut subtree = vec![current.clone()];
    subtree.extend(descendants.iter().cloned());

    println!(
        "✓ Reparented '{}' onto '{}'",
        current.green(),
        new_parent.blue()
    );
    if subtree.len() > 1 {
        println!(
            "  {} descendant branch(es) moved with it:",
            (subtree.len() - 1).to_string().cyan()
        );
        for desc in &subtree[1..] {
            println!("    {}", desc.dimmed());
        }
    }

    Ok(())
}

fn pick_parent_interactively(
    repo: &GitRepo,
    stack: &Stack,
    current: &str,
    trunk: &str,
    descendants: &[String],
) -> Result<String> {
    let all_branches = repo.list_branches()?;
    let branches = build_parent_candidates(&all_branches, current, descendants, trunk);

    if branches.is_empty() {
        bail!("No branches available as a new parent");
    }

    // Build tree-formatted display strings using the same helper the TUI
    // move picker uses (`build_tree_prefix`) so both surfaces look alike.
    // `is_selected = false` because dialoguer provides its own `>` marker.
    let depths: Vec<usize> = branches
        .iter()
        .map(|b| branch_depth(stack, b, trunk))
        .collect();
    let max_depth = depths.iter().copied().max().unwrap_or(0);
    let display_items: Vec<String> = branches
        .iter()
        .zip(&depths)
        .map(|(b, &depth)| {
            let prefix = crate::tui::ui::build_tree_prefix(depth, max_depth, false);
            format!("{}{}", prefix, b)
        })
        .collect();

    let selection = FuzzySelect::with_theme(&ColorfulTheme::default())
        .with_prompt(format!(
            "Select new parent for '{}' (and all its descendants)",
            current
        ))
        .items(&display_items)
        .default(0)
        .interact()?;

    Ok(branches[selection].clone())
}

/// Walk up the parent chain to compute how deep `name` is below `trunk`.
/// Returns 0 for trunk itself, 1 for its direct children, etc. If the
/// branch isn't tracked in the stack, its ancestor chain doesn't reach
/// trunk, or a cycle is detected, stops and returns the depth reached so
/// far. The visited set mirrors `Stack::descendants` to prevent infinite
/// loops on corrupt metadata.
fn branch_depth(stack: &Stack, name: &str, trunk: &str) -> usize {
    let mut depth = 0;
    let mut current = name.to_string();
    let mut visited = HashSet::from([current.clone()]);
    while current != trunk {
        match stack.branches.get(&current).and_then(|i| i.parent.as_ref()) {
            Some(parent) if visited.insert(parent.clone()) => {
                current.clone_from(parent);
                depth += 1;
            }
            _ => break,
        }
    }
    depth
}

#[cfg(test)]
mod tests {
    use super::branch_depth;
    use crate::engine::stack::{Stack, StackBranch};
    use std::collections::HashMap;

    fn stub(name: &str, parent: Option<&str>) -> (String, StackBranch) {
        (
            name.to_string(),
            StackBranch {
                name: name.to_string(),
                parent: parent.map(str::to_string),
                parent_revision: None,
                children: vec![],
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                pr_is_draft: None,
            },
        )
    }

    fn test_stack() -> Stack {
        // main → a → b → c
        Stack {
            branches: HashMap::from([
                stub("main", None),
                stub("a", Some("main")),
                stub("b", Some("a")),
                stub("c", Some("b")),
            ]),
            trunk: "main".to_string(),
        }
    }

    #[test]
    fn depth_of_trunk_is_zero() {
        assert_eq!(branch_depth(&test_stack(), "main", "main"), 0);
    }

    #[test]
    fn depth_of_direct_child_is_one() {
        assert_eq!(branch_depth(&test_stack(), "a", "main"), 1);
    }

    #[test]
    fn depth_of_deeply_nested_branch() {
        assert_eq!(branch_depth(&test_stack(), "c", "main"), 3);
    }

    #[test]
    fn depth_of_untracked_branch_is_zero() {
        assert_eq!(branch_depth(&test_stack(), "unknown", "main"), 0);
    }

    #[test]
    fn depth_terminates_on_cyclic_metadata() {
        // a → b → a (cycle that never reaches trunk)
        let stack = Stack {
            branches: HashMap::from([stub("a", Some("b")), stub("b", Some("a"))]),
            trunk: "main".to_string(),
        };
        let d = branch_depth(&stack, "a", "main");
        assert!(d <= 2, "cyclic chain should terminate, got depth {}", d);
    }
}
