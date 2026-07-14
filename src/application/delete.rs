#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, OperationWarning, RepositorySession, TransactionSummary,
};
use crate::application::repository::MutationTargets;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;

impl RepositorySession {
    /// Delete a local branch while preserving its descendants unchanged.
    pub fn delete_branch(
        &self,
        branch: &str,
        force: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::DeleteBranch {
            branch: branch.to_owned(),
            force,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.delete_branch_unframed(&request, branch, force, reporter)
        })
    }

    pub(super) fn delete_branch_unframed(
        &self,
        request: &OperationRequest,
        branch: &str,
        force: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(
            request,
            MutationTargets::branches([branch.to_string()]),
            || delete_branch_inner(self, request, branch, force, reporter),
        )
    }
}

fn delete_branch_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
    force: bool,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let branch = branch.trim();
    if branch.is_empty() {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::None,
            "Branch name is required",
            "Choose a local branch and retry",
            "delete branch input was empty",
            OperationSideEffects::None,
        ));
    }
    let repo = session.open_repo().map_err(|error| {
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
    let trunk = repo.trunk_branch().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not determine the trunk branch",
            "Resolve the stax metadata error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    if branch == trunk {
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Cannot delete the trunk branch '{trunk}'"),
            "Choose a non-trunk branch and retry",
            "delete target is the configured trunk branch",
            OperationSideEffects::None,
        ));
    }
    let current = repo.current_branch().map_err(|error| {
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
    if branch == current {
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            "Cannot delete the current branch",
            "Check out a different branch and retry",
            "delete target is checked out in the current worktree",
            OperationSideEffects::None,
        ));
    }
    repo.branch_commit(branch).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' does not exist locally"),
            "Choose an existing local branch and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    if let Some(hint) = repo
        .branch_delete_resolution_hint(branch)
        .map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                OperationErrorDetails::Branch {
                    branch: branch.to_string(),
                },
                "Could not inspect branch worktrees",
                "Resolve the Git worktree error and retry",
                error,
                OperationSideEffects::None,
            )
        })?
    {
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' is checked out in another worktree"),
            hint,
            "delete target is checked out in a linked worktree",
            OperationSideEffects::None,
        ));
    }

    let metadata = BranchMetadata::read(repo.inner(), branch).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Could not read metadata for '{branch}'"),
            "Resolve the stax metadata error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    if !force && !is_merged_into_parent_or_trunk(&repo, branch, &trunk, metadata.as_ref()) {
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' is not merged"),
            "Retry with force only if discarding its commits is intended",
            "branch is not an ancestor of its parent or trunk",
            OperationSideEffects::None,
        ));
    }

    let stack = Stack::load(&repo).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not load stack metadata",
            "Resolve the stax metadata error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    let descendants = stack.descendants(branch);
    let warnings = (!descendants.is_empty())
        .then(|| OperationWarning::DescendantsRetained {
            deleted_branch: branch.to_string(),
            descendants: descendants.clone(),
        })
        .into_iter()
        .collect::<Vec<_>>();

    let mut transaction = Transaction::begin(OpKind::Delete, &repo, true).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not start the deletion transaction",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    transaction.plan_branch(&repo, branch).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not prepare branch recovery for deletion",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    transaction
        .plan_metadata_ref(&repo, branch)
        .map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                OperationErrorDetails::None,
                "Could not prepare metadata recovery for deletion",
                "Resolve the receipt error and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
    transaction.snapshot().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not save the deletion recovery snapshot",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::DeletingBranch,
        completed: 0,
        total: Some(1),
        branch: Some(branch.to_string()),
        message: format!("Deleting {branch}"),
    }));
    if let Err(error) = repo.delete_branch(branch, force) {
        return Err(finish_delete_error(
            request,
            transaction,
            &repo,
            branch,
            &descendants,
            warnings,
            if force {
                OperationErrorKind::LocalGit
            } else {
                OperationErrorKind::PreconditionFailed
            },
            format!("Could not delete branch '{branch}'"),
            "Resolve the Git branch error and retry",
            error,
            "delete-ref",
            OperationSideEffects::None,
        ));
    }
    if let Err(error) = BranchMetadata::delete(repo.inner(), branch) {
        return Err(finish_delete_error(
            request,
            transaction,
            &repo,
            branch,
            &descendants,
            warnings,
            OperationErrorKind::LocalGit,
            "Branch deleted, but its metadata could not be removed",
            "Run `stax undo`, resolve the metadata error, and retry",
            error,
            "delete-metadata",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    if let Err(error) = record_after_states(&mut transaction, &repo, branch) {
        return Err(finish_delete_error(
            request,
            transaction,
            &repo,
            branch,
            &descendants,
            warnings,
            OperationErrorKind::LocalGit,
            "Branch deleted, but its recovery state could not be recorded",
            "Inspect the repository and retry if needed",
            error,
            "record-after",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    transaction.set_head_branch_after(&current);
    let finalized = transaction.finish_ok_preserving_receipt();
    let receipt = delete_receipt(
        request,
        branch,
        &descendants,
        warnings,
        Some(TransactionSummary::from(&finalized.receipt)),
        OperationSideEffects::RepositoryChanged,
    );
    if let Some(error) = finalized.persistence_error {
        return Err(OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Branch deleted, but its receipt could not be saved",
            "Inspect the repository and retry if needed",
            &error,
            Some(receipt),
            OperationSideEffects::RepositoryChanged,
        ));
    }
    Ok(receipt)
}

fn is_merged_into_parent_or_trunk(
    repo: &GitRepo,
    branch: &str,
    trunk: &str,
    metadata: Option<&BranchMetadata>,
) -> bool {
    let mut candidates = metadata
        .map(|metadata| vec![metadata.parent_branch_name.as_str()])
        .unwrap_or_default();
    if !candidates.contains(&trunk) {
        candidates.push(trunk);
    }
    candidates
        .into_iter()
        .any(|base| repo.is_ancestor(branch, base).unwrap_or(false))
}

fn record_after_states(
    transaction: &mut Transaction,
    repo: &GitRepo,
    branch: &str,
) -> anyhow::Result<()> {
    transaction.record_optional_after(repo, branch)?;
    transaction.record_metadata_ref_after(repo, branch)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_delete_error(
    request: &OperationRequest,
    mut transaction: Transaction,
    repo: &GitRepo,
    branch: &str,
    descendants: &[String],
    warnings: Vec<OperationWarning>,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    failed_step: &'static str,
    side_effects: OperationSideEffects,
) -> OperationError {
    let primary = primary.into();
    let mut diagnostic_chain = format!("{source:#}");
    if let Err(error) = record_after_states(&mut transaction, repo, branch) {
        diagnostic_chain.push_str("\nfailed to record final ref state: ");
        diagnostic_chain.push_str(&format!("{error:#}"));
    }
    let finalized =
        transaction.finish_err_preserving_receipt(&primary, Some(failed_step), Some(branch));
    if let Some(error) = finalized.persistence_error {
        diagnostic_chain.push_str("\nreceipt persistence failure: ");
        diagnostic_chain.push_str(&format!("{error:#}"));
    }
    let receipt = delete_receipt(
        request,
        branch,
        descendants,
        warnings,
        Some(TransactionSummary::from(&finalized.receipt)),
        side_effects,
    );
    OperationError {
        request: request.clone(),
        kind,
        details: OperationErrorDetails::Branch {
            branch: branch.to_string(),
        },
        primary,
        action: action.into(),
        diagnostic_chain,
        receipt: Some(receipt),
        side_effects,
    }
}

fn delete_receipt(
    request: &OperationRequest,
    branch: &str,
    descendants: &[String],
    warnings: Vec<OperationWarning>,
    transaction: Option<TransactionSummary>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let mut affected_branches = vec![branch.to_string()];
    affected_branches.extend(descendants.iter().cloned());
    OperationReceipt {
        request: request.clone(),
        summary: format!("Deleted {branch}"),
        affected_branches,
        outcome: OperationOutcome::BranchDeleted {
            branch: branch.to_string(),
            retained_descendants: descendants.to_vec(),
        },
        transaction,
        warnings,
        side_effects,
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

fn operation_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    details: OperationErrorDetails,
    primary: impl Into<String>,
    action: impl Into<String>,
    diagnostic_chain: impl Into<String>,
    side_effects: OperationSideEffects,
) -> OperationError {
    OperationError {
        request: request.clone(),
        kind,
        details,
        primary: primary.into(),
        action: action.into(),
        diagnostic_chain: diagnostic_chain.into(),
        receipt: None,
        side_effects,
    }
}
