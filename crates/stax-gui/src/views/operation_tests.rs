use super::operation_overlay::OperationOverlay;
use super::text_input::BranchNameInput;
use super::{
    AppServices, AppView, PickerFuture, RecentRepositoryStore, RepositoryPicker, SnapshotLoader,
    app::DismissOperationBanner,
};
use crate::hydration::{BranchHydrationService, HydrationFuture};
use crate::operation::{
    BrowserService, FakeOperationService, NativeOperationService, OperationService,
    RecordingBrowserService,
};
use crate::state::WorkspaceState;
use gpui::{
    App, AppContext as _, Context, Entity, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, Modifiers, ParentElement as _, Render, TestAppContext,
    VisualTestContext, Window, actions, div,
};
use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CheckoutOutcome, CiSummary, DiffLine, DiffLineKind,
    OperationError, OperationErrorDetails, OperationErrorKind, OperationEvent, OperationOutcome,
    OperationProgress, OperationReceipt, OperationRequest, OperationResult, OperationSideEffects,
    OperationStage, PullRequestChange, PullRequestMode, PullRequestReceipt, RepositorySnapshot,
    RestackScope, TransactionStatus, TransactionSummary,
};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

actions!(
    input_only_action_probe,
    [
        ProbeCreateBranch,
        ProbeRestackSelected,
        ProbeRestackAll,
        ProbeSubmitStack,
        ProbeOpenPullRequest,
    ]
);

struct InputOnlyActionProbe {
    focus_handle: FocusHandle,
    input: Entity<BranchNameInput>,
    parent_action_count: usize,
}

impl InputOnlyActionProbe {
    fn new(input: Entity<BranchNameInput>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            input,
            parent_action_count: 0,
        }
    }

    fn input(&self) -> Entity<BranchNameInput> {
        self.input.clone()
    }

    fn overlay_guard_enabled(&self) -> bool {
        false
    }

    fn parent_action_count(&self) -> usize {
        self.parent_action_count
    }

    fn record_create(&mut self, _: &ProbeCreateBranch, _: &mut Window, _: &mut Context<Self>) {
        self.parent_action_count += 1;
    }

    fn record_restack(&mut self, _: &ProbeRestackSelected, _: &mut Window, _: &mut Context<Self>) {
        self.parent_action_count += 1;
    }

    fn record_restack_all(&mut self, _: &ProbeRestackAll, _: &mut Window, _: &mut Context<Self>) {
        self.parent_action_count += 1;
    }

    fn record_submit(&mut self, _: &ProbeSubmitStack, _: &mut Window, _: &mut Context<Self>) {
        self.parent_action_count += 1;
    }

    fn record_open_pr(&mut self, _: &ProbeOpenPullRequest, _: &mut Window, _: &mut Context<Self>) {
        self.parent_action_count += 1;
    }
}

impl Focusable for InputOnlyActionProbe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for InputOnlyActionProbe {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context("StaxApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::record_create))
            .on_action(cx.listener(Self::record_restack))
            .on_action(cx.listener(Self::record_restack_all))
            .on_action(cx.listener(Self::record_submit))
            .on_action(cx.listener(Self::record_open_pr))
            .child(self.input.clone())
    }
}

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

fn local_git_error() -> OperationError {
    OperationError {
        request: checkout_request(),
        kind: OperationErrorKind::LocalGit,
        details: OperationErrorDetails::None,
        primary: "Git failed while checking out parent".into(),
        action: "Inspect the repository and retry.".into(),
        diagnostic_chain: "git checkout failed".into(),
        receipt: None,
        side_effects: OperationSideEffects::None,
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

fn click_selector(cx: &mut VisualTestContext, selector: &'static str) {
    let bounds = cx
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} was not rendered"));
    cx.simulate_click(bounds.center(), Modifiers::default());
}

fn install_branch_input_key_bindings(cx: &mut TestAppContext) {
    cx.update(|gpui| {
        gpui.bind_keys([
            gpui::KeyBinding::new(
                "backspace",
                super::text_input::Backspace,
                Some("BranchNameInput"),
            ),
            gpui::KeyBinding::new("delete", super::text_input::Delete, Some("BranchNameInput")),
            gpui::KeyBinding::new("left", super::text_input::Left, Some("BranchNameInput")),
            gpui::KeyBinding::new("right", super::text_input::Right, Some("BranchNameInput")),
            gpui::KeyBinding::new("home", super::text_input::Home, Some("BranchNameInput")),
            gpui::KeyBinding::new("end", super::text_input::End, Some("BranchNameInput")),
            gpui::KeyBinding::new("n", gpui::NoAction, Some("BranchNameInput")),
            gpui::KeyBinding::new("r", gpui::NoAction, Some("BranchNameInput")),
            gpui::KeyBinding::new("shift-r", gpui::NoAction, Some("BranchNameInput")),
            gpui::KeyBinding::new("s", gpui::NoAction, Some("BranchNameInput")),
            gpui::KeyBinding::new("p", gpui::NoAction, Some("BranchNameInput")),
        ]);
    });
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

fn open_loaded_app_with_dirty_restack(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    open_loaded_app(cx)
}

fn open_loaded_app_with_focused_stack_row(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    open_loaded_app(cx)
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
    cx.update(super::init);
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

fn open_focused_branch_input(
    cx: &mut TestAppContext,
) -> (gpui::Entity<BranchNameInput>, &mut VisualTestContext) {
    open_focused_branch_input_with_text(cx, "")
}

fn open_focused_branch_input_with_text<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (gpui::Entity<BranchNameInput>, &'a mut VisualTestContext) {
    install_branch_input_key_bindings(cx);
    let initial_text = text.to_string();
    let (input, cx) =
        cx.add_window_view(|window, cx| BranchNameInput::new(initial_text.clone(), window, cx));
    cx.run_until_parked();
    (input, cx)
}

fn open_input_only_action_probe(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<InputOnlyActionProbe>,
    gpui::Entity<BranchNameInput>,
    &mut VisualTestContext,
) {
    cx.update(|gpui| {
        gpui.bind_keys([
            gpui::KeyBinding::new("n", ProbeCreateBranch, Some("StaxApp")),
            gpui::KeyBinding::new("r", ProbeRestackSelected, Some("StaxApp")),
            gpui::KeyBinding::new("shift-r", ProbeRestackAll, Some("StaxApp")),
            gpui::KeyBinding::new("s", ProbeSubmitStack, Some("StaxApp")),
            gpui::KeyBinding::new("p", ProbeOpenPullRequest, Some("StaxApp")),
        ]);
    });
    install_branch_input_key_bindings(cx);
    let (probe, cx) = cx.add_window_view(|window, cx| {
        let input = cx.new(|cx| BranchNameInput::new(String::new(), window, cx));
        input.update(cx, |input, _cx| input.focus_handle().focus(window));
        InputOnlyActionProbe::new(input, cx)
    });
    let input = cx.update(|_, gpui| probe.read(gpui).input());
    cx.run_until_parked();
    (probe, input, cx)
}

fn open_create_overlay_with_focused_input(
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<AppView>,
    &mut VisualTestContext,
    Arc<FakeOperationService>,
) {
    let (app, cx, service) = open_loaded_app(cx);
    cx.simulate_keystrokes("n");
    (app, cx, service)
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
    cx.update(super::init);
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
fn active_mutation_disables_operations_open_refresh_and_navigation() {
    let mut state = loaded_workspace_state("/repo");
    state.begin_operation(submit_request()).unwrap();

    let actions = state.interaction_state();

    assert!(!actions.checkout.enabled);
    assert!(!actions.create.enabled);
    assert!(!actions.restack.enabled);
    assert!(!actions.restack_all.enabled);
    assert!(!actions.submit.enabled);
    assert!(!actions.open_pr.enabled);
    assert!(!actions.open_repository.enabled);
    assert!(!actions.refresh.enabled);
    assert!(!actions.navigation.enabled);
    assert!(actions.refresh.reason.unwrap().contains("operation"));
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

    cx.simulate_keystrokes("up enter");
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
        app.workspace_mut()
            .unwrap()
            .apply_snapshot(snapshot("/other", Some("child")));
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

#[gpui::test]
fn dismiss_banner_clears_presentation_without_changing_snapshot(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
    let snapshot = cx.update(|_, gpui| app.read(gpui).snapshot().clone());

    cx.dispatch_action(DismissOperationBanner);

    assert!(!cx.update(|_, gpui| app.read(gpui).banner_is_visible()));
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot().clone()),
        snapshot
    );
}

#[gpui::test]
fn clicking_submit_receipt_url_uses_browser_service(cx: &mut TestAppContext) {
    let (app, cx, service, browser) = open_loaded_app_with_browser(cx);

    cx.simulate_keystrokes("s enter");
    service.complete_submit_with_url("https://github.com/acme/repo/pull/42");
    cx.run_until_parked();
    click_selector(cx, "operation-receipt-url-0");

    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        1
    );
}

#[gpui::test]
fn browser_rejects_non_http_url_as_unsupported_capability(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("up p");
    service.script_pr_url("parent", "file:///tmp/not-allowed");
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).operation_error().unwrap().kind),
        OperationErrorKind::UnsupportedCapability
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        0
    );
}

#[gpui::test]
fn p_resolves_selected_branch_without_checkout_or_refresh(cx: &mut TestAppContext) {
    let (app, cx, service, browser) = open_loaded_app_with_browser(cx);

    cx.simulate_keystrokes("up p");
    service.script_pr_url("parent", "https://github.com/acme/repo/pull/42");
    cx.run_until_parked();

    assert_eq!(
        service.requests(),
        vec![OperationRequest::ResolvePullRequestUrl {
            branch: "parent".into()
        }]
    );
    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).current_branch()),
        "child"
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        0
    );
}

#[gpui::test]
fn toolbar_create_button_dispatches_create_action(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    click_selector(cx, "toolbar-create-branch");

    assert!(service.requests().is_empty());
    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::CreateBranch { parent, .. }) if parent == "child"
    ));
}

#[gpui::test]
fn toolbar_submit_button_opens_submit_confirmation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    click_selector(cx, "toolbar-submit-stack");

    assert!(service.requests().is_empty());
    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::ConfirmSubmit {
            affected_branches,
            mode: PullRequestMode::Draft,
            ..
        }) if affected_branches == vec!["parent", "child"]
    ));
}

#[gpui::test]
fn toolbar_open_and_refresh_buttons_dispatch_actions(cx: &mut TestAppContext) {
    let (app, cx, _service) = open_loaded_app(cx);

    click_selector(cx, "toolbar-refresh");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        1
    );

    click_selector(cx, "toolbar-open");
    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        1
    );
}

#[gpui::test]
fn inspector_checkout_button_dispatches_selected_checkout(cx: &mut TestAppContext) {
    let (_app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("up");
    click_selector(cx, "inspector-checkout");

    assert_eq!(
        service.requests(),
        vec![OperationRequest::Checkout {
            branch: "parent".into()
        }]
    );
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn inspector_restack_button_opens_selected_confirmation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("up");
    click_selector(cx, "inspector-restack");

    assert!(service.requests().is_empty());
    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::ConfirmRestack {
            scope: RestackScope::StackContaining(branch),
            auto_stash: false,
            ..
        }) if branch == "parent"
    ));
}

#[gpui::test]
fn inspector_open_pr_button_resolves_selected_without_checkout(cx: &mut TestAppContext) {
    let (app, cx, service, browser) = open_loaded_app_with_browser(cx);

    cx.simulate_keystrokes("up");
    click_selector(cx, "inspector-open-pr");
    service.script_pr_url("parent", "https://github.com/acme/repo/pull/42");
    cx.run_until_parked();

    assert_eq!(
        service.requests(),
        vec![OperationRequest::ResolvePullRequestUrl {
            branch: "parent".into()
        }]
    );
    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).current_branch()),
        "child"
    );
}

#[gpui::test]
fn enter_n_r_shift_r_s_p_shortcuts_dispatch_exact_requests(cx: &mut TestAppContext) {
    let (_app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("up enter");
    assert_eq!(
        service.requests(),
        vec![OperationRequest::Checkout {
            branch: "parent".into()
        }]
    );
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    cx.simulate_keystrokes("n");
    cx.simulate_input("daily-stack-work");
    cx.simulate_keystrokes("enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::CreateBranch { name, parent })
            if name == "daily-stack-work" && parent == "parent"
    ));
    service.complete_next_success(OperationReceipt {
        request: OperationRequest::CreateBranch {
            name: "daily-stack-work".into(),
            parent: "parent".into(),
        },
        summary: "Created daily-stack-work".into(),
        affected_branches: vec!["daily-stack-work".into()],
        outcome: OperationOutcome::BranchCreated {
            branch: "daily-stack-work".into(),
            parent: "parent".into(),
        },
        transaction: None,
        warnings: Vec::new(),
        side_effects: OperationSideEffects::RepositoryChanged,
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("r enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::Restack {
            scope: RestackScope::StackContaining(branch),
            auto_stash: false,
        }) if branch == "parent"
    ));
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    cx.simulate_keystrokes("shift-r enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::Restack {
            scope: RestackScope::All,
            auto_stash: false,
        })
    ));
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();

    cx.simulate_keystrokes("s enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::SubmitStack {
            new_pull_requests: PullRequestMode::Draft,
        })
    ));
    service.complete_next_success(submit_receipt());
    cx.run_until_parked();

    cx.simulate_keystrokes("p");
    service.script_pr_url("parent", "https://github.com/acme/repo/pull/42");
    cx.run_until_parked();
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::ResolvePullRequestUrl { branch }) if branch == "parent"
    ));
}

#[gpui::test]
fn progress_banner_renders_stage_branch_and_completed_total(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    app.update_in(cx, |app, _window, cx| {
        let token = app
            .workspace()
            .unwrap()
            .state()
            .active_operation()
            .unwrap()
            .token
            .clone();
        app.workspace_mut().unwrap().apply_operation_event(
            &token,
            OperationEvent::Progress(OperationProgress {
                stage: OperationStage::CheckingOut,
                completed: 1,
                total: Some(2),
                branch: Some("parent".into()),
                message: "checking out parent".into(),
            }),
        );
        cx.notify();
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("operation-banner").is_some());
    assert!(cx.debug_bounds("operation-progress").is_some());

    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn copy_diagnostics_button_writes_only_diagnostic_chain(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_error(local_git_error());
    cx.run_until_parked();
    click_selector(cx, "operation-copy-diagnostics");

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).copied_diagnostics()),
        Some("git checkout failed".to_string())
    );
}

#[gpui::test]
fn create_button_stays_disabled_for_empty_or_invalid_name(cx: &mut TestAppContext) {
    let (_app, cx, _service) = open_loaded_app(cx);

    click_selector(cx, "toolbar-create-branch");

    assert!(
        cx.debug_bounds("operation-overlay-confirm-disabled")
            .is_some()
    );
}

#[gpui::test]
fn create_enter_shows_validation_without_request(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    click_selector(cx, "toolbar-create-branch");
    cx.simulate_keystrokes("enter");

    assert!(service.requests().is_empty());
    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::CreateBranch {
            validation_error: Some(_),
            ..
        })
    ));
}

#[gpui::test]
fn dismiss_button_and_action_clear_only_presentation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_error(local_git_error());
    cx.run_until_parked();
    let snapshot = cx.update(|_, gpui| app.read(gpui).snapshot().clone());

    click_selector(cx, "operation-banner-dismiss");
    assert!(!cx.update(|_, gpui| app.read(gpui).banner_is_visible()));
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot().clone()),
        snapshot
    );
}

#[gpui::test]
fn receipt_url_rendered_row_opens_recording_browser(cx: &mut TestAppContext) {
    let (_app, cx, service, browser) = open_loaded_app_with_browser(cx);

    cx.simulate_keystrokes("s enter");
    service.complete_submit_with_url("https://github.com/acme/repo/pull/42");
    cx.run_until_parked();
    click_selector(cx, "operation-receipt-url-0");

    assert_eq!(browser.urls(), vec!["https://github.com/acme/repo/pull/42"]);
}

#[gpui::test]
fn every_action_is_disabled_during_active_mutation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("s enter");
    assert_eq!(service.requests(), vec![submit_request()]);
    cx.simulate_keystrokes("up down enter n r shift-r s p cmd-r cmd-o");
    for selector in [
        "toolbar-open",
        "toolbar-refresh",
        "toolbar-create-branch",
        "toolbar-submit-stack",
        "inspector-checkout",
        "inspector-restack",
        "inspector-open-pr",
    ] {
        click_selector(cx, selector);
    }

    assert_eq!(service.requests(), vec![submit_request()]);
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).selected_branch()),
        "child"
    );
    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).snapshot_refresh_count()),
        0
    );

    service.complete_next_success(submit_receipt());
    cx.run_until_parked();
    cx.simulate_keystrokes("up enter");
    assert_eq!(
        service.requests().last(),
        Some(&OperationRequest::Checkout {
            branch: "parent".into()
        })
    );
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn branch_input_inserts_platform_text_and_backspaces_one_grapheme(cx: &mut TestAppContext) {
    let (input, cx) = open_focused_branch_input(cx);

    cx.simulate_input("feature-🦀");
    cx.simulate_keystrokes("backspace");

    assert_eq!(
        cx.update(|_, app| input.read(app).text().to_string()),
        "feature-"
    );
}

#[gpui::test]
fn branch_input_maps_utf16_replacement_ranges(cx: &mut TestAppContext) {
    let (input, cx) = open_focused_branch_input_with_text(cx, "a🦀b");

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            EntityInputHandler::replace_text_in_range(input, Some(1..3), "x", window, cx);
        });
    });

    assert_eq!(
        cx.update(|_, app| input.read(app).text().to_string()),
        "axb"
    );
}

#[gpui::test]
fn branch_input_tracks_and_commits_ime_marked_text(cx: &mut TestAppContext) {
    let (input, cx) = open_focused_branch_input(cx);

    cx.update(|window, app| {
        input.update(app, |input, cx| {
            EntityInputHandler::replace_and_mark_text_in_range(
                input,
                None,
                "に",
                Some(1..1),
                window,
                cx,
            );
            assert_eq!(
                EntityInputHandler::marked_text_range(input, window, cx),
                Some(0..1)
            );
            EntityInputHandler::replace_text_in_range(input, None, "日本", window, cx);
        });
    });
    cx.update(|window, app| {
        input.update(app, |input, cx| {
            assert_eq!(input.text(), "日本");
            assert_eq!(
                EntityInputHandler::marked_text_range(input, window, cx),
                None
            );
            assert_eq!(
                EntityInputHandler::selected_text_range(input, false, window, cx)
                    .unwrap()
                    .range,
                2..2
            );
        });
    });
}

#[gpui::test]
fn input_key_context_suppresses_parent_actions_without_overlay_guard(cx: &mut TestAppContext) {
    let (probe, input, cx) = open_input_only_action_probe(cx);

    assert!(cx.update(|window, app| input.read(app).focus_handle().is_focused(window)));
    assert!(!cx.update(|_, app| probe.read(app).overlay_guard_enabled()));
    cx.simulate_keystrokes("n r shift-r s p");

    assert_eq!(cx.update(|_, app| probe.read(app).parent_action_count()), 0);
}

#[gpui::test]
fn text_insertion_is_independent_from_shortcut_suppression(cx: &mut TestAppContext) {
    let (app, cx, _service) = open_create_overlay_with_focused_input(cx);

    cx.simulate_input("nrsp");

    assert_eq!(
        cx.update(|_, gpui| app.read(gpui).branch_input_text()),
        "nrsp"
    );
}

#[gpui::test]
fn s_opens_submit_confirmation_before_request(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("s");

    assert!(service.requests().is_empty());
    let overlay = cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned());
    assert!(matches!(
        overlay,
        Some(OperationOverlay::ConfirmSubmit {
            affected_branches,
            mode: PullRequestMode::Draft,
            ..
        }) if affected_branches == vec!["parent", "child"]
    ));
}

#[gpui::test]
fn submit_enter_confirms_and_escape_cancels(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("s escape");
    assert!(service.requests().is_empty());
    cx.simulate_keystrokes("s enter");

    assert_eq!(service.requests(), vec![submit_request()]);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_overlay().is_none()));
    service.complete_next_success(submit_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn dirty_restack_uses_explicit_stash_confirmation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_dirty_restack(cx);

    cx.simulate_keystrokes("r enter");
    service.complete_next_error(dirty_error(OperationSideEffects::None));
    cx.run_until_parked();

    assert!(matches!(
        cx.update(|_, gpui| app.read(gpui).operation_overlay().cloned()),
        Some(OperationOverlay::ConfirmStashAndRestack { .. })
    ));
    cx.simulate_keystrokes("enter");
    assert!(matches!(
        service.requests().last(),
        Some(OperationRequest::Restack {
            auto_stash: true,
            ..
        })
    ));
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn modal_cancel_and_completion_restore_prior_focus(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_focused_stack_row(cx);
    let prior = cx.update(|window, gpui| window.focused(gpui));

    cx.simulate_keystrokes("n escape");
    assert_eq!(cx.update(|window, gpui| window.focused(gpui)), prior);
    cx.simulate_keystrokes("s enter");
    service.complete_next_success(submit_receipt());
    cx.run_until_parked();

    assert_eq!(cx.update(|window, gpui| window.focused(gpui)), prior);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_overlay().is_none()));
}

#[gpui::test]
fn escape_during_restack_or_submit_does_not_cancel_active_mutation(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app(cx);

    cx.simulate_keystrokes("r enter escape");
    assert!(cx.update(|_, gpui| app.read(gpui).active_operation().is_some()));
    assert_eq!(service.requests().len(), 1);
    service.complete_next_success(checkout_receipt());
    cx.run_until_parked();
    cx.simulate_keystrokes("s enter escape");
    assert!(cx.update(|_, gpui| app.read(gpui).active_operation().is_some()));
    assert_eq!(service.requests().len(), 2);
    service.complete_next_success(submit_receipt());
    cx.run_until_parked();
}

#[gpui::test]
fn terminal_error_restores_prior_focus(cx: &mut TestAppContext) {
    let (app, cx, service) = open_loaded_app_with_focused_stack_row(cx);
    let prior = cx.update(|window, gpui| window.focused(gpui));

    app.update_in(cx, |app, window, cx| {
        app.start_operation(checkout_request(), window, cx);
    });
    service.complete_next_error(local_git_error());
    cx.run_until_parked();

    assert_eq!(cx.update(|window, gpui| window.focused(gpui)), prior);
    assert!(cx.update(|_, gpui| app.read(gpui).operation_error().is_some()));
}
