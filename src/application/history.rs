#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationReporter, OperationRequest, OperationResult,
    OperationSideEffects, OperationStage, RepositorySession, TransactionSummary,
};
use crate::application::repository::MutationTargets;
use crate::ops::receipt::OpReceipt;

impl RepositorySession {
    /// Restore a transaction's local before-state.
    pub fn undo_transaction(
        &self,
        operation_id: Option<&str>,
        update_remote: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::UndoTransaction {
            operation_id: operation_id.map(str::to_string),
            update_remote,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.undo_transaction_unframed(&request, operation_id, update_remote, reporter)
        })
    }

    /// Restore a transaction's local after-state.
    pub fn redo_transaction(
        &self,
        operation_id: Option<&str>,
        update_remote: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::RedoTransaction {
            operation_id: operation_id.map(str::to_string),
            update_remote,
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.redo_transaction_unframed(&request, operation_id, update_remote, reporter)
        })
    }

    pub(super) fn undo_transaction_unframed(
        &self,
        request: &OperationRequest,
        operation_id: Option<&str>,
        update_remote: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(request, MutationTargets::current(), || {
            apply_history(self, request, operation_id, update_remote, false, reporter)
        })
    }

    pub(super) fn redo_transaction_unframed(
        &self,
        request: &OperationRequest,
        operation_id: Option<&str>,
        update_remote: bool,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(request, MutationTargets::current(), || {
            apply_history(self, request, operation_id, update_remote, true, reporter)
        })
    }
}

fn apply_history(
    session: &RepositorySession,
    request: &OperationRequest,
    operation_id: Option<&str>,
    update_remote: bool,
    redo: bool,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let repo = session
        .open_repo()
        .map_err(|error| source_error(request, error))?;
    let receipt = match operation_id {
        Some(id) => OpReceipt::load(
            repo.git_dir()
                .map_err(|error| source_error(request, error))?,
            id,
        ),
        None => OpReceipt::load_latest(
            repo.git_dir()
                .map_err(|error| source_error(request, error))?,
        )
        .and_then(|receipt| receipt.ok_or_else(|| anyhow::anyhow!("No operations are available"))),
    }
    .map_err(|error| {
        input_error(
            request,
            OperationErrorKind::InvalidInput,
            "Could not find the requested operation",
            "Choose an existing operation and retry",
            error,
        )
    })?;
    if (redo && !receipt.can_redo()) || (!redo && !receipt.can_undo()) {
        return Err(input_error(
            request,
            OperationErrorKind::PreconditionFailed,
            if redo {
                "This operation cannot be redone"
            } else {
                "This operation cannot be undone"
            },
            "Choose another operation",
            anyhow::anyhow!("receipt transition is unavailable"),
        ));
    }
    if repo
        .is_dirty()
        .map_err(|error| source_error(request, error))?
    {
        return Err(OperationError {
            request: request.clone(),
            kind: OperationErrorKind::DirtyWorktree,
            details: OperationErrorDetails::None,
            primary: "The working tree has uncommitted changes".into(),
            action: "Commit or stash changes before continuing".into(),
            diagnostic_chain: "history operation requires a clean worktree".into(),
            receipt: None,
            side_effects: OperationSideEffects::None,
        });
    }
    if update_remote && receipt.has_remote_changes() {
        return Err(input_error(
            request,
            OperationErrorKind::UnsupportedCapability,
            "Remote history restoration is not available through this application operation",
            "Use the explicit CLI remote recovery flow",
            anyhow::anyhow!("remote update requested"),
        ));
    }
    let stage = if redo {
        OperationStage::RedoingTransaction
    } else {
        OperationStage::UndoingTransaction
    };
    let mut changed = Vec::new();
    for (index, entry) in receipt.local_refs.iter().enumerate() {
        reporter.report(OperationEvent::Progress(OperationProgress {
            stage,
            completed: index,
            total: Some(receipt.local_refs.len()),
            branch: Some(entry.branch.clone()),
            message: format!("Restoring {}", entry.branch),
        }));
        let target = if redo {
            entry.oid_after.as_deref()
        } else {
            entry.oid_before.as_deref()
        };
        let absence_recorded = if redo {
            entry.after_recorded
        } else {
            !entry.existed_before
        };
        match target {
            Some(oid) => repo.update_ref(&entry.refname, oid),
            None if absence_recorded => repo.delete_ref(&entry.refname),
            None => continue,
        }
        .map_err(|error| changed_error(request, &receipt, redo, changed.clone(), error))?;
        changed.push(entry.refname.clone());
    }
    let checkout = if redo {
        receipt.redo_head_branch()
    } else {
        receipt.undo_head_branch()
    };
    repo.checkout(checkout)
        .map_err(|error| changed_error(request, &receipt, redo, changed.clone(), error))?;
    if let Some(entry) = receipt
        .local_refs
        .iter()
        .find(|entry| entry.branch == checkout)
    {
        let oid = if redo {
            entry.oid_after.as_deref()
        } else {
            entry.oid_before.as_deref()
        };
        if let Some(oid) = oid {
            repo.reset_hard(oid)
                .map_err(|error| changed_error(request, &receipt, redo, changed.clone(), error))?;
        }
    }
    Ok(history_receipt(
        request,
        &receipt,
        redo,
        changed,
        OperationSideEffects::RepositoryChanged,
    ))
}

fn history_receipt(
    request: &OperationRequest,
    receipt: &OpReceipt,
    redo: bool,
    changed: Vec<String>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let outcome = if redo {
        OperationOutcome::TransactionRedone {
            operation_id: receipt.op_id.clone(),
            changed_refs: changed.clone(),
        }
    } else {
        OperationOutcome::TransactionUndone {
            operation_id: receipt.op_id.clone(),
            changed_refs: changed.clone(),
        }
    };
    OperationReceipt {
        request: request.clone(),
        summary: format!(
            "{} operation {}",
            if redo { "Redid" } else { "Undid" },
            receipt.op_id
        ),
        affected_branches: receipt.summary_branch_names(),
        outcome,
        transaction: Some(TransactionSummary::from(receipt)),
        warnings: Vec::new(),
        side_effects,
    }
}

fn changed_error(
    request: &OperationRequest,
    receipt: &OpReceipt,
    redo: bool,
    changed: Vec<String>,
    source: anyhow::Error,
) -> OperationError {
    let retained = history_receipt(
        request,
        receipt,
        redo,
        changed,
        OperationSideEffects::RepositoryChanged,
    );
    OperationError::from_source(
        request.clone(),
        OperationErrorKind::LocalGit,
        OperationErrorDetails::None,
        "Could not finish restoring transaction refs",
        "Inspect the repository and retry recovery",
        &source,
        Some(retained),
        OperationSideEffects::RepositoryChanged,
    )
}

fn source_error(request: &OperationRequest, source: anyhow::Error) -> OperationError {
    OperationError::from_source(
        request.clone(),
        OperationErrorKind::LocalGit,
        OperationErrorDetails::None,
        "Could not inspect transaction history",
        "Resolve the repository error and retry",
        &source,
        None,
        OperationSideEffects::None,
    )
}

fn input_error(
    request: &OperationRequest,
    kind: OperationErrorKind,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
) -> OperationError {
    OperationError::from_source(
        request.clone(),
        kind,
        OperationErrorDetails::None,
        primary,
        action,
        &source,
        None,
        OperationSideEffects::None,
    )
}
