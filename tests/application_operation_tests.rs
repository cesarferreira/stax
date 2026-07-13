use crate::common::{OutputAssertions, TestRepo};
use stax::application::{
    CheckoutOutcome, NoopOperationReporter, OperationErrorDetails, OperationErrorKind,
    OperationEvent, OperationOutcome, OperationRequest, OperationSideEffects, PullRequestMode,
    RepositorySession, RestackScope, TransactionStatus, execute_repository_operation,
};
use std::path::{Path, PathBuf};

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

#[test]
fn create_empty_branch_uses_explicit_parent_without_creating_a_commit() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let feature = repo.create_stack(&["feature"]).remove(0);
    let parent_oid = repo.get_commit_sha(&feature);
    repo.git(&["checkout", "main"]).assert_success();
    let receipt = RepositorySession::open(repo.path())
        .unwrap()
        .create_empty_branch("child", &feature, &mut NoopOperationReporter)
        .unwrap();
    assert_eq!(repo.get_commit_sha("child"), parent_oid);
    assert_eq!(repo.get_current_parent().as_deref(), Some(feature.as_str()));
    assert_eq!(repo.current_branch(), "child");
    assert_eq!(
        receipt.side_effects,
        OperationSideEffects::RepositoryChanged
    );
}

#[test]
fn create_rejects_rebase_in_progress_before_creating_a_ref() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    std::fs::create_dir_all(repo.path().join(".git/rebase-merge")).unwrap();
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .create_empty_branch("child", "main", &mut NoopOperationReporter)
        .unwrap_err();
    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert!(!repo.list_branches().contains(&"child".to_string()));
}

#[test]
fn rename_updates_ref_metadata_children_and_returns_undoable_receipt() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let branches = repo.create_stack(&["parent", "child"]);
    repo.git(&["checkout", &branches[0]]).assert_success();

    let receipt = RepositorySession::open(repo.path())
        .unwrap()
        .rename_branch(&branches[0], "renamed", &mut NoopOperationReporter)
        .unwrap();

    assert_eq!(repo.current_branch(), "renamed");
    assert!(!repo.list_branches().contains(&branches[0]));
    assert!(repo.get_children("renamed").contains(&branches[1]));
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::BranchRenamed {
            ref old_name,
            ref new_name,
        } if old_name == &branches[0] && new_name == "renamed"
    ));
    assert!(receipt.transaction.as_ref().is_some_and(|tx| {
        tx.status == TransactionStatus::Succeeded && tx.can_undo && tx.can_redo
    }));
}

#[test]
fn rename_rejects_the_trunk_branch_without_changing_refs() {
    let repo = TestRepo::new();
    repo.set_trunk("main");

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .rename_branch("main", "renamed", &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::PreconditionFailed);
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert_eq!(repo.current_branch(), "main");
    assert_eq!(receipt_count(&repo), 0);
}

#[test]
fn rename_rejects_a_non_current_branch_without_changing_refs() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let branches = repo.create_stack(&["parent", "child"]);

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .rename_branch(&branches[0], "renamed", &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::PreconditionFailed);
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert!(repo.list_branches().contains(&branches[0]));
    assert!(!repo.list_branches().contains(&"renamed".to_string()));
    assert_eq!(receipt_count(&repo), 0);
}

#[test]
fn rename_rejects_invalid_and_colliding_names_before_writing_a_receipt() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let branches = repo.create_stack(&["parent", "child"]);

    for new_name in ["///", branches[0].as_str()] {
        let error = RepositorySession::open(repo.path())
            .unwrap()
            .rename_branch(&branches[1], new_name, &mut NoopOperationReporter)
            .unwrap_err();
        assert_eq!(error.kind, OperationErrorKind::InvalidInput);
        assert_eq!(error.side_effects, OperationSideEffects::None);
    }

    assert_eq!(repo.current_branch(), branches[1]);
    assert_eq!(receipt_count(&repo), 0);
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

#[test]
fn repository_open_error_emits_started_then_exactly_one_failed_event() {
    let request = OperationRequest::Checkout {
        branch: "feature".into(),
    };
    let mut events = Vec::new();

    let error = execute_repository_operation(
        "/definitely/missing/stax-repository",
        request.clone(),
        &mut |event| events.push(event),
    )
    .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RepositoryUnavailable);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0], OperationEvent::Started(request));
    assert!(matches!(events[1], OperationEvent::Failed(_)));
}

#[test]
fn uninitialized_repository_maps_to_initialization_required() {
    let repository = tempfile::tempdir().unwrap();
    crate::common::init_test_repo(repository.path()).unwrap();

    let error = execute_repository_operation(
        repository.path(),
        OperationRequest::Restack {
            scope: RestackScope::All,
            auto_stash: false,
        },
        &mut NoopOperationReporter,
    )
    .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::InitializationRequired);
    assert!(!error.primary.is_empty());
    assert!(!error.action.is_empty());
    assert!(!error.diagnostic_chain.is_empty());
    assert_eq!(error.side_effects, OperationSideEffects::None);
}

fn output_text(output: std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repository_git_dir(repo: &TestRepo) -> PathBuf {
    let path = PathBuf::from(output_text(repo.git(&["rev-parse", "--git-common-dir"])));
    if path.is_absolute() {
        path
    } else {
        repo.path().join(path)
    }
}

fn linked_git_dir(repo: &TestRepo, linked: &Path) -> PathBuf {
    let path = PathBuf::from(output_text(
        repo.git_in(linked, &["rev-parse", "--git-dir"]),
    ));
    if path.is_absolute() {
        path
    } else {
        linked.join(path)
    }
}

fn receipt_count(repo: &TestRepo) -> usize {
    let ops_dir = repository_git_dir(repo).join("stax").join("ops");
    std::fs::read_dir(ops_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("json"))
        .count()
}

fn stash_count(repo: &TestRepo, cwd: &Path) -> usize {
    TestRepo::stdout(&repo.git_in(cwd, &["stash", "list"]))
        .lines()
        .count()
}

fn linked_restack_target(repo: &TestRepo, name: &str) -> (String, tempfile::TempDir, PathBuf) {
    let branch = repo.create_stack(&[name]).remove(0);
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked");
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &branch])
        .assert_success();
    repo.create_file(&format!("{name}-main.txt"), "main moved\n");
    repo.commit(&format!("Advance main for {name}"));
    (branch, linked_parent, linked)
}

fn linked_submit_target(repo: &TestRepo, name: &str) -> (String, tempfile::TempDir, PathBuf) {
    let branch = repo.create_stack(&[name]).remove(0);
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("submit-linked");
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &branch])
        .assert_success();
    (branch, linked_parent, linked)
}

#[test]
fn restack_active_rebase_in_linked_target_reports_canonical_path() {
    let repo = TestRepo::new();
    let (branch, _linked_parent, linked) = linked_restack_target(&repo, "app-linked-active-rebase");
    std::fs::create_dir_all(linked_git_dir(&repo, &linked).join("rebase-merge")).unwrap();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::Branch(branch.clone()),
            true,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(
        error.details,
        OperationErrorDetails::Rebase {
            branch: Some(branch),
            worktree: linked.canonicalize().unwrap(),
        }
    );
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert!(error.receipt.is_none());
}

#[test]
fn restack_linked_rebase_preflight_runs_before_stash_and_transaction() {
    let repo = TestRepo::new();
    let (branch, _linked_parent, linked) =
        linked_restack_target(&repo, "app-linked-preflight-order");
    let branch_before = repo.get_commit_sha(&branch);
    let receipts_before = receipt_count(&repo);
    std::fs::write(linked.join("dirty.txt"), "dirty\n").unwrap();
    std::fs::create_dir_all(linked_git_dir(&repo, &linked).join("rebase-merge")).unwrap();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::Branch(branch.clone()),
            true,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(
        error.details,
        OperationErrorDetails::Rebase {
            branch: Some(branch.clone()),
            worktree: linked.canonicalize().unwrap(),
        }
    );
    assert_eq!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(stash_count(&repo, &linked), 0);
    assert_eq!(receipt_count(&repo), receipts_before);
    assert!(linked.join("dirty.txt").exists());
}

#[test]
fn restack_dirty_linked_target_requires_auto_stash() {
    let repo = TestRepo::new();
    let (branch, _linked_parent, linked) =
        linked_restack_target(&repo, "app-linked-dirty-required");
    let branch_before = repo.get_commit_sha(&branch);
    let receipts_before = receipt_count(&repo);
    std::fs::write(linked.join("dirty.txt"), "dirty\n").unwrap();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::Branch(branch.clone()),
            false,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::DirtyWorktree);
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert!(error.receipt.is_none());
    assert_eq!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(receipt_count(&repo), receipts_before);
    assert_eq!(
        std::fs::read_to_string(linked.join("dirty.txt")).unwrap(),
        "dirty\n"
    );
}

#[test]
fn restack_auto_stash_restores_exact_linked_worktree() {
    let repo = TestRepo::new();
    let (branch, _linked_parent, linked) = linked_restack_target(&repo, "app-linked-auto-stash");
    let branch_before = repo.get_commit_sha(&branch);
    std::fs::write(linked.join("dirty.txt"), "dirty\n").unwrap();

    let receipt = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::Branch(branch.clone()),
            true,
            &mut NoopOperationReporter,
        )
        .unwrap();

    assert_ne!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(
        std::fs::read_to_string(linked.join("dirty.txt")).unwrap(),
        "dirty\n"
    );
    assert!(
        TestRepo::stdout(&repo.git_in(&linked, &["status", "--porcelain", "--", "dirty.txt"]))
            .contains("dirty.txt")
    );
    assert_eq!(stash_count(&repo, &linked), 0);
    assert_eq!(
        receipt.side_effects,
        OperationSideEffects::RepositoryChanged
    );
}

#[tokio::test]
async fn submit_inside_tokio_returns_runtime_without_remote_change() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");
    let branch = repo.create_stack(&["app-submit-runtime"]).remove(0);
    let remote_branches_before = repo.list_remote_branches();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .submit_stack(PullRequestMode::Draft, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::Runtime);
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert!(error.receipt.is_none());
    assert_eq!(repo.list_remote_branches(), remote_branches_before);
    assert_eq!(repo.get_commit_sha(&branch), repo.get_commit_sha(&branch));
}

#[test]
fn submit_active_rebase_in_linked_branch_reports_canonical_path() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");
    let (branch, _linked_parent, linked) = linked_submit_target(&repo, "app-submit-linked-rebase");
    std::fs::create_dir_all(linked_git_dir(&repo, &linked).join("rebase-merge")).unwrap();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .submit_stack(PullRequestMode::Draft, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(
        error.details,
        OperationErrorDetails::Rebase {
            branch: Some(branch),
            worktree: linked.canonicalize().unwrap(),
        }
    );
    assert_eq!(error.side_effects, OperationSideEffects::None);
    assert!(error.receipt.is_none());
}

#[test]
fn submit_linked_rebase_aborts_before_fetch_discovery_or_temp_refs() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");
    let (branch, _linked_parent, linked) =
        linked_submit_target(&repo, "app-submit-linked-rebase-order");
    let branch_before = repo.get_commit_sha(&branch);
    let remote_branches_before = repo.list_remote_branches();
    let receipts_before = receipt_count(&repo);
    std::fs::create_dir_all(linked_git_dir(&repo, &linked).join("rebase-merge")).unwrap();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .submit_stack(PullRequestMode::Draft, &mut NoopOperationReporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(repo.list_remote_branches(), remote_branches_before);
    assert_eq!(receipt_count(&repo), receipts_before);
    assert!(TestRepo::stdout(&repo.git(&["for-each-ref", "refs/stax/submit"])).is_empty());
}

#[cfg(unix)]
#[test]
fn restack_success_receipt_persistence_failure_returns_in_memory_receipt() {
    use std::os::unix::fs::PermissionsExt;

    let repo = TestRepo::new();
    let branch = repo.create_stack(&["app-success-receipt-save"]).remove(0);
    let branch_before = repo.get_commit_sha(&branch);
    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-success-receipt-save-main.txt", "main moved\n");
    repo.commit("Advance main before receipt persistence failure");
    let ops_dir = repository_git_dir(&repo).join("stax").join("ops");
    std::fs::create_dir_all(&ops_dir).unwrap();
    let hook = repository_git_dir(&repo).join("hooks").join("post-rewrite");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"rebase\" ]; then for receipt in '{}'/*.json; do rm -f \"$receipt\"; mkdir \"$receipt\"; done; fi\n",
            ops_dir.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&hook, permissions).unwrap();
    repo.git(&["checkout", &branch]).assert_success();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::Branch(branch.clone()),
            false,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_ne!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(error.kind, OperationErrorKind::LocalGit);
    assert_eq!(error.side_effects, OperationSideEffects::RepositoryChanged);
    let receipt = error.receipt.expect("successful in-memory receipt");
    let transaction = receipt.transaction.expect("transaction summary");
    assert_eq!(transaction.status, TransactionStatus::Succeeded);
    assert_eq!(
        receipt.side_effects,
        OperationSideEffects::RepositoryChanged
    );
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. } if branches == &vec![branch]
    ));
}
