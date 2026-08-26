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
use std::collections::HashSet;

impl RepositorySession {
    /// Move a tracked branch and its complete descendant subtree onto a new parent.
    pub fn move_subtree(
        &self,
        source: &str,
        new_parent: &str,
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::MoveSubtree {
            source: source.to_owned(),
            new_parent: new_parent.to_owned(),
            auto_stash,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.move_subtree_unframed(&request, source, new_parent, auto_stash, reporter)
        })
    }

    pub(super) fn move_subtree_unframed(
        &self,
        request: &OperationRequest,
        source: &str,
        new_parent: &str,
        auto_stash: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let repo = self.open_repo().map_err(|error| {
            source_error(
                request,
                OperationErrorKind::RepositoryUnavailable,
                OperationErrorDetails::None,
                "Could not open the repository",
                "Check the repository path and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
        let stack = Stack::load(&repo).map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                OperationErrorDetails::None,
                "Could not load the stack",
                "Resolve the stack metadata error and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
        let mut moved = vec![source.trim().to_string()];
        moved.extend(stack.descendants(source.trim()));
        let mut targets = moved.clone();
        targets.push(new_parent.trim().to_string());
        self.with_mutation(request, MutationTargets::branches(targets), || {
            move_subtree_inner(
                self,
                request,
                &repo,
                stack,
                source.trim(),
                new_parent.trim(),
                moved,
                auto_stash,
                reporter,
            )
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn move_subtree_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    repo: &GitRepo,
    stack: Stack,
    source: &str,
    new_parent: &str,
    moved: Vec<String>,
    auto_stash: bool,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    if source.is_empty() || new_parent.is_empty() {
        return Err(simple_error(
            request,
            OperationErrorKind::InvalidInput,
            "Source and new parent branch names are required",
            "Choose existing local branches and retry",
            "move subtree input was empty",
        ));
    }
    if source == stack.trunk {
        return Err(simple_error(
            request,
            OperationErrorKind::PreconditionFailed,
            "Cannot move the trunk branch",
            "Choose a stacked branch and retry",
            "move source is trunk",
        ));
    }
    repo.branch_commit(source).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::InvalidInput,
            branch_details(source),
            format!("Branch '{source}' does not exist locally"),
            "Choose an existing source branch and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    repo.branch_commit(new_parent).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::InvalidInput,
            branch_details(new_parent),
            format!("Branch '{new_parent}' does not exist locally"),
            "Choose an existing parent branch and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    let old_metadata = BranchMetadata::read(repo.inner(), source)
        .map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                branch_details(source),
                format!("Could not read metadata for '{source}'"),
                "Resolve the stax metadata error and retry",
                error,
                OperationSideEffects::None,
            )
        })?
        .ok_or_else(|| {
            simple_error(
                request,
                OperationErrorKind::PreconditionFailed,
                format!("Branch '{source}' is not tracked by stax"),
                "Track the branch before moving it",
                "move source has no branch metadata",
            )
        })?;
    if source == new_parent {
        return Err(simple_error(
            request,
            OperationErrorKind::InvalidInput,
            "Cannot move a branch onto itself",
            "Choose a different parent and retry",
            "move source equals new parent",
        ));
    }
    if moved.iter().skip(1).any(|branch| branch == new_parent) {
        return Err(simple_error(
            request,
            OperationErrorKind::PreconditionFailed,
            format!(
                "Cannot move '{source}' onto its descendant '{new_parent}': this would create a circular dependency"
            ),
            "Choose a branch outside the moved subtree",
            "move would create a circular dependency",
        ));
    }
    if old_metadata.parent_branch_name == new_parent {
        return Ok(move_receipt(
            request,
            source,
            new_parent,
            moved,
            None,
            OperationSideEffects::None,
        ));
    }

    let original_checkout = repo.current_branch().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not determine the current branch",
            "Resolve the Git HEAD error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    let metadata = load_moved_metadata(request, repo, &moved)?;
    preflight_dirty_worktrees(request, repo, &moved, auto_stash)?;

    let mut transaction = Transaction::begin(OpKind::MoveSubtree, repo, true).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not start the move transaction",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    transaction.plan_branches(repo, &moved).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not prepare branch recovery for the move",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    for branch in &moved {
        transaction
            .plan_metadata_ref(repo, branch)
            .map_err(|error| {
                source_error(
                    request,
                    OperationErrorKind::LocalGit,
                    OperationErrorDetails::None,
                    "Could not prepare metadata recovery for the move",
                    "Resolve the receipt error and retry",
                    error,
                    OperationSideEffects::None,
                )
            })?;
    }
    transaction.set_auto_stash_pop(auto_stash);
    transaction.snapshot().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not save the move recovery snapshot",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    let mut transaction = Some(transaction);

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::MovingSubtree,
        completed: 0,
        total: Some(moved.len()),
        branch: Some(source.to_string()),
        message: format!("Moving {source} onto {new_parent}"),
    }));

    for (index, branch) in moved.iter().enumerate() {
        let existing = metadata
            .iter()
            .find(|(name, _)| name == branch)
            .and_then(|(_, metadata)| metadata.as_ref())
            .expect("moved branches were validated as tracked");
        let parent = if index == 0 {
            new_parent
        } else {
            existing.parent_branch_name.as_str()
        };
        let parent_revision = repo.branch_commit(parent).map_err(|error| {
            finish_move_error(
                request,
                &mut transaction,
                repo,
                source,
                new_parent,
                &moved,
                &original_checkout,
                OperationErrorKind::LocalGit,
                format!("Could not resolve parent '{parent}'"),
                "Resolve the Git ref error and retry",
                error,
                "resolve-parent",
                OperationSideEffects::RepositoryChanged,
            )
        })?;
        let merge_base = repo
            .merge_base(parent, branch)
            .unwrap_or_else(|_| parent_revision.clone());
        let upstream = resolve_rebase_upstream(
            repo,
            &existing.parent_branch_name,
            &existing.parent_branch_revision,
            branch,
            &merge_base,
        )
        .map_err(|error| {
            finish_move_error(
                request,
                &mut transaction,
                repo,
                source,
                new_parent,
                &moved,
                &original_checkout,
                OperationErrorKind::LocalGit,
                format!("Could not prepare rebase for '{branch}'"),
                "Resolve the Git history error and retry",
                error,
                "prepare-rebase",
                OperationSideEffects::RepositoryChanged,
            )
        })?;
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage: OperationStage::Restacking,
            completed: index,
            total: Some(moved.len()),
            branch: Some(branch.clone()),
            message: format!("Restacking {branch} onto {parent}"),
        }));
        let rebase = repo
            .rebase_branch_onto_with_provenance(branch, parent, &upstream, auto_stash)
            .map_err(|error| {
                finish_move_error(
                    request,
                    &mut transaction,
                    repo,
                    source,
                    new_parent,
                    &moved,
                    &original_checkout,
                    OperationErrorKind::LocalGit,
                    format!("Could not rebase '{branch}' onto '{parent}'"),
                    "Resolve the Git rebase error and retry",
                    error,
                    "rebase",
                    OperationSideEffects::RepositoryChanged,
                )
            })?;
        if rebase == RebaseResult::Conflict {
            let worktree = repo
                .branch_rebase_target_workdir(branch)
                .unwrap_or_else(|_| session.repository_root().to_path_buf());
            return Err(finish_move_error_with_details(
                request,
                &mut transaction,
                repo,
                source,
                new_parent,
                &moved,
                &original_checkout,
                OperationErrorKind::RebaseConflict,
                OperationErrorDetails::Rebase {
                    branch: Some(branch.clone()),
                    worktree,
                },
                format!("Rebase conflict while moving '{branch}'"),
                "Resolve conflicts, then run `st continue`, `st abort`, or `stax undo`",
                anyhow::anyhow!("rebase conflict"),
                "rebase",
                OperationSideEffects::RepositoryChanged,
            ));
        }
        let mut updated = existing.clone();
        updated.parent_branch_name = parent.to_string();
        updated.parent_branch_revision = repo.branch_commit(parent).unwrap_or(parent_revision);
        if let Err(error) = updated.write(repo.inner(), branch) {
            return Err(finish_move_error(
                request,
                &mut transaction,
                repo,
                source,
                new_parent,
                &moved,
                &original_checkout,
                OperationErrorKind::LocalGit,
                format!("Could not update metadata for '{branch}'"),
                "Run `stax undo`, resolve the metadata error, and retry",
                error,
                "write-metadata",
                OperationSideEffects::RepositoryChanged,
            ));
        }
        transaction
            .as_mut()
            .expect("move transaction should remain active")
            .push_completed_branch(branch);
    }

    if !matches!(repo.current_branch().as_deref(), Ok(branch) if branch == original_checkout)
        && let Err(error) = repo.checkout(&original_checkout)
    {
        return Err(finish_move_error(
            request,
            &mut transaction,
            repo,
            source,
            new_parent,
            &moved,
            &original_checkout,
            OperationErrorKind::LocalGit,
            "The subtree moved, but the original checkout could not be restored",
            "Check out the original branch manually and inspect the repository",
            error,
            "restore-checkout",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    if let Err(error) = record_after_states(
        transaction
            .as_mut()
            .expect("move transaction should remain active"),
        repo,
        &moved,
    ) {
        return Err(finish_move_error(
            request,
            &mut transaction,
            repo,
            source,
            new_parent,
            &moved,
            &original_checkout,
            OperationErrorKind::LocalGit,
            "The subtree moved, but its recovery state could not be recorded",
            "Inspect the repository and retry if needed",
            error,
            "record-after",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    let mut transaction = transaction
        .take()
        .expect("move transaction should remain active");
    transaction.set_head_branch_after(&original_checkout);
    let finalized = transaction.finish_ok_preserving_receipt();
    let receipt = move_receipt(
        request,
        source,
        new_parent,
        moved,
        Some(TransactionSummary::from(&finalized.receipt)),
        OperationSideEffects::RepositoryChanged,
    );
    if let Some(error) = finalized.persistence_error {
        return Err(OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "The subtree moved, but its receipt could not be saved",
            "Inspect the repository and retry if needed",
            &error,
            Some(receipt),
            OperationSideEffects::RepositoryChanged,
        ));
    }
    Ok(receipt)
}

fn load_moved_metadata(
    request: &OperationRequest,
    repo: &GitRepo,
    moved: &[String],
) -> Result<Vec<(String, Option<BranchMetadata>)>, OperationError> {
    let mut result = Vec::with_capacity(moved.len());
    for branch in moved {
        let metadata = BranchMetadata::read(repo.inner(), branch).map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                branch_details(branch),
                format!("Could not read metadata for '{branch}'"),
                "Resolve the stax metadata error and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
        if metadata.is_none() {
            return Err(simple_error(
                request,
                OperationErrorKind::PreconditionFailed,
                format!("Branch '{branch}' is not tracked by stax"),
                "Track every branch in the subtree before moving it",
                "moved subtree contains a branch without metadata",
            ));
        }
        result.push((branch.clone(), metadata));
    }
    Ok(result)
}

fn preflight_dirty_worktrees(
    request: &OperationRequest,
    repo: &GitRepo,
    moved: &[String],
    auto_stash: bool,
) -> Result<(), OperationError> {
    if auto_stash {
        return Ok(());
    }
    let mut seen = HashSet::new();
    for branch in moved {
        let worktree = repo.branch_rebase_target_workdir(branch).map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                branch_details(branch),
                format!("Could not locate the rebase worktree for '{branch}'"),
                "Resolve the Git worktree error and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
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
                diagnostic_chain: "dirty worktree preflight rejected subtree move".into(),
                receipt: None,
                side_effects: OperationSideEffects::None,
            });
        }
    }
    Ok(())
}

fn resolve_rebase_upstream(
    repo: &GitRepo,
    old_parent: &str,
    old_revision: &str,
    branch: &str,
    merge_base: &str,
) -> anyhow::Result<String> {
    if let Ok(tip) = repo.branch_commit(old_parent)
        && repo.is_ancestor(&tip, branch)?
    {
        return Ok(tip);
    }
    if !old_revision.is_empty() && repo.is_ancestor(old_revision, branch)? {
        return Ok(old_revision.to_string());
    }
    Ok(merge_base.to_string())
}

fn record_after_states(
    transaction: &mut Transaction,
    repo: &GitRepo,
    moved: &[String],
) -> anyhow::Result<()> {
    for branch in moved {
        transaction.record_optional_after(repo, branch)?;
        transaction.record_metadata_ref_after(repo, branch)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_move_error(
    request: &OperationRequest,
    transaction: &mut Option<Transaction>,
    repo: &GitRepo,
    source: &str,
    new_parent: &str,
    moved: &[String],
    original_checkout: &str,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    error: anyhow::Error,
    step: &'static str,
    side_effects: OperationSideEffects,
) -> OperationError {
    finish_move_error_with_details(
        request,
        transaction,
        repo,
        source,
        new_parent,
        moved,
        original_checkout,
        kind,
        branch_details(source),
        primary,
        action,
        error,
        step,
        side_effects,
    )
}

#[allow(clippy::too_many_arguments)]
fn finish_move_error_with_details(
    request: &OperationRequest,
    transaction: &mut Option<Transaction>,
    repo: &GitRepo,
    source: &str,
    new_parent: &str,
    moved: &[String],
    original_checkout: &str,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: impl Into<String>,
    action: impl Into<String>,
    error: anyhow::Error,
    step: &'static str,
    side_effects: OperationSideEffects,
) -> OperationError {
    let primary = primary.into();
    let mut diagnostic = format!("{error:#}");
    let mut transaction = transaction
        .take()
        .expect("move transaction should remain active during failure");
    if let Err(error) = record_after_states(&mut transaction, repo, moved) {
        diagnostic.push_str("\nfailed to record final move state: ");
        diagnostic.push_str(&format!("{error:#}"));
    }
    transaction.set_head_branch_after(original_checkout);
    let finalized = transaction.finish_err_preserving_receipt(&primary, Some(step), Some(source));
    if let Some(error) = finalized.persistence_error {
        diagnostic.push_str("\nreceipt persistence failure: ");
        diagnostic.push_str(&format!("{error:#}"));
    }
    let receipt = move_receipt(
        request,
        source,
        new_parent,
        moved.to_vec(),
        Some(TransactionSummary::from(&finalized.receipt)),
        side_effects,
    );
    OperationError {
        request: request.clone(),
        kind,
        details,
        primary,
        action: action.into(),
        diagnostic_chain: diagnostic,
        receipt: Some(receipt),
        side_effects,
    }
}

fn move_receipt(
    request: &OperationRequest,
    source: &str,
    new_parent: &str,
    moved: Vec<String>,
    transaction: Option<TransactionSummary>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    OperationReceipt {
        request: request.clone(),
        summary: if side_effects == OperationSideEffects::None {
            format!("{source} is already parented onto {new_parent}")
        } else {
            format!("Moved {source} onto {new_parent}")
        },
        affected_branches: moved.clone(),
        outcome: OperationOutcome::SubtreeMoved {
            source: source.to_string(),
            new_parent: new_parent.to_string(),
            moved_branches: moved,
        },
        transaction,
        warnings: Vec::new(),
        side_effects,
    }
}

fn branch_details(branch: &str) -> OperationErrorDetails {
    OperationErrorDetails::Branch {
        branch: branch.to_string(),
    }
}

fn simple_error(
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

fn source_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    side_effects: OperationSideEffects,
) -> OperationError {
    OperationError::from_source(
        request.clone(),
        kind,
        details,
        primary,
        action,
        &source,
        None,
        side_effects,
    )
}
