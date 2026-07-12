#![allow(clippy::result_large_err)]

use super::branch_name::BranchNameResult;
use super::operation::report_operation;
use super::{
    BranchNameContext, BranchNameError, OperationError, OperationErrorDetails, OperationErrorKind,
    OperationEvent, OperationOutcome, OperationProgress, OperationReceipt, OperationReporter,
    OperationRequest, OperationResult, OperationSideEffects, OperationStage, RepositorySession,
    format_branch_name,
};
use crate::application::repository::MutationTargets;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use anyhow::Context;

impl RepositorySession {
    pub fn create_empty_branch(
        &self,
        name: &str,
        parent: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let result = format_branch_name(name, &BranchNameContext::literal()).map_err(|error| {
            let request = OperationRequest::CreateBranch {
                name: name.to_owned(),
                parent: parent.to_owned(),
            };
            map_branch_name_error(&request, error)
        })?;
        self.create_empty_branch_with_formatted_name(result, parent, reporter)
    }

    pub(crate) fn create_empty_branch_with_formatted_name(
        &self,
        result: BranchNameResult,
        parent: &str,
        reporter: &mut dyn OperationReporter,
    ) -> OperationResult {
        let request = OperationRequest::CreateBranch {
            name: result.name.clone(),
            parent: parent.to_owned(),
        };
        report_operation(request.clone(), reporter, |reporter| {
            self.with_mutation(
                &request,
                MutationTargets::branches([parent.to_string()]),
                || create_empty_branch_inner(self, &request, result, parent, reporter),
            )
        })
    }
}

fn create_empty_branch_inner(
    session: &RepositorySession,
    request: &OperationRequest,
    result: BranchNameResult,
    parent: &str,
    reporter: &mut dyn OperationReporter,
) -> OperationResult {
    let branch = result.name;
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
    let parent_oid = repo.branch_commit(parent).map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: parent.to_string(),
            },
            format!("Parent branch '{parent}' does not exist locally"),
            "Choose an existing local parent branch and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    reject_branch_conflict(request, &repo, &branch)?;

    reporter.report(OperationEvent::Progress(OperationProgress {
        stage: OperationStage::CreatingBranch,
        completed: 0,
        total: Some(1),
        branch: Some(branch.clone()),
        message: format!("Creating branch {branch}"),
    }));

    repo.create_branch_at_commit(&branch, &parent_oid)
        .map_err(|error| {
            OperationError::from_source(
                request.clone(),
                OperationErrorKind::LocalGit,
                OperationErrorDetails::Branch {
                    branch: branch.clone(),
                },
                format!("Could not create branch '{branch}'"),
                "Resolve the Git error and retry",
                &error,
                None,
                OperationSideEffects::None,
            )
        })?;

    let metadata = BranchMetadata::new(parent, &parent_oid);
    if let Err(error) = metadata.write(repo.inner(), &branch) {
        return Err(rollback_created_branch_error(
            request,
            &repo,
            &branch,
            false,
            error.context("metadata write failed after branch creation"),
        ));
    }

    if let Err(error) = repo.checkout(&branch) {
        return Err(rollback_created_branch_error(
            request,
            &repo,
            &branch,
            true,
            error.context("checkout failed after branch creation"),
        ));
    }

    Ok(OperationReceipt {
        request: request.clone(),
        summary: format!("Created branch {branch}"),
        affected_branches: vec![branch.clone()],
        outcome: OperationOutcome::BranchCreated {
            branch,
            parent: parent.to_string(),
        },
        transaction: None,
        warnings: result.warnings,
        side_effects: OperationSideEffects::RepositoryChanged,
    })
}

fn reject_branch_conflict(
    request: &OperationRequest,
    repo: &GitRepo,
    branch: &str,
) -> Result<(), OperationError> {
    let existing = repo.list_branches().map_err(|error| {
        OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not inspect local branches",
            "Resolve the Git error and retry",
            &error,
            None,
            OperationSideEffects::None,
        )
    })?;
    if let Some(conflict) = existing.iter().find(|existing| {
        existing.as_str() == branch
            || branch.starts_with(&format!("{existing}/"))
            || existing.starts_with(&format!("{branch}/"))
    }) {
        return Err(operation_error(
            request,
            OperationErrorKind::InvalidInput,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Branch '{branch}' conflicts with existing branch '{conflict}'"),
            "Choose a different branch name and retry",
            "local branch ref namespace conflict",
            OperationSideEffects::None,
        ));
    }
    Ok(())
}

fn rollback_created_branch_error(
    request: &OperationRequest,
    repo: &GitRepo,
    branch: &str,
    delete_metadata: bool,
    original: anyhow::Error,
) -> OperationError {
    match rollback_created_branch(repo, branch, delete_metadata) {
        Ok(()) => OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::Branch {
                branch: branch.to_string(),
            },
            format!("Could not create branch '{branch}'"),
            "Resolve the Git error and retry",
            &original,
            None,
            OperationSideEffects::None,
        ),
        Err(rollback) => {
            let diagnostic =
                format!("original error:\n{original:#}\n\nrollback error:\n{rollback:#}");
            operation_error(
                request,
                OperationErrorKind::LocalGit,
                OperationErrorDetails::Branch {
                    branch: branch.to_string(),
                },
                format!("Could not create branch '{branch}'"),
                "Resolve the Git error, inspect the repository, and retry",
                diagnostic,
                OperationSideEffects::RepositoryChanged,
            )
        }
    }
}

fn rollback_created_branch(
    repo: &GitRepo,
    branch: &str,
    delete_metadata: bool,
) -> anyhow::Result<()> {
    if delete_metadata {
        BranchMetadata::delete(repo.inner(), branch)
            .with_context(|| format!("failed to delete metadata for '{branch}'"))?;
    }
    repo.delete_branch(branch, true)
        .with_context(|| format!("failed to delete branch '{branch}'"))?;
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
