use super::{
    AppServices, AppView, PickerFuture, RecentRepositoryStore, RepositoryPicker, SnapshotLoader,
};
use crate::hydration::{BranchHydrationService, HydrationFuture};
use crate::operation::{
    BrowserService, FakeOperationService, NativeOperationService, OperationService,
    RecordingBrowserService,
};
use crate::state::WorkspaceState;
use gpui::{App, TestAppContext, VisualTestContext};
use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CheckoutOutcome, CiSummary, DiffLine, DiffLineKind,
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationReceipt, OperationRequest, OperationResult, OperationSideEffects, PullRequestChange,
    PullRequestMode, PullRequestReceipt, RepositorySnapshot, RestackScope, TransactionStatus,
    TransactionSummary,
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

struct TestSnapshotLoader {
    snapshots: Mutex<Vec<RepositorySnapshot>>,
}

impl TestSnapshotLoader {
    fn new(initial: RepositorySnapshot) -> Self {
        Self {
            snapshots: Mutex::new(vec![initial]),
        }
    }

    fn push_snapshot(&self, snapshot: RepositorySnapshot) {
        self.snapshots.lock().unwrap().push(snapshot);
    }
}

impl SnapshotLoader for TestSnapshotLoader {
    fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
        Ok(self.snapshots.lock().unwrap().last().unwrap().clone())
    }
}

struct NoopPicker;

impl RepositoryPicker for NoopPicker {
    fn pick(&self, _cx: &mut App) -> PickerFuture {
        Box::pin(async { Ok(None) })
    }
}

#[derive(Default)]
struct NoopRecents;

impl RecentRepositoryStore for NoopRecents {
    fn load(&self) -> Result<Vec<PathBuf>, String> {
        Ok(Vec::new())
    }

    fn record(&self, _path: &Path) -> Result<(), String> {
        Ok(())
    }
}

struct NoopHydration;

impl BranchHydrationService for NoopHydration {
    fn load_details(
        &self,
        _repository: PathBuf,
        branch: BranchSummary,
    ) -> HydrationFuture<BranchDetails> {
        Box::pin(async move { Ok(details(&branch.name)) })
    }

    fn load_cached_diff(
        &self,
        _repository: PathBuf,
        _branch: String,
        _parent: String,
    ) -> HydrationFuture<Option<BranchDiff>> {
        Box::pin(async { Ok(None) })
    }

    fn load_diff(
        &self,
        _repository: PathBuf,
        branch: String,
        _parent: String,
    ) -> HydrationFuture<BranchDiff> {
        Box::pin(async move {
            Ok(BranchDiff {
                stat: Vec::new(),
                lines: vec![DiffLine {
                    kind: DiffLineKind::Context,
                    content: format!("{branch} diff"),
                }],
            })
        })
    }

    fn load_ci(&self, _repository: PathBuf, _branch: String) -> HydrationFuture<CiSummary> {
        Box::pin(async {
            Ok(CiSummary {
                overall_status: Some("success".into()),
                total: 1,
                passed: 1,
                failed: 0,
                running: 0,
                queued: 0,
                skipped: 0,
                started_at: None,
                completed_at: None,
                average_secs: None,
            })
        })
    }
}

fn loaded_workspace_state(root: &str) -> WorkspaceState {
    WorkspaceState::new(snapshot(root, Some("child")))
}

fn checkout_request() -> OperationRequest {
    OperationRequest::Checkout {
        branch: "parent".into(),
    }
}

fn restack_request() -> OperationRequest {
    OperationRequest::Restack {
        scope: RestackScope::StackContaining("child".into()),
        auto_stash: false,
    }
}

fn submit_request() -> OperationRequest {
    OperationRequest::SubmitStack {
        new_pull_requests: PullRequestMode::Draft,
    }
}

fn checkout_receipt() -> OperationReceipt {
    OperationReceipt {
        request: checkout_request(),
        summary: "Checked out parent".into(),
        affected_branches: vec!["parent".into()],
        outcome: OperationOutcome::Checkout(CheckoutOutcome::CheckedOut {
            branch: "parent".into(),
        }),
        transaction: None,
        warnings: Vec::new(),
        side_effects: OperationSideEffects::RepositoryChanged,
    }
}

fn submit_receipt() -> OperationReceipt {
    OperationReceipt {
        request: submit_request(),
        summary: "Submitted stack".into(),
        affected_branches: vec!["parent".into(), "child".into()],
        outcome: OperationOutcome::Submitted {
            pull_requests: vec![PullRequestReceipt {
                branch: "child".into(),
                number: 42,
                url: "https://github.com/acme/repo/pull/42".into(),
                change: PullRequestChange::Updated,
            }],
        },
        transaction: Some(transaction(TransactionStatus::Succeeded)),
        warnings: Vec::new(),
        side_effects: OperationSideEffects::RemoteMayHaveChanged,
    }
}

fn conflict_error_with_failed_receipt(side_effects: OperationSideEffects) -> OperationError {
    let mut receipt = checkout_receipt();
    receipt.request = restack_request();
    receipt.summary = "Restack stopped with conflicts".into();
    receipt.transaction = Some(transaction(TransactionStatus::Failed));
    receipt.side_effects = side_effects;
    OperationError {
        request: restack_request(),
        kind: OperationErrorKind::RebaseConflict,
        details: OperationErrorDetails::Rebase {
            branch: Some("child".into()),
            worktree: PathBuf::from("/repo"),
        },
        primary: "Restack stopped because child has conflicts".into(),
        action: "Resolve conflicts and run `st continue`.".into(),
        diagnostic_chain: "conflict while rebasing child".into(),
        receipt: Some(receipt),
        side_effects,
    }
}

fn dirty_error(side_effects: OperationSideEffects) -> OperationError {
    OperationError {
        request: restack_request(),
        kind: OperationErrorKind::DirtyWorktree,
        details: OperationErrorDetails::None,
        primary: "Affected worktrees contain uncommitted changes".into(),
        action: "Commit, stash, or discard changes and retry.".into(),
        diagnostic_chain: "dirty worktree precondition failed".into(),
        receipt: None,
        side_effects,
    }
}

fn partial_submit_error(side_effects: OperationSideEffects) -> OperationError {
    let mut receipt = submit_receipt();
    receipt.summary = "Submit partially updated remote state".into();
    receipt.side_effects = side_effects;
    OperationError {
        request: submit_request(),
        kind: OperationErrorKind::PartialRemoteUpdate,
        details: OperationErrorDetails::PullRequest {
            branch: "child".into(),
        },
        primary: "Submit failed after updating remote state".into(),
        action: "Refresh the repository, inspect the remote, and retry.".into(),
        diagnostic_chain: "remote update failed after push".into(),
        receipt: Some(receipt),
        side_effects,
    }
}

fn details(label: &str) -> BranchDetails {
    BranchDetails {
        ahead: label.len(),
        behind: 0,
        has_remote: true,
        unpushed: 0,
        unpulled: 0,
        commits: vec![format!("{label} commit")],
    }
}

fn snapshot_after_partial_submit() -> RepositorySnapshot {
    snapshot("/repo", Some("child-updated"))
}

fn run_service_to_completion(
    service: &dyn OperationService,
    repository_root: PathBuf,
    request: OperationRequest,
    cx: &mut TestAppContext,
) -> (Vec<OperationEvent>, OperationResult) {
    let (sender, receiver) = async_channel::bounded(32);
    let future = service.execute(repository_root, request, sender);
    let result = cx.background_executor.block(future);
    let events = cx.background_executor.block(async move {
        let mut events = Vec::new();
        while let Ok(event) = receiver.recv().await {
            events.push(event);
        }
        events
    });
    (events, result)
}

fn open_loaded_app(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    let service = Arc::new(FakeOperationService::default());
    let loader = Arc::new(TestSnapshotLoader::new(snapshot("/repo", Some("child"))));
    open_test_app(cx, loader, service)
}

fn open_loaded_app_with_refresh_snapshot(
    cx: &mut TestAppContext,
    refreshed: RepositorySnapshot,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    let service = Arc::new(FakeOperationService::default());
    let loader = Arc::new(TestSnapshotLoader::new(snapshot("/repo", Some("child"))));
    let (app, cx, service) = open_test_app(cx, Arc::clone(&loader), service);
    loader.push_snapshot(refreshed);
    (app, cx, service)
}

fn open_loaded_app_with_browser(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
    Arc<RecordingBrowserService>,
) {
    let service = Arc::new(FakeOperationService::default());
    let browser = Arc::new(RecordingBrowserService::default());
    let loader = Arc::new(TestSnapshotLoader::new(snapshot("/repo", Some("child"))));
    let services = AppServices::with_operation_services(
        loader,
        Rc::new(NoopPicker),
        Arc::new(NoopRecents),
        Arc::new(NoopHydration),
        service.clone(),
        browser.clone(),
    );
    let (app, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    (app, cx, service, browser)
}

fn open_test_app(
    cx: &mut TestAppContext,
    loader: Arc<TestSnapshotLoader>,
    service: Arc<FakeOperationService>,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    let services = AppServices::with_operation_services(
        loader,
        Rc::new(NoopPicker),
        Arc::new(NoopRecents),
        Arc::new(NoopHydration),
        service.clone(),
        Arc::new(RecordingBrowserService::default()),
    );
    let (app, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    (app, cx, service)
}

fn transaction(status: TransactionStatus) -> TransactionSummary {
    TransactionSummary {
        id: "op-1".into(),
        kind: "restack".into(),
        status,
        branches: vec!["child".into()],
        can_undo: status == TransactionStatus::Succeeded,
        changed_remote_refs: false,
    }
}

fn snapshot(root: &str, child_name: Option<&str>) -> RepositorySnapshot {
    let child = child_name.unwrap_or("child");
    RepositorySnapshot {
        repository_root: PathBuf::from(root),
        current_branch: child.into(),
        trunk: "main".into(),
        branches: vec![
            branch("main", None, false, true),
            branch("parent", Some("main"), false, false),
            branch(child, Some("parent"), true, false),
        ],
    }
}

fn branch(name: &str, parent: Option<&str>, current: bool, trunk: bool) -> BranchSummary {
    BranchSummary {
        name: name.into(),
        parent: parent.map(str::to_string),
        column: usize::from(!trunk),
        is_current: current,
        is_trunk: trunk,
        needs_restack: name == "child",
        pr_number: (name == "child").then_some(42),
        pr_state: (name == "child").then(|| "open".into()),
        ci_state: None,
    }
}

#[test]
fn successful_mutation_invalidates_before_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(checkout_request()).unwrap();
    let generation = state.generation();

    let effect = state
        .finish_operation(&token, Ok(checkout_receipt()))
        .unwrap();

    assert!(effect.refresh_snapshot);
    assert!(state.generation() > generation);
}

#[test]
fn side_effecting_failure_refreshes_and_preserves_error_and_receipt() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(restack_request()).unwrap();
    let error = conflict_error_with_failed_receipt(OperationSideEffects::RepositoryChanged);
    let generation = state.generation();

    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();

    assert!(effect.refresh_snapshot);
    assert!(state.generation() > generation);
    assert_eq!(state.operation_error(), Some(&error));
    assert_eq!(state.last_receipt(), error.receipt.as_ref());
}

#[test]
fn precondition_failure_does_not_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let token = state.begin_operation(restack_request()).unwrap();
    let error = dirty_error(OperationSideEffects::None);
    let generation = state.generation();

    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();

    assert!(!effect.refresh_snapshot);
    assert_eq!(state.generation(), generation);
    assert_eq!(state.operation_error(), Some(&error));
}

#[test]
fn hydration_started_before_partial_failure_is_rejected_after_refresh() {
    let mut state = loaded_workspace_state("/repo");
    let old_details = state.begin_details_load("child").unwrap();
    let token = state.begin_operation(submit_request()).unwrap();
    let error = partial_submit_error(OperationSideEffects::RemoteMayHaveChanged);

    let effect = state.finish_operation(&token, Err(error.clone())).unwrap();

    assert!(effect.refresh_snapshot);
    assert!(!state.apply_details(old_details, Ok(details("stale"))));
    state.replace_snapshot(snapshot_after_partial_submit());
    assert_eq!(state.operation_error(), Some(&error));
    assert_eq!(state.last_receipt(), error.receipt.as_ref());
}

#[gpui::test]
fn native_service_open_failure_has_one_started_and_one_failed_terminal(cx: &mut TestAppContext) {
    let (events, result) = run_service_to_completion(
        &NativeOperationService,
        PathBuf::from("/missing/repository"),
        checkout_request(),
        cx,
    );

    assert!(result.is_err());
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], OperationEvent::Started(_)));
    assert!(matches!(events[1], OperationEvent::Failed(_)));
}

#[gpui::test]
fn coordinator_drains_more_than_channel_capacity_in_order(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);
    service.script_progress_count(40);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).operation_progress_log()),
        (1..=40).collect::<Vec<_>>()
    );
}

#[gpui::test]
fn retained_result_is_the_only_completion_state_transition(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).completion_transition_count()),
        1
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        1
    );
}

#[gpui::test]
fn terminal_event_plus_retained_result_finishes_exactly_once(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(submit_request(), window, cx);
    });
    service.complete_next_success(submit_receipt());
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).completion_transition_count()),
        1
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        1
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).last_receipt()),
        Some(submit_receipt())
    );
}

#[gpui::test]
fn coordinator_ignores_completion_after_opening_another_repository(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
        app.open_repository(PathBuf::from("/other"), window, cx);
    });
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).completion_transition_count()),
        0
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        0
    );
}

#[gpui::test]
fn precondition_error_keeps_snapshot_generation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);
    let generation = cx.update(|_, gpui| app.read(gpui).workspace().unwrap().state().generation());

    app.update_in(cx, |app, window, cx| {
        app.start_operation(restack_request(), window, cx);
    });
    service.complete_next_error(dirty_error(OperationSideEffects::None));
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).workspace().unwrap().state().generation()),
        generation
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        0
    );
}

#[gpui::test]
fn partial_submit_refreshes_actual_remote_state_and_rejects_old_hydration(cx: &mut TestAppContext) {
    let (app, cx, service) =
        open_loaded_app_with_refresh_snapshot(cx, snapshot_after_partial_submit());
    let old_details = app.update_in(cx, |app, _window, _cx| {
        app.workspace_mut()
            .unwrap()
            .state_mut()
            .begin_details_load("child")
            .unwrap()
    });

    app.update_in(cx, |app, window, cx| {
        app.start_operation(submit_request(), window, cx);
    });
    service.complete_next_error(partial_submit_error(
        OperationSideEffects::RemoteMayHaveChanged,
    ));
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| {
            app.read(gpui)
                .workspace()
                .unwrap()
                .state()
                .snapshot()
                .current_branch
                .clone()
        }),
        "child-updated"
    );
    assert!(!app.update_in(cx, |app, _window, _cx| {
        app.workspace_mut()
            .unwrap()
            .state_mut()
            .apply_details(old_details, Ok(details("stale")))
    }));
}

#[gpui::test]
fn browser_service_records_only_http_urls(cx: &mut TestAppContext) {
    let (_app, cx, _service, browser) = open_loaded_app_with_browser(cx);

    assert!(
        cx.update(|_, gpui| browser.open_url("https://example.com/pr/1", gpui))
            .is_ok()
    );
    assert!(
        cx.update(|_, gpui| browser.open_url("file:///tmp/not-allowed", gpui))
            .is_err()
    );
    assert_eq!(browser.urls(), vec!["https://example.com/pr/1"]);
}
