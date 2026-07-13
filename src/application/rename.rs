#![allow(clippy::result_large_err)]

use super::branch_name::BranchNameResult;
use super::operation::report_operation;
use super::{
    BranchNameContext, BranchNameError, OperationError, OperationErrorDetails, OperationErrorKind,
    OperationEvent, OperationOutcome, OperationProgress, OperationReceipt, OperationReporter,
    OperationRequest, OperationResult, OperationSideEffects, OperationStage, RepositorySession,
    TransactionSummary, format_branch_name,
};
use crate::application::repository::MutationTargets;
use crate::engine::{BranchMetadata, Stack};
use crate::git::GitRepo;
use crate::ops::receipt::OpKind;
use crate::ops::tx::Transaction;
use anyhow::anyhow;
use git2::BranchType;

impl RepositorySession {
    /// Rename the current local branch using a literal new name.
    pub fn rename_branch(
        &self,
        branch: &str,
        new_name: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::RenameBranch {
            branch: branch.to_owned(),
            new_name: new_name.to_owned(),
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.rename_branch_unframed(&request, branch, new_name, reporter)
        })
    }

    pub(super) fn rename_branch_unframed(
        &self,
        request: &OperationRequest,
        branch: &str,
        new_name: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let formatted = format_branch_name(new_name, &BranchNameContext::literal())
            .map_err(|error| map_branch_name_error(request, error))?;
        self.with_mutation(
            request,
            MutationTargets::branches([branch.to_string()]),
            || rename_branch_inner(self, request, branch, formatted, reporter),
        )
    }
}

fn rename_branch_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    branch: &str,
    formatted: BranchNameResult,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let branch = branch.trim();
    let new_name = formatted.name;
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
            format!("Cannot rename the trunk branch '{trunk}'"),
            "Choose a non-trunk branch and retry",
            "rename target is the configured trunk branch",
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
    if branch != current {
        return Err(operation_error(
            request,
            OperationErrorKind::PreconditionFailed,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' is not the current branch"),
            "Check out the branch before renaming it",
            format!("current branch is '{current}'"),
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
    reject_branch_conflict(request, &repo, branch, &new_name)?;

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
    let children = stack
        .branches
        .iter()
        .filter(|(_, info)| info.parent.as_deref() == Some(branch))
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    let old_metadata = BranchMetadata::read(repo.inner(), branch).map_err(|error| {
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
    let mut child_metadata = Vec::with_capacity(children.len());
    for child in &children {
        let metadata = BranchMetadata::read(repo.inner(), child).map_err(|error| {
            source_error(
                request,
                OperationErrorKind::LocalGit,
                OperationErrorDetails::Branch {
                    branch: child.clone(),
                },
                format!("Could not read metadata for '{child}'"),
                "Resolve the stax metadata error and retry",
                error,
                OperationSideEffects::None,
            )
        })?;
        child_metadata.push((child.clone(), metadata));
    }

    let mut transaction = Transaction::begin(OpKind::Rename, &repo, true).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not start the rename transaction",
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
            "Could not prepare the rename transaction",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    transaction.plan_branch(&repo, &new_name).map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not prepare the rename transaction",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    for metadata_branch in std::iter::once(branch)
        .chain(std::iter::once(new_name.as_str()))
        .chain(children.iter().map(String::as_str))
    {
        transaction
            .plan_metadata_ref(&repo, metadata_branch)
            .map_err(|error| {
                source_error(
                    request,
                    OperationErrorKind::LocalGit,
                    OperationErrorDetails::None,
                    "Could not prepare metadata recovery for the rename",
                    "Resolve the receipt error and retry",
                    error,
                    OperationSideEffects::None,
                )
            })?;
    }
    transaction.snapshot().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not save the rename recovery snapshot",
            "Resolve the receipt error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::RenamingBranch,
        completed: 0,
        total: Some(1),
        branch: Some(branch.to_string()),
        message: format!("Renaming {branch} to {new_name}"),
    }));

    let rename_result = repo
        .inner()
        .find_branch(branch, BranchType::Local)
        .and_then(|mut local| local.rename(&new_name, false));
    if let Err(error) = rename_result {
        return Err(finish_rename_error(
            request,
            transaction,
            &repo,
            branch,
            &new_name,
            &children,
            formatted.warnings,
            "Could not rename the local branch",
            "Resolve the Git ref error and retry",
            anyhow!(error),
            "rename-ref",
            OperationSideEffects::None,
        ));
    }

    if let Some(metadata) = old_metadata
        && let Err(error) = metadata.write(repo.inner(), &new_name)
    {
        return Err(finish_rename_error(
            request,
            transaction,
            &repo,
            branch,
            &new_name,
            &children,
            formatted.warnings,
            "Branch renamed, but its metadata could not be moved",
            "Run `stax undo`, resolve the metadata error, and retry",
            error,
            "write-metadata",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    for (child, metadata) in child_metadata {
        let Some(mut metadata) = metadata else {
            continue;
        };
        metadata.parent_branch_name = new_name.clone();
        if let Err(error) = metadata.write(repo.inner(), &child) {
            return Err(finish_rename_error(
                request,
                transaction,
                &repo,
                branch,
                &new_name,
                &children,
                formatted.warnings,
                format!("Branch renamed, but child metadata for '{child}' could not be updated"),
                "Run `stax undo`, resolve the metadata error, and retry",
                error,
                "update-child-metadata",
                OperationSideEffects::RepositoryChanged,
            ));
        }
    }
    if let Err(error) = BranchMetadata::delete(repo.inner(), branch) {
        return Err(finish_rename_error(
            request,
            transaction,
            &repo,
            branch,
            &new_name,
            &children,
            formatted.warnings,
            "Branch renamed, but its old metadata could not be removed",
            "Run `stax undo`, resolve the metadata error, and retry",
            error,
            "delete-old-metadata",
            OperationSideEffects::RepositoryChanged,
        ));
    }

    if let Err(error) = record_after_states(&mut transaction, &repo, branch, &new_name, &children) {
        return Err(finish_rename_error(
            request,
            transaction,
            &repo,
            branch,
            &new_name,
            &children,
            formatted.warnings,
            "Branch renamed, but its recovery state could not be recorded",
            "Inspect the repository and retry if needed",
            error,
            "record-after",
            OperationSideEffects::RepositoryChanged,
        ));
    }
    transaction.set_head_branch_after(&new_name);
    let finalized = transaction.finish_ok_preserving_receipt();
    let transaction_summary = TransactionSummary::from(&finalized.receipt);
    let receipt = rename_receipt(
        request,
        branch,
        &new_name,
        &children,
        formatted.warnings,
        Some(transaction_summary),
        OperationSideEffects::RepositoryChanged,
    );
    if let Some(error) = finalized.persistence_error {
        return Err(OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Branch renamed, but its receipt could not be saved",
            "Inspect the repository and retry if needed",
            &error,
            Some(receipt),
            OperationSideEffects::RepositoryChanged,
        ));
    }
    Ok(receipt)
}

fn record_after_states(
    transaction: &mut Transaction,
    repo: &GitRepo,
    branch: &str,
    new_name: &str,
    children: &[String],
) -> anyhow::Result<()> {
    transaction.record_optional_after(repo, branch)?;
    transaction.record_optional_after(repo, new_name)?;
    transaction.record_metadata_ref_after(repo, branch)?;
    transaction.record_metadata_ref_after(repo, new_name)?;
    for child in children {
        transaction.record_metadata_ref_after(repo, child)?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish_rename_error(
    request: &OperationRequest,
    mut transaction: Transaction,
    repo: &GitRepo,
    branch: &str,
    new_name: &str,
    children: &[String],
    warnings: Vec<super::OperationWarning>,
    primary: impl Into<String>,
    action: impl Into<String>,
    source: anyhow::Error,
    failed_step: &'static str,
    side_effects: OperationSideEffects,
) -> OperationError {
    let primary = primary.into();
    let mut diagnostic_chain = format!("{source:#}");
    if let Err(error) = record_after_states(&mut transaction, repo, branch, new_name, children) {
        diagnostic_chain.push_str("\nfailed to record final ref state: ");
        diagnostic_chain.push_str(&format!("{error:#}"));
    }
    let finalized =
        transaction.finish_err_preserving_receipt(&primary, Some(failed_step), Some(branch));
    if let Some(error) = finalized.persistence_error {
        diagnostic_chain.push_str("\nreceipt persistence failure: ");
        diagnostic_chain.push_str(&format!("{error:#}"));
    }
    let receipt = rename_receipt(
        request,
        branch,
        new_name,
        children,
        warnings,
        Some(TransactionSummary::from(&finalized.receipt)),
        side_effects,
    );
    OperationError {
        request: request.clone(),
        kind: OperationErrorKind::LocalGit,
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

fn rename_receipt(
    request: &OperationRequest,
    branch: &str,
    new_name: &str,
    children: &[String],
    warnings: Vec<super::OperationWarning>,
    transaction: Option<TransactionSummary>,
    side_effects: OperationSideEffects,
) -> OperationReceipt {
    let mut affected_branches = vec![branch.to_string(), new_name.to_string()];
    affected_branches.extend(children.iter().cloned());
    OperationReceipt {
        request: request.clone(),
        summary: format!("Renamed {branch} to {new_name}"),
        affected_branches,
        outcome: OperationOutcome::BranchRenamed {
            old_name: branch.to_string(),
            new_name: new_name.to_string(),
        },
        transaction,
        warnings,
        side_effects,
    }
}

fn reject_branch_conflict(
    request: &OperationRequest,
    repo: &GitRepo,
    branch: &str,
    new_name: &str,
) -> Result<(), OperationError> {
    if new_name == branch {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: new_name.to_string(),
            },
            "The new branch name matches the current name",
            "Choose a different branch name and retry",
            "rename would not change the ref name",
            OperationSideEffects::None,
        ));
    }
    let existing = repo.list_branches().map_err(|error| {
        source_error(
            request,
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not inspect local branches",
            "Resolve the Git error and retry",
            error,
            OperationSideEffects::None,
        )
    })?;
    if let Some(conflict) = existing.iter().find(|existing| {
        existing.as_str() != branch
            && (existing.as_str() == new_name
                || new_name.starts_with(&format!("{existing}/"))
                || existing.starts_with(&format!("{new_name}/")))
    }) {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: new_name.to_string(),
            },
            format!("Branch '{new_name}' conflicts with existing branch '{conflict}'"),
            "Choose a different branch name and retry",
            "local branch ref namespace conflict",
            OperationSideEffects::None,
        ));
    }
    Ok(())
}

fn map_branch_name_error(request: &OperationRequest, error: BranchNameError) -> OperationError {
    let diagnostic = format!("{error:?}");
    let (primary, action) = match &error {
        BranchNameError::Empty => (
            "Branch name is required".to_string(),
            "Choose a non-empty branch name and retry".to_string(),
        ),
        BranchNameError::MissingMessagePlaceholder { .. } => (
            "Branch name format is invalid".to_string(),
            "Include {message} in the branch name format and retry".to_string(),
        ),
        BranchNameError::InvalidRef { candidate } => (
            format!("Branch name '{candidate}' is not a valid Git ref"),
            "Choose a valid branch name and retry".to_string(),
        ),
    };
    operation_error(
        request,
        OperationErrorKind::InvalidInput,
        OperationErrorDetails::None,
        primary,
        action,
        diagnostic,
        OperationSideEffects::None,
    )
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
