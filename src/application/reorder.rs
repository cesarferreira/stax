#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, RepositorySession, TransactionSummary,
};
use crate::application::repository::MutationTargets;
use crate::engine::{BranchMetadata, Stack};
use crate::git::{GitRepo, RebaseResult};
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use std::collections::{HashMap, HashSet};

impl RepositorySession {
    /// Apply a stale-safe bottom-to-top order to one linear stack.
    pub fn reorder_stack(
        &self,
        original_order: &[String],
        proposed_order: &[String],
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::ReorderStack {
            original_order: original_order.to_vec(),
            proposed_order: proposed_order.to_vec(),
            auto_stash,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.reorder_stack_unframed(
                &request,
                original_order,
                proposed_order,
                auto_stash,
                reporter,
            )
        })
    }

    pub(super) fn reorder_stack_unframed(
        &self,
        request: &OperationRequest,
        original_order: &[String],
        proposed_order: &[String],
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(
            request,
            MutationTargets::branches(original_order.iter().chain(proposed_order.iter()).cloned()),
            || {
                reorder_inner(
                    self,
                    request,
                    original_order,
                    proposed_order,
                    auto_stash,
                    reporter,
                )
            },
        )
    }
}

fn reorder_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    original_order: &[String],
    proposed_order: &[String],
    auto_stash: bool,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    validate_order_shape(request, original_order, proposed_order)?;
    let repo = session
        .open_repo()
        .map_err(|error| source_error(request, error))?;
    let stack = Stack::load(&repo).map_err(|error| source_error(request, error))?;
    let live_order = linear_chain_containing(request, &stack, &original_order[0])?;
    if live_order != original_order {
        return Err(input_error(
            request,
            OperationErrorKind::PreconditionFailed,
            "The stack changed after this reorder preview was created",
            "Refresh the repository and create a new reorder preview",
            format!("expected {original_order:?}, live order is {live_order:?}"),
        ));
    }
    if proposed_order == original_order {
        return Ok(reorder_receipt(
            request,
            original_order,
            proposed_order,
            None,
            OperationSideEffects::None,
        ));
    }

    let original_checkout = repo
        .current_branch()
        .map_err(|error| source_error(request, error))?;
    let metadata = load_metadata(request, &repo, original_order)?;
    let changed = proposed_order
        .iter()
        .enumerate()
        .filter_map(|(index, branch)| {
            let desired = if index == 0 {
                stack.trunk.as_str()
            } else {
                proposed_order[index - 1].as_str()
            };
            (metadata[branch].parent_branch_name != desired).then_some(branch.clone())
        })
        .collect::<Vec<_>>();
    preflight_dirty(request, &repo, &changed, auto_stash)?;

    let mut tx = Transaction::begin(OpKind::Reorder, &repo, true)
        .map_err(|error| source_error(request, error))?;
    tx.plan_branches(&repo, proposed_order)
        .map_err(|error| source_error(request, error))?;
    for branch in proposed_order {
        tx.plan_metadata_ref(&repo, branch)
            .map_err(|error| source_error(request, error))?;
    }
    tx.set_auto_stash_pop(auto_stash);
    tx.snapshot()
        .map_err(|error| source_error(request, error))?;
    let mut tx = Some(tx);

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::ReorderingStack,
        completed: 0,
        total: Some(changed.len()),
        branch: None,
        message: "Applying stack order".into(),
    }));
    let mut completed = 0;
    for (index, branch) in proposed_order.iter().enumerate() {
        let desired_parent = if index == 0 {
            stack.trunk.as_str()
        } else {
            proposed_order[index - 1].as_str()
        };
        let existing = &metadata[branch];
        if existing.parent_branch_name == desired_parent {
            continue;
        }
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage: OperationStage::Restacking,
            completed,
            total: Some(changed.len()),
            branch: Some(branch.clone()),
            message: format!("Restacking {branch} onto {desired_parent}"),
        }));
        let parent_oid = match repo.branch_commit(desired_parent) {
            Ok(oid) => oid,
            Err(error) => {
                return Err(finish_error(
                    request,
                    &mut tx,
                    &repo,
                    original_order,
                    proposed_order,
                    &original_checkout,
                    OperationErrorKind::LocalGit,
                    format!("Could not resolve parent '{desired_parent}'"),
                    error,
                    "resolve-parent",
                ));
            }
        };
        let merge_base = repo
            .merge_base(desired_parent, branch)
            .unwrap_or_else(|_| parent_oid.clone());
        let upstream = match rebase_upstream(&repo, existing, branch, &merge_base) {
            Ok(upstream) => upstream,
            Err(error) => {
                return Err(finish_error(
                    request,
                    &mut tx,
                    &repo,
                    original_order,
                    proposed_order,
                    &original_checkout,
                    OperationErrorKind::LocalGit,
                    format!("Could not prepare rebase for '{branch}'"),
                    error,
                    "prepare-rebase",
                ));
            }
        };
        match repo.rebase_branch_onto_with_provenance(branch, desired_parent, &upstream, auto_stash)
        {
            Ok(RebaseResult::Success) => {}
            Ok(RebaseResult::Conflict) => {
                return Err(finish_error(
                    request,
                    &mut tx,
                    &repo,
                    original_order,
                    proposed_order,
                    &original_checkout,
                    OperationErrorKind::RebaseConflict,
                    format!("Rebase conflict while reordering '{branch}'"),
                    anyhow::anyhow!("rebase conflict"),
                    "rebase",
                ));
            }
            Err(error) => {
                return Err(finish_error(
                    request,
                    &mut tx,
                    &repo,
                    original_order,
                    proposed_order,
                    &original_checkout,
                    OperationErrorKind::LocalGit,
                    format!("Could not rebase '{branch}'"),
                    error,
                    "rebase",
                ));
            }
        }
        let mut updated = existing.clone();
        updated.parent_branch_name = desired_parent.to_string();
        updated.parent_branch_revision = repo.branch_commit(desired_parent).unwrap_or(parent_oid);
        if let Err(error) = updated.write(repo.inner(), branch) {
            return Err(finish_error(
                request,
                &mut tx,
                &repo,
                original_order,
                proposed_order,
                &original_checkout,
                OperationErrorKind::LocalGit,
                format!("Could not update metadata for '{branch}'"),
                error,
                "write-metadata",
            ));
        }
        tx.as_mut().unwrap().push_completed_branch(branch);
        completed += 1;
    }

    if !matches!(repo.current_branch().as_deref(), Ok(branch) if branch == original_checkout)
        && let Err(error) = repo.checkout(&original_checkout)
    {
        return Err(finish_error(
            request,
            &mut tx,
            &repo,
            original_order,
            proposed_order,
            &original_checkout,
            OperationErrorKind::LocalGit,
            "The stack reordered, but the original checkout could not be restored",
            error,
            "restore-checkout",
        ));
    }
    let transaction = tx.as_mut().unwrap();
    if let Err(error) = record_after(transaction, &repo, proposed_order) {
        return Err(finish_error(
            request,
            &mut tx,
            &repo,
            original_order,
            proposed_order,
            &original_checkout,
            OperationErrorKind::LocalGit,
            "The stack reordered, but its recovery state could not be recorded",
            error,
            "record-after",
        ));
    }
    let mut transaction = tx.take().unwrap();
    transaction.set_head_branch_after(&original_checkout);
    let finalized = transaction.finish_ok_preserving_receipt();
    let receipt = reorder_receipt(
        request,
        original_order,
        proposed_order,
        Some(TransactionSummary::from(&finalized.receipt)),
        OperationSideEffects::RepositoryChanged,
    );
    if let Some(error) = finalized.persistence_error {
        return Err(OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "The stack reordered, but its receipt could not be saved",
            "Inspect the repository and retry if needed",
            &error,
            Some(receipt),
            OperationSideEffects::RepositoryChanged,
        ));
    }
    Ok(receipt)
}

fn validate_order_shape(
    request: &OperationRequest,
    original: &[String],
    proposed: &[String],
) -> Result<(), OperationError> {
    let original_set = original.iter().collect::<HashSet<_>>();
    let proposed_set = proposed.iter().collect::<HashSet<_>>();
    if original.is_empty() || original_set.len() != original.len() {
        return Err(input_error(
            request,
            OperationErrorKind::InvalidInput,
            "Original stack order is invalid",
            "Refresh and retry",
            "empty or duplicate original order",
        ));
    }
    if original.len() != proposed.len()
        || proposed_set.len() != proposed.len()
        || original_set != proposed_set
    {
        return Err(input_error(
            request,
            OperationErrorKind::InvalidInput,
            "Proposed order must contain each original branch exactly once",
            "Correct the reorder preview and retry",
            "proposed order is not a permutation",
        ));
    }
    Ok(())
}

fn linear_chain_containing(
    request: &OperationRequest,
    stack: &Stack,
    seed: &str,
) -> Result<Vec<String>, OperationError> {
    if seed == stack.trunk || !stack.branches.contains_key(seed) {
        return Err(input_error(
            request,
            OperationErrorKind::InvalidInput,
            "Reorder preview contains an unknown or trunk branch",
            "Choose a stacked branch and retry",
            "invalid reorder seed",
        ));
    }
    let mut root = seed.to_string();
    while let Some(parent) = stack
        .branches
        .get(&root)
        .and_then(|branch| branch.parent.as_ref())
    {
        if parent == &stack.trunk {
            break;
        }
        root.clone_from(parent);
    }
    let mut chain = Vec::new();
    let mut current = root;
    loop {
        chain.push(current.clone());
        let mut children = stack
            .branches
            .get(&current)
            .map(|branch| branch.children.clone())
            .unwrap_or_default();
        children.retain(|child| child != &stack.trunk);
        if children.len() > 1 {
            return Err(input_error(
                request,
                OperationErrorKind::PreconditionFailed,
                "Forked stacks cannot be reordered as one linear chain",
                "Move branches explicitly until the stack is linear",
                format!("branch '{current}' has children {children:?}"),
            ));
        }
        let Some(child) = children.pop() else {
            break;
        };
        current = child;
    }
    Ok(chain)
}

fn load_metadata(
    request: &OperationRequest,
    repo: &GitRepo,
    order: &[String],
) -> Result<HashMap<String, BranchMetadata>, OperationError> {
    let mut result = HashMap::new();
    for branch in order {
        let metadata = BranchMetadata::read(repo.inner(), branch)
            .map_err(|error| source_error(request, error))?
            .ok_or_else(|| {
                input_error(
                    request,
                    OperationErrorKind::PreconditionFailed,
                    format!("Branch '{branch}' is not tracked"),
                    "Track every branch before reordering",
                    "missing branch metadata",
                )
            })?;
        result.insert(branch.clone(), metadata);
    }
    Ok(result)
}

fn preflight_dirty(
    request: &OperationRequest,
    repo: &GitRepo,
    changed: &[String],
    auto_stash: bool,
) -> Result<(), OperationError> {
    if auto_stash {
        return Ok(());
    }
    let mut seen = HashSet::new();
    for branch in changed {
        let worktree = repo
            .branch_rebase_target_workdir(branch)
            .map_err(|error| source_error(request, error))?;
        if seen.insert(worktree.clone()) && repo.is_dirty_at(&worktree).unwrap_or(false) {
            return Err(OperationError {
                request: request.clone(),
                kind: OperationErrorKind::DirtyWorktree,
                details: OperationErrorDetails::Rebase {
                    branch: Some(branch.clone()),
                    worktree,
                },
                primary: "An affected worktree has uncommitted changes".into(),
                action: "Retry with automatic stashing after reviewing the worktree".into(),
                diagnostic_chain: "dirty worktree preflight rejected reorder".into(),
                receipt: None,
                side_effects: OperationSideEffects::None,
            });
        }
    }
    Ok(())
}

fn rebase_upstream(
    repo: &GitRepo,
    metadata: &BranchMetadata,
    branch: &str,
    merge_base: &str,
) -> anyhow::Result<String> {
    if let Ok(tip) = repo.branch_commit(&metadata.parent_branch_name)
        && repo.is_ancestor(&tip, branch)?
    {
        return Ok(tip);
    }
    if !metadata.parent_branch_revision.is_empty()
        && repo.is_ancestor(&metadata.parent_branch_revision, branch)?
    {
        return Ok(metadata.parent_branch_revision.clone());
    }
    Ok(merge_base.to_string())
}

fn record_after(tx: &mut Transaction, repo: &GitRepo, order: &[String]) -> anyhow::Result<()> {
    for branch in order {
        tx.record_optional_after(repo, branch)?;
        tx.record_metadata_ref_after(repo, branch)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_error(
    request: &OperationRequest,
    tx: &mut Option<Transaction>,
    repo: &GitRepo,
    original: &[String],
    proposed: &[String],
    checkout: &str,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    error: anyhow::Error,
    step: &'static str,
) -> OperationError {
    let primary = primary.into();
    let mut transaction = tx.take().expect("reorder transaction should be active");
    let _ = record_after(&mut transaction, repo, proposed);
    transaction.set_head_branch_after(checkout);
    let finalized = transaction.finish_err_preserving_receipt(&primary, Some(step), None);
    let receipt = reorder_receipt(
        request,
        original,
        proposed,
        Some(TransactionSummary::from(&finalized.receipt)),
        OperationSideEffects::RepositoryChanged,
    );
    OperationError {
        request: request.clone(),
        kind,
        details: OperationErrorDetails::None,
        primary,
        action: "Inspect the repository and use the recovery commands before retrying".into(),
        diagnostic_chain: format!("{error:#}"),
        receipt: Some(receipt),
        side_effects: OperationSideEffects::RepositoryChanged,
    }
}

fn reorder_receipt(
    request: &OperationRequest,
    original: &[String],
    proposed: &[String],
    transaction: Option<TransactionSummary>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    OperationReceipt {
        request: request.clone(),
        summary: if side_effects == OperationSideEffects::None {
            "Stack order unchanged".into()
        } else {
            "Stack reordered".into()
        },
        affected_branches: proposed.to_vec(),
        outcome: OperationOutcome::StackReordered {
            original_order: original.to_vec(),
            applied_order: proposed.to_vec(),
        },
        transaction,
        warnings: Vec::new(),
        side_effects,
    }
}

fn input_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic: impl Into<String>,
) -> OperationError {
    OperationError {
        request: request.clone(),
        kind,
        details: OperationErrorDetails::None,
        primary: primary.into(),
        action: action.into(),
        diagnostic_chain: diagnostic.into(),
        receipt: None,
        side_effects: OperationSideEffects::None,
    }
}

fn source_error(request: &OperationRequest, source: anyhow::Error) -> OperationError {
    OperationError::from_source(
        request.clone(),
        OperationErrorKind::LocalGit,
        OperationErrorDetails::None,
        "Could not apply the stack reorder",
        "Resolve the repository error and retry",
        &source,
        None,
        OperationSideEffects::None,
    )
}
