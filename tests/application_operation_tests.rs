use stax::application::{
    OperationErrorKind, OperationRequest, OperationSideEffects, PullRequestMode,
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
