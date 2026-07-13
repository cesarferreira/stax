#![allow(clippy::result_large_err)]
#![deny(missing_docs)]

//! Presentation-neutral requests, events, outcomes, and errors for repository operations.

use std::path::PathBuf;

/// A typed request that an application client can execute against a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationRequest {
    /// Check out an existing branch.
    Checkout {
        /// Branch to check out.
        branch: String,
    },
    /// Create a branch with an explicit parent.
    CreateBranch {
        /// Name of the branch to create.
        name: String,
        /// Parent branch for the new branch.
        parent: String,
    },
    /// Rename the current local branch.
    RenameBranch {
        /// Current branch name.
        branch: String,
        /// New literal branch name.
        new_name: String,
    },
    /// Delete a local branch.
    DeleteBranch {
        /// Branch to delete.
        branch: String,
        /// Whether an unmerged branch may be deleted.
        force: bool,
    },
    /// Move a branch and its descendant subtree onto a new parent.
    MoveSubtree {
        /// Root of the subtree to move.
        source: String,
        /// New parent branch.
        new_parent: String,
        /// Whether dirty affected worktrees may be stashed automatically.
        auto_stash: bool,
    },
    /// Apply a previously previewed linear stack order.
    ReorderStack {
        /// Exact live order used to create the preview.
        original_order: Vec<String>,
        /// Proposed bottom-to-top order.
        proposed_order: Vec<String>,
        /// Whether dirty affected worktrees may be stashed automatically.
        auto_stash: bool,
    },
    /// Restore a persisted transaction's before-state.
    UndoTransaction {
        /// Specific operation id, or latest when absent.
        operation_id: Option<String>,
        /// Whether remote refs may also be updated.
        update_remote: bool,
    },
    /// Restore a persisted transaction's after-state.
    RedoTransaction {
        /// Specific operation id, or latest when absent.
        operation_id: Option<String>,
        /// Whether remote refs may also be updated.
        update_remote: bool,
    },
    /// Restack branches selected by a deterministic scope.
    Restack {
        /// Branch scope to restack.
        scope: RestackScope,
        /// Whether dirty affected worktrees may be stashed automatically.
        auto_stash: bool,
    },
    /// Push a stack and create or update its pull requests.
    SubmitStack {
        /// Initial mode for pull requests created by the submit.
        new_pull_requests: PullRequestMode,
    },
    /// Resolve the pull-request URL associated with a branch without mutating the repository.
    ResolvePullRequestUrl {
        /// Branch whose pull-request URL should be resolved.
        branch: String,
    },
}

impl OperationRequest {
    /// Returns whether this request can change repository or remote state.
    pub fn is_mutating(&self) -> bool {
        !matches!(self, Self::ResolvePullRequestUrl { .. })
    }
}

/// Selects the branches included in a restack request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestackScope {
    /// Restack only the named branch.
    Branch(String),
    /// Restack the complete stack containing the named branch.
    StackContaining(String),
    /// Restack from the stack root through the named branch.
    ThroughBranch(String),
    /// Restack every eligible branch in the repository.
    All,
}

/// Initial state for pull requests created during submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestMode {
    /// Create new pull requests as drafts.
    Draft,
    /// Create new pull requests ready for review.
    Ready,
}

/// A presentation-neutral stage in an operation lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStage {
    /// Validate the request and repository preconditions.
    Validating,
    /// Prepare resources and an execution plan.
    Preparing,
    /// Check out a branch.
    CheckingOut,
    /// Create a branch.
    CreatingBranch,
    /// Rename a local branch and update its stack metadata.
    RenamingBranch,
    /// Delete a local branch and its metadata.
    DeletingBranch,
    /// Prepare and apply a subtree move.
    MovingSubtree,
    /// Apply a linear stack reorder.
    ReorderingStack,
    /// Restore transaction before-state.
    UndoingTransaction,
    /// Restore transaction after-state.
    RedoingTransaction,
    /// Rebase branches onto their updated parents.
    Restacking,
    /// Push local refs to a remote.
    Pushing,
    /// Create or update pull requests.
    UpdatingPullRequests,
    /// Resolve a branch's pull-request URL.
    ResolvingPullRequest,
}

/// Structured progress suitable for any presentation layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationProgress {
    /// Current operation stage.
    pub stage: OperationStage,
    /// Number of completed work items in this stage.
    pub completed: usize,
    /// Total work items when known.
    pub total: Option<usize>,
    /// Branch currently being processed, when applicable.
    pub branch: Option<String>,
    /// Safe, user-facing progress message.
    pub message: String,
}

/// An event emitted during one operation execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationEvent {
    /// The operation started with the supplied request.
    Started(OperationRequest),
    /// The operation reported intermediate progress.
    Progress(OperationProgress),
    /// The operation completed successfully with a receipt.
    Completed(OperationReceipt),
    /// The operation terminated with a structured error.
    Failed(OperationError),
}

/// Classifies observable state changes made before an operation returned.
///
/// Clients should refresh repository state whenever [`Self::requires_refresh`] returns `true`,
/// including after failures, because an error can follow partial local or remote effects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationSideEffects {
    /// No repository or remote state changed; a refresh is unnecessary.
    None,
    /// Local repository state changed and clients must refresh it.
    RepositoryChanged,
    /// Remote state may have changed and clients must refresh local and remote-derived state.
    RemoteMayHaveChanged,
}

impl OperationSideEffects {
    /// Returns whether clients must refresh state after receiving this classification.
    pub fn requires_refresh(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Successful operation data retained for presentation and refresh handling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationReceipt {
    /// Request that produced this receipt.
    pub request: OperationRequest,
    /// Safe, user-facing summary of the completed operation.
    pub summary: String,
    /// Branches whose state was affected.
    pub affected_branches: Vec<String>,
    /// Operation-specific successful outcome.
    pub outcome: OperationOutcome,
    /// Transaction facts when the operation used a persisted transaction.
    pub transaction: Option<TransactionSummary>,
    /// Non-fatal conditions observed while completing the operation.
    pub warnings: Vec<OperationWarning>,
    /// Observable state changes that determine whether clients must refresh.
    pub side_effects: OperationSideEffects,
}

/// Operation-specific data returned after successful completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOutcome {
    /// Outcome of a checkout request.
    Checkout(CheckoutOutcome),
    /// A new branch was created.
    BranchCreated {
        /// Created branch name.
        branch: String,
        /// Parent assigned to the created branch.
        parent: String,
    },
    /// A local branch was renamed.
    BranchRenamed {
        /// Branch name before the rename.
        old_name: String,
        /// Branch name after the rename.
        new_name: String,
    },
    /// A local branch was deleted.
    BranchDeleted {
        /// Deleted branch name.
        branch: String,
        /// Descendants deliberately retained with their existing metadata.
        retained_descendants: Vec<String>,
    },
    /// A branch and its descendants were moved onto a new parent.
    SubtreeMoved {
        /// Root branch that moved.
        source: String,
        /// New parent of the root branch.
        new_parent: String,
        /// Root and descendants restacked by the operation.
        moved_branches: Vec<String>,
    },
    /// A linear stack order was applied.
    StackReordered {
        /// Order validated before mutation.
        original_order: Vec<String>,
        /// Order applied by the operation.
        applied_order: Vec<String>,
    },
    /// A transaction's before-state was restored.
    TransactionUndone {
        /// Restored operation id.
        operation_id: String,
        /// Local refs changed.
        changed_refs: Vec<String>,
    },
    /// A transaction's after-state was restored.
    TransactionRedone {
        /// Restored operation id.
        operation_id: String,
        /// Local refs changed.
        changed_refs: Vec<String>,
    },
    /// One or more branches were restacked.
    Restacked {
        /// Branches successfully restacked.
        branches: Vec<String>,
        /// Frozen branches intentionally skipped.
        skipped_frozen: Vec<String>,
    },
    /// A stack was submitted.
    Submitted {
        /// Pull requests created, updated, or observed during submit.
        pull_requests: Vec<PullRequestReceipt>,
    },
    /// A branch's pull-request URL was resolved.
    PullRequestResolved {
        /// Branch associated with the pull request.
        branch: String,
        /// Resolved pull-request URL.
        url: String,
    },
}

/// Result of requesting a branch checkout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckoutOutcome {
    /// The requested branch became current.
    CheckedOut {
        /// Branch that was checked out.
        branch: String,
    },
    /// The requested branch was already current.
    AlreadyCurrent {
        /// Branch that was already current.
        branch: String,
    },
}

/// Describes how submit affected one pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullRequestChange {
    /// The pull request was created.
    Created,
    /// The pull request already existed and was updated.
    Updated,
    /// The pull request already matched the requested state.
    Unchanged,
}

/// Pull-request facts returned by a successful submit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PullRequestReceipt {
    /// Branch represented by the pull request.
    pub branch: String,
    /// Provider-assigned pull-request number.
    pub number: u64,
    /// Canonical pull-request URL.
    pub url: String,
    /// Change made to the pull request.
    pub change: PullRequestChange,
}

/// Lifecycle state recorded by an operation transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionStatus {
    /// The persisted transaction has not reached a terminal state.
    InProgress,
    /// The persisted transaction completed successfully.
    Succeeded,
    /// The persisted transaction terminated with a failure.
    Failed,
}

/// Presentation-neutral facts projected from an internal operation receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionSummary {
    /// Stable operation identifier.
    pub id: String,
    /// Human-readable operation kind.
    pub kind: String,
    /// Persisted transaction status.
    pub status: TransactionStatus,
    /// Distinct branch names in stable first-seen order.
    pub branches: Vec<String>,
    /// Whether canonical receipt semantics permit undo.
    pub can_undo: bool,
    /// Whether canonical receipt semantics permit redo.
    pub can_redo: bool,
    /// Whether at least one remote ref was changed.
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
            can_redo: receipt.can_redo(),
            changed_remote_refs: receipt.changed_remote_refs(),
        }
    }
}

/// A non-fatal condition retained alongside a successful outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationWarning {
    /// A requested branch name was normalized before use.
    BranchNameNormalized {
        /// Original branch name supplied by the caller.
        original: String,
        /// Normalized branch name used by the operation.
        normalized: String,
    },
    /// Deleting a branch retained descendants that still reference it as an ancestor.
    DescendantsRetained {
        /// Deleted branch name.
        deleted_branch: String,
        /// Descendants left unchanged.
        descendants: Vec<String>,
    },
    /// A requested restack boundary was adjusted to preserve stack semantics.
    RestackBoundaryAdjusted {
        /// Branch whose requested boundary was adjusted.
        branch: String,
        /// Safe explanation of the adjustment.
        reason: String,
    },
    /// Work completed, but restoring an automatic stash failed.
    StashRestoreFailed {
        /// Worktree where stash restoration failed.
        worktree: PathBuf,
        /// Diagnostic details for explicit troubleshooting.
        diagnostic: String,
    },
    /// The selected provider cannot apply requested reviewers.
    SubmitReviewersUnsupported {
        /// Provider that lacks reviewer support.
        provider: String,
        /// Reviewers that could not be applied.
        reviewers: Vec<String>,
    },
    /// Native stack integration was unavailable or declined.
    SubmitNativeStackAdvisory {
        /// Structured reason native stack integration was not used.
        reason: NativeStackAdvisory,
        /// Safe, user-facing advisory message.
        message: String,
    },
}

/// Reason native stack integration could not be used during submit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeStackAdvisory {
    /// The GitHub CLI executable was unavailable.
    GhUnavailable,
    /// The required native stack extension was not installed.
    ExtensionMissing,
    /// The installed native stack extension was too old.
    ExtensionOutdated,
    /// The stack spans a fork unsupported by native integration.
    ForkedStack,
    /// Available authentication cannot support native integration.
    AuthenticationUnsupported,
    /// Native integration is disabled by configuration.
    FeatureDisabled,
    /// The native integration rejected a requested link.
    LinkRejected,
}

/// Stable category for an operation failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationErrorKind {
    /// The requested repository could not be opened or accessed.
    RepositoryUnavailable,
    /// Stax repository initialization is required before continuing.
    InitializationRequired,
    /// Authentication credentials are missing, invalid, or expired.
    Authentication,
    /// Authenticated credentials lack permission for the requested action.
    Authorization,
    /// Worktree changes violate a clean-worktree precondition.
    DirtyWorktree,
    /// Another operation-specific precondition was not met.
    PreconditionFailed,
    /// A rebase is already active in an affected worktree.
    RebaseInProgress,
    /// A rebase stopped because of conflicts.
    RebaseConflict,
    /// A local Git operation failed.
    LocalGit,
    /// A network request failed before a known partial remote update.
    Network,
    /// A remote operation failed after some remote state may have changed.
    PartialRemoteUpdate,
    /// The provider or environment lacks a required capability.
    UnsupportedCapability,
    /// Another mutation currently holds the repository operation lease.
    Busy,
    /// Caller-supplied input is invalid.
    InvalidInput,
    /// The operation cannot safely run in the current asynchronous runtime.
    Runtime,
    /// An unexpected internal invariant or implementation failure occurred.
    Internal,
}

/// Structured context that presentation layers may use for targeted recovery UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationErrorDetails {
    /// No additional structured context is available.
    None,
    /// The failure concerns one branch.
    Branch {
        /// Branch associated with the failure.
        branch: String,
    },
    /// The failure concerns the pull request for one branch.
    PullRequest {
        /// Branch associated with the pull request.
        branch: String,
    },
    /// The branch is checked out in another worktree.
    AlreadyCheckedOutElsewhere {
        /// Branch that cannot be checked out here.
        branch: String,
        /// Canonical path of the worktree where the branch is checked out.
        path: PathBuf,
    },
    /// A rebase state or conflict exists in an affected worktree.
    Rebase {
        /// Branch being rebased when known.
        branch: Option<String>,
        /// Canonical path of the affected worktree.
        worktree: PathBuf,
    },
}

/// Presentation-safe operation failure plus explicitly retained diagnostics.
///
/// [`std::fmt::Display`] returns only [`Self::primary`]. Detailed diagnostics are retained in
/// [`Self::diagnostic_chain`] and must never be displayed automatically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationError {
    /// Request that failed.
    pub request: OperationRequest,
    /// Stable failure category.
    pub kind: OperationErrorKind,
    /// Structured context for targeted recovery behavior.
    pub details: OperationErrorDetails,
    /// Safe primary message returned by [`std::fmt::Display`].
    pub primary: String,
    /// Safe user-facing recovery action.
    pub action: String,
    /// Full underlying error chain for explicit diagnostic copying.
    ///
    /// This value may contain sensitive repository, filesystem, provider, or network details.
    /// Presentation layers must expose it only through an explicit diagnostics action; it must
    /// not be shown automatically and is deliberately excluded from [`std::fmt::Display`].
    pub diagnostic_chain: String,
    /// Receipt retained when work completed or partially completed before the failure surfaced.
    pub receipt: Option<OperationReceipt>,
    /// Observable effects that determine whether clients must refresh after the failure.
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

/// Receives structured lifecycle events from an operation.
pub trait OperationReporter {
    /// Records one operation lifecycle event.
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

/// Reporter that intentionally discards every operation event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopOperationReporter;

impl OperationReporter for NoopOperationReporter {
    fn report(&mut self, _event: OperationEvent) {}
}

/// Result returned by a presentation-neutral operation.
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
    use super::{
        CheckoutOutcome, OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent,
        OperationOutcome, OperationReceipt, OperationRequest, OperationSideEffects,
        TransactionStatus, TransactionSummary, report_operation,
    };
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
        assert_eq!(summary.can_redo, receipt.can_redo());
        assert!(summary.can_undo);
        assert!(summary.can_redo);
    }

    #[test]
    fn transaction_summary_maps_failure_without_changing_undo_semantics() {
        let receipt = receipt_with_status_and_local_ref(OpStatus::Failed, None, Some("after"));

        let summary = TransactionSummary::from(&receipt);

        assert_eq!(summary.status, TransactionStatus::Failed);
        assert_eq!(summary.can_undo, receipt.can_undo());
        assert_eq!(summary.can_redo, receipt.can_redo());
        assert!(summary.can_undo);
        assert!(!summary.can_redo);
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
