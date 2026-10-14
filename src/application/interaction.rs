//! Presentation-neutral action availability and interaction state.
//!
//! Shared by the `st web` workspace.

use super::{OperationReceipt, RepositorySnapshot, TransactionSummary};

/// Whether a specific user action is currently available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionAvailability {
    pub enabled: bool,
    pub reason: Option<String>,
}

impl ActionAvailability {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            reason: None,
        }
    }

    pub fn disabled(reason: impl Into<String>) -> Self {
        Self {
            enabled: false,
            reason: Some(reason.into()),
        }
    }
}

/// Full availability state for every interaction in the workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InteractionState {
    pub checkout: ActionAvailability,
    pub create: ActionAvailability,
    pub rename: ActionAvailability,
    pub delete: ActionAvailability,
    pub move_subtree: ActionAvailability,
    pub reorder: ActionAvailability,
    pub undo: ActionAvailability,
    pub redo: ActionAvailability,
    pub restack: ActionAvailability,
    pub restack_all: ActionAvailability,
    pub submit: ActionAvailability,
    pub open_pr: ActionAvailability,
    pub open_repository: ActionAvailability,
    pub refresh: ActionAvailability,
    pub navigation: ActionAvailability,
}

/// Returns a complete `InteractionState` from a snapshot and optional operation context.
///
/// - `selected`: the branch name currently highlighted (not necessarily checked out).
/// - `active_mutating`: whether a mutation operation is in flight.
/// - `last_receipt`: the most recently completed operation receipt.
pub fn interaction_state(
    snapshot: &RepositorySnapshot,
    selected: Option<&str>,
    active_mutating: bool,
    last_receipt: Option<&OperationReceipt>,
) -> InteractionState {
    interaction_state_from_transaction(
        snapshot,
        selected,
        active_mutating,
        last_receipt.and_then(|receipt| receipt.transaction.as_ref()),
    )
}

/// Returns interaction state from the latest persisted local transaction.
///
/// Application operations normally provide an [`OperationReceipt`], while
/// clients that invoke a legacy transactional command can reload its
/// [`TransactionSummary`] directly from disk and use this entry point.
pub fn interaction_state_from_transaction(
    snapshot: &RepositorySnapshot,
    selected: Option<&str>,
    active_mutating: bool,
    last_transaction: Option<&TransactionSummary>,
) -> InteractionState {
    if active_mutating {
        let reason = "A repository operation is running.";
        let disabled = ActionAvailability::disabled(reason);
        return InteractionState {
            checkout: disabled.clone(),
            create: disabled.clone(),
            rename: disabled.clone(),
            delete: disabled.clone(),
            move_subtree: disabled.clone(),
            reorder: disabled.clone(),
            undo: disabled.clone(),
            redo: disabled.clone(),
            restack: disabled.clone(),
            restack_all: disabled.clone(),
            submit: disabled.clone(),
            open_pr: disabled.clone(),
            open_repository: disabled.clone(),
            refresh: disabled.clone(),
            navigation: disabled,
        };
    }

    let selected_summary =
        selected.and_then(|name| snapshot.branches.iter().find(|b| b.name == name));
    let selected_name = selected_summary
        .map(|b| b.name.as_str())
        .unwrap_or("the selected branch");
    let has_non_trunk = snapshot.branches.iter().any(|b| !b.is_trunk);
    let local_transaction = last_transaction.filter(|tx| !tx.changed_remote_refs);
    let reorder_len = selected_summary
        .and_then(|b| linear_stack_order(snapshot, &b.name))
        .map(|v| v.len())
        .unwrap_or(0);

    InteractionState {
        checkout: match selected_summary {
            Some(b) if !b.is_current && !b.is_trunk => ActionAvailability::enabled(),
            Some(b) if b.is_trunk => {
                ActionAvailability::disabled("Select a tracked branch to check out.")
            }
            Some(_) => ActionAvailability::disabled(format!("{selected_name} is already current.")),
            None => ActionAvailability::disabled("Select a branch to check out."),
        },
        create: if selected_summary.is_some() {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("Open a repository before creating a branch.")
        },
        rename: match selected_summary {
            Some(b) if b.is_current && !b.is_trunk => ActionAvailability::enabled(),
            Some(b) if b.is_trunk => {
                ActionAvailability::disabled("The trunk branch cannot be renamed here.")
            }
            Some(_) => ActionAvailability::disabled("Check out the branch before renaming it."),
            None => ActionAvailability::disabled("Select a branch to rename."),
        },
        delete: match selected_summary {
            Some(b) if !b.is_current && !b.is_trunk => ActionAvailability::enabled(),
            Some(b) if b.is_trunk => {
                ActionAvailability::disabled("The trunk branch cannot be deleted.")
            }
            Some(_) => {
                ActionAvailability::disabled("Check out another branch before deleting this one.")
            }
            None => ActionAvailability::disabled("Select a branch to delete."),
        },
        move_subtree: match selected_summary {
            Some(b) if !b.is_trunk && !move_parent_candidates(snapshot, &b.name).is_empty() => {
                ActionAvailability::enabled()
            }
            Some(b) if b.is_trunk => {
                ActionAvailability::disabled("The trunk branch cannot be moved.")
            }
            Some(_) => ActionAvailability::disabled("No eligible parent branch is available."),
            None => ActionAvailability::disabled("Select a branch to move."),
        },
        reorder: if reorder_len >= 2 {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("Select a linear stack with at least two branches.")
        },
        undo: match local_transaction {
            Some(tx) if tx.can_undo => ActionAvailability::enabled(),
            Some(_) => ActionAvailability::disabled("The latest local operation cannot be undone."),
            None => ActionAvailability::disabled("No safe local operation is available to undo."),
        },
        redo: match local_transaction {
            Some(tx) if tx.can_redo => ActionAvailability::enabled(),
            Some(_) => ActionAvailability::disabled("The latest local operation cannot be redone."),
            None => ActionAvailability::disabled("No safe local operation is available to redo."),
        },
        restack: match selected_summary {
            Some(b) if !b.is_trunk => ActionAvailability::enabled(),
            Some(_) => ActionAvailability::disabled("Select a tracked branch to restack."),
            None => ActionAvailability::disabled("Select a branch to restack."),
        },
        restack_all: if has_non_trunk {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("No tracked branches are available to restack.")
        },
        submit: if has_non_trunk {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("No stack branches are available to submit.")
        },
        open_pr: match selected_summary {
            Some(b) if !b.is_trunk => ActionAvailability::enabled(),
            Some(_) => ActionAvailability::disabled("Select a branch with a pull request."),
            None => ActionAvailability::disabled("Select a branch to open its pull request."),
        },
        open_repository: ActionAvailability::enabled(),
        refresh: ActionAvailability::enabled(),
        navigation: if has_non_trunk {
            ActionAvailability::enabled()
        } else {
            ActionAvailability::disabled("No tracked branches to navigate.")
        },
    }
}

/// Returns branch names that are valid re-parent targets for `branch_name`.
///
/// Excludes:
/// - `branch_name` itself
/// - its current parent (already the parent — no change needed)
/// - any of its transitive descendants (would create a cycle)
pub fn move_parent_candidates(snapshot: &RepositorySnapshot, branch_name: &str) -> Vec<String> {
    let Some(source) = snapshot
        .branches
        .iter()
        .find(|b| b.name == branch_name && !b.is_trunk)
    else {
        return Vec::new();
    };
    let excluded = descendants_of(snapshot, branch_name);
    snapshot
        .branches
        .iter()
        .filter(|b| {
            b.name != branch_name
                && source.parent.as_deref() != Some(b.name.as_str())
                && !excluded.contains(&b.name)
        })
        .map(|b| b.name.clone())
        .collect()
}

/// Returns the set of branch names that are (transitive) descendants of `branch_name`.
pub fn descendants_of(snapshot: &RepositorySnapshot, branch_name: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut queue = vec![branch_name.to_string()];
    while let Some(current) = queue.pop() {
        for b in &snapshot.branches {
            if b.parent.as_deref() == Some(current.as_str()) {
                result.push(b.name.clone());
                queue.push(b.name.clone());
            }
        }
    }
    result
}

/// Returns the linear stack order for the stack containing `branch_name`, or `None` if
/// the stack is not linear (has forks).
pub fn linear_stack_order(snapshot: &RepositorySnapshot, branch_name: &str) -> Option<Vec<String>> {
    // Find the root of the stack (non-trunk branch directly above trunk).
    let trunk = &snapshot.trunk;

    // Walk up from branch_name to trunk to find the stack root.
    let mut stack_branches: Vec<String> = Vec::new();
    let mut current = branch_name.to_string();
    loop {
        stack_branches.push(current.clone());
        let parent = snapshot
            .branches
            .iter()
            .find(|b| b.name == current)
            .and_then(|b| b.parent.clone());
        match parent {
            Some(p) if p != *trunk => current = p,
            _ => break,
        }
    }
    stack_branches.reverse();

    // Now collect all descendant branches (walking down from each leaf).
    let root = stack_branches[0].clone();
    let all_in_stack = collect_stack_downward(snapshot, &root);

    // Check that the stack is linear: each branch has at most one child.
    for branch in &all_in_stack {
        let child_count = snapshot
            .branches
            .iter()
            .filter(|b| b.parent.as_deref() == Some(branch.as_str()))
            .count();
        if child_count > 1 {
            return None;
        }
    }

    // Ensure minimum 2 branches (not counting trunk).
    if all_in_stack.len() < 2 {
        return None;
    }

    Some(all_in_stack)
}

fn collect_stack_downward(snapshot: &RepositorySnapshot, branch_name: &str) -> Vec<String> {
    let mut result = vec![branch_name.to_string()];
    let children: Vec<String> = snapshot
        .branches
        .iter()
        .filter(|b| b.parent.as_deref() == Some(branch_name))
        .map(|b| b.name.clone())
        .collect();
    for child in children {
        result.extend(collect_stack_downward(snapshot, &child));
    }
    result
}
