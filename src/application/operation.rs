#![allow(clippy::result_large_err)]

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
    Checkout {
        branch: String,
    },
    CreateBranch {
        name: String,
        parent: String,
    },
    Restack {
        scope: RestackScope,
        auto_stash: bool,
    },
    SubmitStack {
        new_pull_requests: PullRequestMode,
    },
    ResolvePullRequestUrl {
        branch: String,
    },
}

impl OperationRequest {
    pub fn is_mutating(&self) -> bool {
        !matches!(self, Self::ResolvePullRequestUrl { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestackScope {
    Branch(String),
    StackContaining(String),
    ThroughBranch(String),
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestMode {
    Draft,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStage {
    Validating,
    Preparing,
    CheckingOut,
    CreatingBranch,
    Restacking,
    Pushing,
    UpdatingPullRequests,
    ResolvingPullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationProgress {
    pub stage: OperationStage,
    pub completed: usize,
    pub total: Option<usize>,
    pub branch: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationEvent {
    Started(OperationRequest),
    Progress(OperationProgress),
    Completed(OperationReceipt),
    Failed(OperationError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSideEffects {
    None,
    RepositoryChanged,
    RemoteMayHaveChanged,
}

impl OperationSideEffects {
    pub fn requires_refresh(self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationReceipt {
    pub request: OperationRequest,
    pub summary: String,
    pub affected_branches: Vec<String>,
    pub outcome: OperationOutcome,
    pub transaction: Option<TransactionSummary>,
    pub warnings: Vec<OperationWarning>,
    pub side_effects: OperationSideEffects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    Checkout(CheckoutOutcome),
    BranchCreated {
        branch: String,
        parent: String,
    },
    Restacked {
        branches: Vec<String>,
        skipped_frozen: Vec<String>,
    },
    Submitted {
        pull_requests: Vec<PullRequestReceipt>,
    },
    PullRequestResolved {
        branch: String,
        url: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutOutcome {
    CheckedOut { branch: String },
    AlreadyCurrent { branch: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestChange {
    Created,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestReceipt {
    pub branch: String,
    pub number: u64,
    pub url: String,
    pub change: PullRequestChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    InProgress,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionSummary {
    pub id: String,
    pub kind: String,
    pub status: TransactionStatus,
    pub branches: Vec<String>,
    pub can_undo: bool,
    pub changed_remote_refs: bool,
}

impl From<&crate::ops::receipt::OpReceipt> for TransactionSummary {
    fn from(receipt: &crate::ops::receipt::OpReceipt) -> Self {
        use crate::ops::receipt::OpStatus;

        Self {
            id: receipt.summary_id().to_string(),
            kind: receipt.summary_kind().to_string(),
            status: match receipt.summary_status() {
                OpStatus::InProgress => TransactionStatus::InProgress,
                OpStatus::Success => TransactionStatus::Succeeded,
                OpStatus::Failed => TransactionStatus::Failed,
            },
            branches: receipt.summary_branch_names(),
            can_undo: receipt.can_undo(),
            changed_remote_refs: receipt.changed_remote_refs(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationWarning {
    BranchNameNormalized {
        original: String,
        normalized: String,
    },
    RestackBoundaryAdjusted {
        branch: String,
        reason: String,
    },
    StashRestoreFailed {
        worktree: PathBuf,
        diagnostic: String,
    },
    SubmitReviewersUnsupported {
        provider: String,
        reviewers: Vec<String>,
    },
    SubmitNativeStackAdvisory {
        reason: NativeStackAdvisory,
        message: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStackAdvisory {
    GhUnavailable,
    ExtensionMissing,
    ExtensionOutdated,
    ForkedStack,
    AuthenticationUnsupported,
    FeatureDisabled,
    LinkRejected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationErrorKind {
    RepositoryUnavailable,
    InitializationRequired,
    Authentication,
    Authorization,
    DirtyWorktree,
    PreconditionFailed,
    RebaseInProgress,
    RebaseConflict,
    LocalGit,
    Network,
    PartialRemoteUpdate,
    UnsupportedCapability,
    Busy,
    InvalidInput,
    Runtime,
    Internal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationErrorDetails {
    None,
    Branch {
        branch: String,
    },
    PullRequest {
        branch: String,
    },
    AlreadyCheckedOutElsewhere {
        branch: String,
        path: PathBuf,
    },
    Rebase {
        branch: Option<String>,
        worktree: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationError {
    pub request: OperationRequest,
    pub kind: OperationErrorKind,
    pub details: OperationErrorDetails,
    pub primary: String,
    pub action: String,
    pub diagnostic_chain: String,
    pub receipt: Option<OperationReceipt>,
    pub side_effects: OperationSideEffects,
}

impl OperationError {
    #[allow(dead_code)]
    pub(crate) fn from_source(
        request: OperationRequest,
        kind: OperationErrorKind,
        details: OperationErrorDetails,
        primary: impl Into<String>,
        action: impl Into<String>,
        source: &anyhow::Error,
        receipt: Option<OperationReceipt>,
        side_effects: OperationSideEffects,
    ) -> Self {
        Self {
            request,
            kind,
            details,
            primary: primary.into(),
            action: action.into(),
            diagnostic_chain: format!("{source:#}"),
            receipt,
            side_effects,
        }
    }
}

impl std::fmt::Display for OperationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.primary)
    }
}

impl std::error::Error for OperationError {}

pub trait OperationReporter {
    fn report(&mut self, event: OperationEvent);
}

impl<F> OperationReporter for F
where
    F: FnMut(OperationEvent),
{
    fn report(&mut self, event: OperationEvent) {
        self(event);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopOperationReporter;

impl OperationReporter for NoopOperationReporter {
    fn report(&mut self, _event: OperationEvent) {}
}

pub type OperationResult = Result<OperationReceipt, OperationError>;

#[allow(dead_code)]
pub(crate) fn report_operation(
    request: OperationRequest,
    reporter: &mut dyn OperationReporter,
    run: impl FnOnce(&mut dyn OperationReporter) -> OperationResult,
) -> OperationResult {
    reporter.report(OperationEvent::Started(request));
    match run(reporter) {
        Ok(receipt) => {
            reporter.report(OperationEvent::Completed(receipt.clone()));
            Ok(receipt)
        }
        Err(error) => {
            reporter.report(OperationEvent::Failed(error.clone()));
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::receipt::{OpKind, OpReceipt, OpStatus};
    use anyhow::{Context, anyhow};

    fn checkout_request() -> OperationRequest {
        OperationRequest::Checkout {
            branch: "feature".into(),
        }
    }

    fn checkout_receipt() -> OperationReceipt {
        let request = checkout_request();
        OperationReceipt {
            request: request.clone(),
            summary: "Checked out feature".into(),
            affected_branches: vec!["feature".into()],
            outcome: OperationOutcome::Checkout(CheckoutOutcome::CheckedOut {
                branch: "feature".into(),
            }),
            transaction: None,
            warnings: Vec::new(),
            side_effects: OperationSideEffects::RepositoryChanged,
        }
    }

    fn receipt_with_status_and_local_ref(
        status: OpStatus,
        before: Option<&str>,
        after: Option<&str>,
    ) -> OpReceipt {
        let mut receipt = OpReceipt::new(
            "transaction-id".into(),
            OpKind::Restack,
            "/tmp/repo".into(),
            "main".into(),
            "feature".into(),
        );
        receipt.add_local_ref("feature", before);
        if let Some(after) = after {
            receipt.update_local_ref_after("feature", after);
        }
        receipt.status = status;
        receipt
    }

    #[test]
    fn operation_error_separates_safe_display_from_diagnostics() {
        let source = Err::<(), _>(anyhow!("low-level failure"))
            .context("high-level context")
            .unwrap_err();

        let error = OperationError::from_source(
            checkout_request(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not check out the branch",
            "Resolve the Git error and retry",
            &source,
            None,
            OperationSideEffects::None,
        );

        assert_eq!(error.to_string(), "Could not check out the branch");
        assert_eq!(error.action, "Resolve the Git error and retry");
        assert!(error.diagnostic_chain.contains("high-level context"));
        assert!(error.diagnostic_chain.contains("low-level failure"));
        assert!(!error.to_string().contains("low-level failure"));
    }

    #[test]
    fn report_operation_emits_one_terminal_event_for_success() {
        let request = checkout_request();
        let receipt = checkout_receipt();
        let mut events = Vec::new();

        let result = report_operation(request.clone(), &mut |event| events.push(event), |_| {
            Ok(receipt.clone())
        })
        .unwrap();

        assert_eq!(result, receipt);
        assert_eq!(
            events,
            vec![
                OperationEvent::Started(request),
                OperationEvent::Completed(receipt),
            ]
        );
    }

    #[test]
    fn report_operation_emits_one_terminal_event_for_failure() {
        let request = checkout_request();
        let source = anyhow!("checkout failed");
        let error = OperationError::from_source(
            request.clone(),
            OperationErrorKind::LocalGit,
            OperationErrorDetails::None,
            "Could not check out the branch",
            "Retry after fixing Git",
            &source,
            None,
            OperationSideEffects::None,
        );
        let mut events = Vec::new();

        let result = report_operation(request.clone(), &mut |event| events.push(event), |_| {
            Err(error.clone())
        });

        assert_eq!(result.unwrap_err(), error);
        assert_eq!(
            events,
            vec![
                OperationEvent::Started(request),
                OperationEvent::Failed(error),
            ]
        );
    }

    #[test]
    fn transaction_summary_uses_canonical_can_undo_for_success() {
        let receipt =
            receipt_with_status_and_local_ref(OpStatus::Success, Some("before"), Some("after"));

        let summary = TransactionSummary::from(&receipt);

        assert_eq!(summary.status, TransactionStatus::Succeeded);
        assert_eq!(summary.can_undo, receipt.can_undo());
        assert!(summary.can_undo);
    }

    #[test]
    fn transaction_summary_maps_failure_without_changing_undo_semantics() {
        let receipt = receipt_with_status_and_local_ref(OpStatus::Failed, None, Some("after"));

        let summary = TransactionSummary::from(&receipt);

        assert_eq!(summary.status, TransactionStatus::Failed);
        assert_eq!(summary.can_undo, receipt.can_undo());
        assert!(!summary.can_undo);
    }

    #[test]
    fn transaction_summary_deduplicates_metadata_labels_and_detects_remote_changes() {
        let mut receipt =
            receipt_with_status_and_local_ref(OpStatus::InProgress, Some("before"), None);
        receipt.add_metadata_ref("feature", Some("metadata-before"));
        receipt.add_remote_ref("origin", "feature", Some("remote-before"));
        receipt.update_remote_ref_after("origin", "feature", "remote-after");

        let summary = TransactionSummary::from(&receipt);

        assert_eq!(summary.id, "transaction-id");
        assert_eq!(summary.kind, "restack");
        assert_eq!(summary.status, TransactionStatus::InProgress);
        assert_eq!(summary.branches, vec!["feature"]);
        assert!(summary.changed_remote_refs);
    }
}
