#![allow(clippy::result_large_err)]

use super::operation::report_operation;
use super::{
    CheckoutOutcome, OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent,
    OperationOutcome, OperationProgress, OperationReceipt, OperationReporter, OperationRequest,
    OperationResult, OperationSideEffects, OperationStage, RepositorySession,
};
use crate::application::repository::MutationTargets;
use git2::BranchType;
use std::path::Path;

impl RepositorySession {
    pub fn checkout(&self, branch: &str, reporter: &mut dyn OperationReporter) -> OperationResult {
        let request = OperationRequest::Checkout {
            branch: branch.to_owned(),
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.checkout_unframed(&request, branch, reporter)
        })
    }

    pub(super) fn checkout_unframed(
        &self,
        request: &OperationRequest,
        branch: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        self.with_mutation(request, MutationTargets::branches([branch]), || {
            checkout_explicit(self, request, branch, reporter)
        })
    }
}

fn checkout_explicit(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
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
            "checkout branch input was empty",
            OperationSideEffects::None,
        ));
    }

    let repo = session.open_repo().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::RepositoryUnavailable,
            OperationErrorDetails::None,
            "Could not open the repository",
            "Check the repository path and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    if repo.inner().find_branch(branch, BranchType::Local).is_err() {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' does not exist locally"),
            "Choose an existing local branch and retry",
            "local branch lookup failed",
            OperationSideEffects::None,
        ));
    }

    if matches!(repo.current_branch(), Ok(current) if current == branch) {
        return Ok(checkout_receipt(
            request,
            CheckoutOutcome::AlreadyCurrent {
                branch: branch.to_string(),
            },
            OperationSideEffects::None,
        ));
    }

    reject_linked_worktree_checkout(session.repository_root(), request, branch, &repo)?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::CheckingOut,
        completed: 0,
        total: Some(1),
        branch: Some(branch.to_string()),
        message: format!("Checking out {branch}"),
    }));
    repo.checkout(branch).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Could not check out branch '{branch}'"),
            "Resolve the Git checkout error and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;

    Ok(checkout_receipt(
        request,
        CheckoutOutcome::CheckedOut {
            branch: branch.to_string(),
        },
        OperationSideEffects::RepositoryChanged,
    ))
}

fn reject_linked_worktree_checkout(
    repository_root: &Path,
    request: &OperationRequest,
    branch: &str,
    repo: &crate::git::GitRepo,
) -> Result<(), OperationError> {
    let current_root = repository_root.canonicalize().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::RepositoryUnavailable,
            OperationErrorDetails::None,
            "Could not inspect the repository path",
            "Check the repository path and retry",
            &anyhow::Error::from(error).context("failed to canonicalize repository root"),
            None,
            OperationSideEffects::None,
        )
    })?;
    for worktree in repo.list_worktrees().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not inspect repository worktrees",
            "Resolve the Git worktree error and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })? {
        if worktree.branch.as_deref() != Some(branch) {
            continue;
        }
        let path = worktree.path.canonicalize().unwrap_or(worktree.path);
        if path == current_root {
            continue;
        }
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::AlreadyCheckedOutElsewhere {
                branch: branch.to_string(),
                path,
            },
            format!("Branch '{branch}' is checked out in another worktree"),
            "Switch that worktree to another branch or remove it, then retry",
            "target branch is already checked out in a linked worktree",
            OperationSideEffects::None,
        ));
    }
    Ok(())
}

fn checkout_receipt(
    request: &OperationRequest,
    outcome: CheckoutOutcome,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let branch = match &outcome {
        CheckoutOutcome::CheckedOut { branch } | CheckoutOutcome::AlreadyCurrent { branch } => {
            branch.clone()
        }
    };
    let summary = match &outcome {
        CheckoutOutcome::CheckedOut { branch } => format!("Checked out {branch}"),
        CheckoutOutcome::AlreadyCurrent { branch } => format!("Already on {branch}"),
    };
    OperationReceipt {
        request: request.clone(),
        summary,
        affected_branches: vec![branch],
        outcome: OperationOutcome::Checkout(outcome),
        transaction: None,
        warnings: Vec::new(),
        side_effects,
    }
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
