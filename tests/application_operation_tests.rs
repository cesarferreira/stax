use crate::common::{OutputAssertions, TestRepo};
use stax::application::{
    CheckoutOutcome, NoopOperationReporter, OperationErrorDetails, OperationErrorKind,
    OperationOutcome, OperationRequest, OperationSideEffects, PullRequestMode, RepositorySession,
};

#[test]
fn operation_requests_classify_mutations_and_refresh_effects() {
    assert!(
        OperationRequest::Checkout {
            branch: "feature".into()
        }
        .is_mutating()
    );
    assert!(
        OperationRequest::SubmitStack {
            new_pull_requests: PullRequestMode::Draft,
        }
        .is_mutating()
    );
    assert!(
        !OperationRequest::ResolvePullRequestUrl {
            branch: "feature".into(),
        }
        .is_mutating()
    );
    assert!(!OperationSideEffects::None.requires_refresh());
    assert!(OperationSideEffects::RepositoryChanged.requires_refresh());
    assert!(OperationSideEffects::RemoteMayHaveChanged.requires_refresh());
}

#[test]
fn error_categories_are_copyable_and_cover_runtime_and_security_boundaries() {
    fn copy_kind(kind: OperationErrorKind) -> OperationErrorKind {
        kind
    }

    assert_eq!(
        copy_kind(OperationErrorKind::Authentication),
        OperationErrorKind::Authentication
    );
    assert_eq!(
        copy_kind(OperationErrorKind::Authorization),
        OperationErrorKind::Authorization
    );
    assert_eq!(
        copy_kind(OperationErrorKind::Runtime),
        OperationErrorKind::Runtime
    );
}

#[test]
fn checkout_changes_only_the_explicit_repository() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    let unrelated = TestRepo::new();
    let session = RepositorySession::open(repo.path()).unwrap();
    let receipt = session
        .checkout("feature", &mut NoopOperationReporter)
        .unwrap();

    assert_eq!(repo.current_branch(), feature);
    assert_eq!(unrelated.current_branch(), "main");
    assert_eq!(
        receipt.side_effects,
        OperationSideEffects::RepositoryChanged
    );
    assert_eq!(
        receipt.outcome,
        OperationOutcome::Checkout(CheckoutOutcome::CheckedOut { branch: feature })
    );
}

#[test]
fn checkout_returns_already_current_without_refresh() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let session = RepositorySession::open(repo.path()).unwrap();
    let receipt = session
        .checkout("main", &mut NoopOperationReporter)
        .unwrap();

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(receipt.side_effects, OperationSideEffects::None);
    assert_eq!(
        receipt.outcome,
        OperationOutcome::Checkout(CheckoutOutcome::AlreadyCurrent {
            branch: "main".into()
        })
    );
}

#[test]
fn checkout_rejects_empty_branch_before_changing_head() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let before = repo.current_branch();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout(" ", &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::InvalidInput);
    assert_eq!(repo.current_branch(), before);
}

#[test]
fn checkout_rejects_missing_local_branch_before_changing_head() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let before = repo.current_branch();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout("missing", &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::InvalidInput);
    assert_eq!(repo.current_branch(), before);
}

#[test]
fn checkout_reports_a_linked_worktree_without_changing_cwd() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked");
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &feature])
        .assert_success();
    let cwd = std::env::current_dir().unwrap();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout(&feature, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::PreconditionFailed);
    assert_eq!(
        error.details,
        OperationErrorDetails::AlreadyCheckedOutElsewhere {
            branch: feature,
            path: linked.canonicalize().unwrap(),
        }
    );
    assert_eq!(std::env::current_dir().unwrap(), cwd);
}

#[test]
fn checkout_rejects_an_existing_rebase_before_changing_head() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let before = repo.current_branch();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .checkout(&feature, &mut NoopOperationReporter)
        .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(repo.current_branch(), before);
}

#[tokio::test]
async fn pull_request_network_fallback_returns_runtime_error_inside_tokio() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .resolve_pull_request_url(&feature, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::Runtime);
    assert_eq!(error.side_effects, OperationSideEffects::None);
}
