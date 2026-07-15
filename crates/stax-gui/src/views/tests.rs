use super::{
    AppModeKind, AppServices, AppView, PaneMarkers, PickerFuture, RecentRepositoryStore,
    RepositoryPicker, RootLoadKind, SnapshotLoader,
};
use super::{changes_pane, inspector_pane, stack_pane};
use crate::hydration::{BranchHydrationService, HydrationFuture};
use crate::preferences::{
    PaneVisibility, PaneWidths, WorkspacePreferenceStore, WorkspacePreferences,
};
use crate::state::{ActionAvailability, LoadState};
use gpui::{
    App, KeyDownEvent, KeyUpEvent, Keystroke, MenuItem, Modifiers, MouseButton, TestAppContext,
    point, px,
};
use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DiffLine, DiffLineKind, RepositorySnapshot,
};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::task::{Context as TaskContext, Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};

#[path = "hydration_tests.rs"]
mod hydration_tests;

#[derive(Clone)]
struct FixtureLoader {
    result: Result<RepositorySnapshot, String>,
}

impl SnapshotLoader for FixtureLoader {
    fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
        self.result.clone()
    }
}

struct PathAwareLoader {
    rejected: Option<PathBuf>,
}

impl SnapshotLoader for PathAwareLoader {
    fn load(&self, path: &Path) -> Result<RepositorySnapshot, String> {
        if self.rejected.as_deref() == Some(path) {
            return Err(format!("{} is not a Git repository", path.display()));
        }
        Ok(snapshot(&path.display().to_string()))
    }
}

struct FixturePicker {
    result: Result<Option<PathBuf>, String>,
    calls: Arc<AtomicUsize>,
}

impl FixturePicker {
    fn new(result: Result<Option<PathBuf>, String>) -> Self {
        Self {
            result,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn with_calls(result: Result<Option<PathBuf>, String>, calls: Arc<AtomicUsize>) -> Self {
        Self { result, calls }
    }
}

impl RepositoryPicker for FixturePicker {
    fn pick(&self, _cx: &mut App) -> PickerFuture {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

#[derive(Default)]
struct FixtureRecents {
    paths: Mutex<Vec<PathBuf>>,
    load_error: Option<String>,
    record_error: Option<String>,
    record_attempts: AtomicUsize,
}

#[derive(Default)]
struct FixtureWorkspacePreferences {
    values: Mutex<std::collections::HashMap<PathBuf, WorkspacePreferences>>,
    saves: Mutex<Vec<(PathBuf, WorkspacePreferences)>>,
}

impl FixtureWorkspacePreferences {
    fn with(repository: impl Into<PathBuf>, preferences: WorkspacePreferences) -> Self {
        Self {
            values: Mutex::new(std::collections::HashMap::from([(
                repository.into(),
                preferences,
            )])),
            saves: Mutex::new(Vec::new()),
        }
    }

    fn saved(&self) -> Vec<(PathBuf, WorkspacePreferences)> {
        self.saves.lock().unwrap().clone()
    }
}

impl WorkspacePreferenceStore for FixtureWorkspacePreferences {
    fn load(&self, repository: &Path) -> WorkspacePreferences {
        self.values
            .lock()
            .unwrap()
            .get(repository)
            .cloned()
            .unwrap_or_default()
    }

    fn save(&self, repository: &Path, preferences: &WorkspacePreferences) -> Result<(), String> {
        self.values
            .lock()
            .unwrap()
            .insert(repository.to_path_buf(), preferences.clone());
        self.saves
            .lock()
            .unwrap()
            .push((repository.to_path_buf(), preferences.clone()));
        Ok(())
    }
}

impl RecentRepositoryStore for FixtureRecents {
    fn load(&self) -> Result<Vec<PathBuf>, String> {
        self.load_error.as_ref().map_or_else(
            || Ok(self.paths.lock().unwrap().clone()),
            |error| Err(error.clone()),
        )
    }

    fn record(&self, path: &Path) -> Result<(), String> {
        self.record_attempts.fetch_add(1, Ordering::SeqCst);
        if let Some(error) = &self.record_error {
            return Err(error.clone());
        }
        self.paths.lock().unwrap().insert(0, path.to_path_buf());
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HydrationCall {
    Details { repository: PathBuf, branch: String },
    CachedDiff { branch: String, parent: String },
    Diff { branch: String, parent: String },
    Ci { repository: PathBuf, branch: String },
}

type DetailsHandler =
    dyn Fn(PathBuf, BranchSummary) -> HydrationFuture<BranchDetails> + Send + Sync;
type CachedDiffHandler =
    dyn Fn(PathBuf, String, String) -> HydrationFuture<Option<BranchDiff>> + Send + Sync;
type DiffHandler = dyn Fn(PathBuf, String, String) -> HydrationFuture<BranchDiff> + Send + Sync;
type CiHandler = dyn Fn(PathBuf, String) -> HydrationFuture<CiSummary> + Send + Sync;

struct FixtureHydration {
    calls: Mutex<Vec<HydrationCall>>,
    details: Arc<DetailsHandler>,
    cached_diff: Arc<CachedDiffHandler>,
    diff: Arc<DiffHandler>,
    ci: Arc<CiHandler>,
}

impl FixtureHydration {
    fn new(
        details: impl Fn(&Path, &BranchSummary) -> Result<BranchDetails, String> + Send + Sync + 'static,
        cached_diff: impl Fn(&Path, &str, &str) -> Result<Option<BranchDiff>, String>
        + Send
        + Sync
        + 'static,
        diff: impl Fn(&Path, &str, &str) -> Result<BranchDiff, String> + Send + Sync + 'static,
        ci: impl Fn(&Path, &str) -> Result<CiSummary, String> + Send + Sync + 'static,
    ) -> Self {
        let details = Arc::new(details);
        let cached_diff = Arc::new(cached_diff);
        let diff = Arc::new(diff);
        let ci = Arc::new(ci);
        Self::new_async(
            move |repository, branch| {
                let details = Arc::clone(&details);
                Box::pin(async move { details(&repository, &branch) })
            },
            move |repository, branch, parent| {
                let cached_diff = Arc::clone(&cached_diff);
                Box::pin(async move { cached_diff(&repository, &branch, &parent) })
            },
            move |repository, branch, parent| {
                let diff = Arc::clone(&diff);
                Box::pin(async move { diff(&repository, &branch, &parent) })
            },
            move |repository, branch| {
                let ci = Arc::clone(&ci);
                Box::pin(async move { ci(&repository, &branch) })
            },
        )
    }

    fn new_async(
        details: impl Fn(PathBuf, BranchSummary) -> HydrationFuture<BranchDetails>
        + Send
        + Sync
        + 'static,
        cached_diff: impl Fn(PathBuf, String, String) -> HydrationFuture<Option<BranchDiff>>
        + Send
        + Sync
        + 'static,
        diff: impl Fn(PathBuf, String, String) -> HydrationFuture<BranchDiff> + Send + Sync + 'static,
        ci: impl Fn(PathBuf, String) -> HydrationFuture<CiSummary> + Send + Sync + 'static,
    ) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            details: Arc::new(details),
            cached_diff: Arc::new(cached_diff),
            diff: Arc::new(diff),
            ci: Arc::new(ci),
        }
    }

    fn immediate_no_remote() -> Self {
        Self::new(
            |_, _| Ok(details(1, false)),
            |_, _, _| Ok(None),
            |_, branch, _| Ok(diff(&format!("{branch} patch"))),
            |_, _| Ok(ci("success")),
        )
    }

    fn calls(&self) -> Vec<HydrationCall> {
        self.calls.lock().unwrap().clone()
    }
}

impl BranchHydrationService for FixtureHydration {
    fn load_details(
        &self,
        repository: PathBuf,
        branch: BranchSummary,
    ) -> HydrationFuture<BranchDetails> {
        self.calls.lock().unwrap().push(HydrationCall::Details {
            repository: repository.clone(),
            branch: branch.name.clone(),
        });
        (self.details)(repository, branch)
    }

    fn load_cached_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<Option<BranchDiff>> {
        self.calls.lock().unwrap().push(HydrationCall::CachedDiff {
            branch: branch.clone(),
            parent: parent.clone(),
        });
        (self.cached_diff)(repository, branch, parent)
    }

    fn load_diff(
        &self,
        repository: PathBuf,
        branch: String,
        parent: String,
    ) -> HydrationFuture<BranchDiff> {
        self.calls.lock().unwrap().push(HydrationCall::Diff {
            branch: branch.clone(),
            parent: parent.clone(),
        });
        (self.diff)(repository, branch, parent)
    }

    fn load_ci(&self, repository: PathBuf, branch: String) -> HydrationFuture<CiSummary> {
        self.calls.lock().unwrap().push(HydrationCall::Ci {
            repository: repository.clone(),
            branch: branch.clone(),
        });
        (self.ci)(repository, branch)
    }
}

#[derive(Default)]
struct GateState {
    started: bool,
    released: bool,
    waker: Option<Waker>,
}

#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    changed: Condvar,
}

impl Gate {
    fn wait(self: &Arc<Self>) -> GateWait {
        GateWait {
            gate: Arc::clone(self),
        }
    }

    fn wait_until_started(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        while !state.started {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, timed_out) = self.changed.wait_timeout(state, remaining).unwrap();
            state = next;
            if timed_out.timed_out() && !state.started {
                return false;
            }
        }
        true
    }

    fn release(&self) {
        let waker = {
            let mut state = self.state.lock().unwrap();
            state.released = true;
            self.changed.notify_all();
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }
}

struct GateWait {
    gate: Arc<Gate>,
}

impl std::future::Future for GateWait {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        let mut state = self.gate.state.lock().unwrap();
        state.started = true;
        self.gate.changed.notify_all();
        if state.released {
            Poll::Ready(())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

fn details(ahead: usize, has_remote: bool) -> BranchDetails {
    BranchDetails {
        ahead,
        behind: 0,
        has_remote,
        unpushed: usize::from(has_remote),
        unpulled: 0,
        commits: vec![format!("commit-{ahead}")],
    }
}

fn diff(content: &str) -> BranchDiff {
    BranchDiff {
        stat: Vec::new(),
        lines: vec![DiffLine {
            content: content.to_string(),
            kind: DiffLineKind::Context,
        }],
    }
}

fn ci(status: &str) -> CiSummary {
    CiSummary {
        overall_status: Some(status.to_string()),
        total: 1,
        passed: usize::from(status == "success"),
        failed: usize::from(status == "failure"),
        running: 0,
        queued: 0,
        skipped: 0,
        started_at: None,
        completed_at: None,
        average_secs: None,
    }
}

fn branch(name: &str, parent: Option<&str>, current: bool, trunk: bool) -> BranchSummary {
    BranchSummary {
        name: name.into(),
        parent: parent.map(str::to_string),
        column: if trunk { 0 } else { 1 },
        is_current: current,
        is_trunk: trunk,
        needs_restack: name == "feature-b",
        pr_number: (name == "feature-a").then_some(42),
        pr_state: (name == "feature-a").then(|| "open".into()),
        ci_state: (name == "feature-a").then(|| "success".into()),
    }
}

fn snapshot(path: &str) -> RepositorySnapshot {
    RepositorySnapshot {
        repository_root: PathBuf::from(path),
        current_branch: "feature-a".into(),
        trunk: "main".into(),
        branches: vec![
            branch("feature-b", Some("feature-a"), false, false),
            branch("feature-a", Some("main"), true, false),
            branch("main", None, false, true),
        ],
    }
}

fn services(
    loader: Result<RepositorySnapshot, String>,
    picker: Result<Option<PathBuf>, String>,
    recents: Arc<FixtureRecents>,
) -> AppServices {
    services_with_hydration(
        loader,
        picker,
        recents,
        Arc::new(FixtureHydration::immediate_no_remote()),
    )
}

fn services_with_hydration(
    loader: Result<RepositorySnapshot, String>,
    picker: Result<Option<PathBuf>, String>,
    recents: Arc<FixtureRecents>,
    hydration: Arc<dyn BranchHydrationService>,
) -> AppServices {
    AppServices::with_hydration(
        Arc::new(FixtureLoader { result: loader }),
        Rc::new(FixturePicker::new(picker)),
        recents,
        hydration,
    )
}

fn services_with_workspace_preferences(
    loader: Result<RepositorySnapshot, String>,
    preferences: Arc<dyn WorkspacePreferenceStore>,
) -> AppServices {
    AppServices::with_workspace_preferences(
        Arc::new(FixtureLoader { result: loader }),
        Rc::new(FixturePicker::new(Ok(None))),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
        preferences,
    )
}

fn path_aware_services(
    picker: Rc<dyn RepositoryPicker>,
    recents: Arc<FixtureRecents>,
    rejected: Option<PathBuf>,
) -> AppServices {
    AppServices::with_hydration(
        Arc::new(PathAwareLoader { rejected }),
        picker,
        recents,
        Arc::new(FixtureHydration::immediate_no_remote()),
    )
}

#[gpui::test]
fn no_path_renders_the_welcome_mode_without_panicking(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Welcome
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).inline_error().map(str::to_string)),
        None
    );
}

#[gpui::test]
fn no_path_reopens_the_most_recent_repository(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![
            PathBuf::from("/recent/last"),
            PathBuf::from("/recent/older"),
        ]),
        ..Default::default()
    });
    let services = path_aware_services(
        Rc::new(FixturePicker::new(Ok(None))),
        Arc::clone(&recents),
        None,
    );
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/recent/last"))
    );
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 1);
}

#[gpui::test]
fn invalid_restored_repository_returns_to_the_actionable_error_view(cx: &mut TestAppContext) {
    let rejected = PathBuf::from("/recent/not-a-repository");
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![rejected.clone()]),
        ..Default::default()
    });
    let services = path_aware_services(
        Rc::new(FixturePicker::new(Ok(None))),
        recents,
        Some(rejected.clone()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Error
    );
    assert!(cx.update(|_, app| {
        view.read(app)
            .inline_error()
            .is_some_and(|error| error.contains("is not a Git repository"))
    }));
}

#[gpui::test]
fn workspace_renders_codex_presentation_landmarks(cx: &mut TestAppContext) {
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::new(FixtureWorkspacePreferences::default()),
    );
    let (_view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    for selector in [
        "stack-topology-gutter",
        "stack-topology-cell",
        "stack-topology-vertical-rail",
        "stack-topology-node",
        "changes-file-summary",
        "inspector-card",
        "inspector-branch-strip",
        "inspector-health-section",
        "inspector-metrics",
        "inspector-pr-section",
        "inspector-ci-section",
        "inspector-actions-section",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "expected {selector} to be rendered"
        );
    }
}

#[gpui::test]
fn project_switcher_lists_recent_repositories_and_switches_on_release(cx: &mut TestAppContext) {
    cx.update(super::init);
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![
            PathBuf::from("/projects/current"),
            PathBuf::from("/projects/other"),
        ]),
        ..Default::default()
    });
    let services = path_aware_services(Rc::new(FixturePicker::new(Ok(None))), recents, None);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(
            Some(PathBuf::from("/projects/current")),
            services,
            window,
            cx,
        )
    });
    cx.run_until_parked();

    assert!(cx.debug_bounds("project-switcher-control").is_some());
    assert!(cx.debug_bounds("project-switcher-menu").is_none());

    let switcher = cx
        .debug_bounds("project-switcher-control")
        .expect("project switcher control should be rendered")
        .center();
    cx.simulate_mouse_move(switcher, None, Modifiers::default());
    cx.simulate_mouse_down(switcher, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(switcher, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert!(cx.debug_bounds("project-switcher-menu").is_some());
    assert!(cx.debug_bounds("project-switcher-add").is_some());
    let other = cx
        .debug_bounds("project-switcher-repository-0")
        .expect("other recent repository should be rendered")
        .center();
    cx.simulate_mouse_move(other, None, Modifiers::default());
    cx.simulate_mouse_down(other, MouseButton::Left, Modifiers::default());
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/projects/current"))
    );
    cx.simulate_mouse_up(other, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/projects/other"))
    );
    assert!(!cx.update(|_, app| view.read(app).project_switcher_is_open()));
}

#[gpui::test]
fn project_switcher_add_opens_and_keeps_the_selected_repository_visible(cx: &mut TestAppContext) {
    cx.update(super::init);
    let picker_calls = Arc::new(AtomicUsize::new(0));
    let services = path_aware_services(
        Rc::new(FixturePicker::with_calls(
            Ok(Some(PathBuf::from("/projects/new"))),
            Arc::clone(&picker_calls),
        )),
        Arc::new(FixtureRecents::default()),
        None,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(
            Some(PathBuf::from("/projects/current")),
            services,
            window,
            cx,
        )
    });
    cx.run_until_parked();

    let switcher = cx
        .debug_bounds("project-switcher-control")
        .expect("project switcher control should be rendered")
        .center();
    cx.simulate_mouse_move(switcher, None, Modifiers::default());
    cx.simulate_mouse_down(switcher, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(switcher, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    let add = cx
        .debug_bounds("project-switcher-add")
        .expect("Add Project control should be rendered")
        .center();
    cx.simulate_mouse_move(add, None, Modifiers::default());
    cx.simulate_mouse_down(add, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(add, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();

    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/projects/new"))
    );

    view.update_in(cx, |view, _window, cx| view.toggle_project_switcher(cx));
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("project-switcher-current").is_some(),
        "the selected project should remain visible as the current recent project"
    );
}

#[gpui::test]
fn narrow_inspector_keeps_long_branch_identities_single_line(cx: &mut TestAppContext) {
    let mut long_snapshot = snapshot("/repo");
    let long_name = format!("feature/{}", "very-long-branch-name-".repeat(12));
    let long_parent = format!("parent/{}", "equally-long-parent-name-".repeat(12));
    long_snapshot.current_branch = long_name.clone();
    long_snapshot.branches[1].name = long_name;
    long_snapshot.branches[1].parent = Some(long_parent);
    let preferences = WorkspacePreferences {
        visibility: PaneVisibility::default(),
        widths: PaneWidths {
            stack: 0.25,
            changes: 0.60,
            inspector: 0.15,
        },
    };
    let services = services_with_workspace_preferences(
        Ok(long_snapshot),
        Arc::new(FixtureWorkspacePreferences::with("/repo", preferences)),
    );
    let (_view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    let title = cx
        .debug_bounds("inspector-branch-identity")
        .expect("branch identity should render");
    let parent = cx
        .debug_bounds("inspector-parent-identity")
        .expect("parent identity should render");
    let card = cx
        .debug_bounds("inspector-card")
        .expect("inspector card should render");

    assert!(title.size.width <= card.size.width);
    assert!(parent.size.width <= card.size.width);
    assert!(title.size.height <= px(24.0));
    assert!(parent.size.height <= px(20.0));
}

#[gpui::test]
fn keyboard_welcome_open_activates_once_for_enter_and_space(cx: &mut TestAppContext) {
    cx.update(super::init);
    let picker_calls = Arc::new(AtomicUsize::new(0));
    let services = AppServices::with_hydration(
        Arc::new(FixtureLoader {
            result: Ok(snapshot("/repo")),
        }),
        Rc::new(FixturePicker::with_calls(
            Ok(None),
            Arc::clone(&picker_calls),
        )),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
    );
    let (_view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));
    cx.run_until_parked();

    cx.simulate_keystrokes("enter space");
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);

    cx.update(|window, _| window.focus_next());
    cx.simulate_keystrokes("enter");
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);

    cx.simulate_keystrokes("space");
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: true,
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 2);
}

#[gpui::test]
fn mouse_welcome_open_activates_on_release_and_retains_keyboard_focus(cx: &mut TestAppContext) {
    cx.update(super::init);
    let picker_calls = Arc::new(AtomicUsize::new(0));
    let services = AppServices::with_hydration(
        Arc::new(FixtureLoader {
            result: Ok(snapshot("/repo")),
        }),
        Rc::new(FixturePicker::with_calls(
            Ok(None),
            Arc::clone(&picker_calls),
        )),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
    );
    let (_view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));
    cx.run_until_parked();
    let position = cx
        .debug_bounds("welcome-open-repository-control")
        .expect("welcome Open Repository control was not rendered")
        .center();

    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);

    cx.simulate_keystrokes("enter");
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 2);
}

#[gpui::test]
fn keyboard_recent_repository_row_enter_activates_once(cx: &mut TestAppContext) {
    assert_recent_repository_row_key_activates_once("enter", cx);
}

#[gpui::test]
fn keyboard_recent_repository_row_space_activates_once(cx: &mut TestAppContext) {
    assert_recent_repository_row_key_activates_once("space", cx);
}

fn assert_recent_repository_row_key_activates_once(key: &str, cx: &mut TestAppContext) {
    cx.update(super::init);
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![
            PathBuf::from("/current/repo"),
            PathBuf::from("/recent/repo"),
        ]),
        ..Default::default()
    });
    let services = path_aware_services(
        Rc::new(FixturePicker::new(Ok(None))),
        Arc::clone(&recents),
        None,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/current/repo")), services, window, cx)
    });
    cx.run_until_parked();

    let switcher = cx
        .debug_bounds("project-switcher-control")
        .expect("project switcher control should be rendered")
        .center();
    cx.simulate_mouse_move(switcher, None, Modifiers::default());
    cx.simulate_mouse_down(switcher, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_up(switcher, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, _| window.focus_next());
    cx.simulate_keystrokes(key);
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse(key).unwrap(),
        is_held: true,
    });
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 1);

    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse(key).unwrap(),
    });
    cx.run_until_parked();
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/recent/repo"))
    );
}

#[gpui::test]
fn mouse_recent_repository_row_activates_once_on_release(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![
            PathBuf::from("/current/repo"),
            PathBuf::from("/recent/repo"),
        ]),
        ..Default::default()
    });
    let services = path_aware_services(
        Rc::new(FixturePicker::new(Ok(None))),
        Arc::clone(&recents),
        None,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/current/repo")), services, window, cx)
    });
    cx.run_until_parked();
    view.update_in(cx, |view, _window, cx| view.toggle_project_switcher(cx));
    cx.run_until_parked();
    let position = cx
        .debug_bounds("project-switcher-repository-0")
        .expect("first recent repository control was not rendered")
        .center();

    cx.simulate_mouse_move(position, None, Modifiers::default());
    cx.simulate_mouse_down(position, MouseButton::Left, Modifiers::default());
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 1);
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/current/repo"))
    );

    cx.simulate_mouse_up(position, MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .map(|workspace| workspace.state().snapshot().repository_root.clone())
        }),
        Some(PathBuf::from("/recent/repo"))
    );
}

#[gpui::test]
fn recent_repositories_load_after_the_welcome_first_paint(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, app| view.read(app).recent_repositories().to_vec()),
        Vec::<PathBuf>::new()
    );

    let token = view.update_in(cx, |view, _window, cx| {
        let token = view.begin_recent_load();
        cx.notify();
        token
    });
    cx.run_until_parked();
    assert!(cx.update(|_, app| view.read(app).recent_load_is_pending()));
    assert!(cx.debug_bounds("recent-repositories-loading").is_some());

    view.update_in(cx, |view, _window, cx| {
        assert!(view.apply_recent_load_result(token, Ok(vec![PathBuf::from("/recent/repo")]), cx,));
    });
    assert!(!cx.update(|_, app| view.read(app).recent_load_is_pending()));
}

#[gpui::test]
fn stale_recent_load_results_cannot_replace_newer_results(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    view.update_in(cx, |view, _window, cx| {
        let first = view.begin_recent_load();
        let second = view.begin_recent_load();
        assert!(!view.apply_recent_load_result(first, Ok(vec![PathBuf::from("/stale")]), cx,));
        assert!(view.apply_recent_load_result(second, Ok(vec![PathBuf::from("/current")]), cx,));
    });

    assert_eq!(
        cx.update(|_, app| view.read(app).recent_repositories().to_vec()),
        vec![PathBuf::from("/current")]
    );
}

#[gpui::test]
fn initial_path_loads_a_workspace_and_records_recents_after_success(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), Arc::clone(&recents));
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Workspace
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).pane_markers()),
        Some(PaneMarkers::all())
    );
    for marker in [
        PaneMarkers::all().stack,
        PaneMarkers::all().changes,
        PaneMarkers::all().inspector,
    ] {
        assert!(
            cx.debug_bounds(marker).is_some(),
            "{marker} was not rendered"
        );
    }
    assert_eq!(
        recents.paths.lock().unwrap().as_slice(),
        &[PathBuf::from("/repo")]
    );
}

#[gpui::test]
fn invalid_initial_path_returns_to_actionable_welcome_error(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(
        Err("not a git repository; choose another folder".into()),
        Ok(None),
        recents,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/invalid")), services, window, cx)
    });

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Error
    );
    assert!(cx.update(|_, app| {
        view.read(app)
            .inline_error()
            .is_some_and(|error| error.contains("choose another folder"))
    }));
}

#[gpui::test]
fn picker_cancellation_leaves_the_current_mode_unchanged(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    view.update_in(cx, |view, window, cx| {
        view.pick_repository(window, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Welcome
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).inline_error().map(str::to_string)),
        None
    );
}

#[gpui::test]
fn picker_errors_are_inline_and_nonfatal(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(
        Ok(snapshot("/repo")),
        Err("folder picker unavailable".into()),
        recents,
    );
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    view.update_in(cx, |view, window, cx| {
        view.pick_repository(window, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Welcome
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).inline_error().map(str::to_string)),
        Some("folder picker unavailable".to_string())
    );
}

#[gpui::test]
fn latest_picker_error_takes_precedence_over_an_older_refresh_failure(cx: &mut TestAppContext) {
    let services = services(
        Ok(snapshot("/repo")),
        Err("latest folder picker failure".into()),
        Arc::new(FixtureRecents::default()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    view.update_in(cx, |view, window, cx| {
        let refresh = view.begin_load(PathBuf::from("/repo"), RootLoadKind::Refresh);
        assert!(view.apply_load_result(refresh, Err("older refresh failure".into()), cx));
        view.pick_repository(window, cx);
    });
    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).inline_error().map(str::to_string)),
        Some("latest folder picker failure".to_string())
    );
}

#[gpui::test]
fn recent_storage_errors_stay_inline_with_open_choices(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents {
        load_error: Some("recent repository storage is unreadable".into()),
        ..Default::default()
    });
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Welcome
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).inline_error().map(str::to_string)),
        Some("recent repository storage is unreadable".to_string())
    );
}

#[gpui::test]
fn recent_record_errors_do_not_block_a_successful_workspace(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents {
        record_error: Some("preferences directory is read-only".into()),
        ..Default::default()
    });
    let services = services(Ok(snapshot("/repo")), Ok(None), Arc::clone(&recents));
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Workspace
    );
    assert!(cx.update(|_, app| {
        view.read(app)
            .inline_error()
            .is_some_and(|error| error.contains("recent entry was not saved"))
    }));
    assert!(recents.paths.lock().unwrap().is_empty());
}

#[gpui::test]
fn recent_write_completion_survives_an_intervening_refresh(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |view, _window, cx| {
        let write = view.begin_recent_write(PathBuf::from("/repo"));
        let _refresh = view.begin_load(PathBuf::from("/repo"), RootLoadKind::Refresh);

        assert!(view.apply_recent_write_result(
            write,
            Err("delayed recent write failed".into()),
            cx,
        ));
    });

    assert!(cx.update(|_, app| {
        view.read(app)
            .inline_error()
            .is_some_and(|error| error.contains("delayed recent write failed"))
    }));
}

#[gpui::test]
fn only_newest_recent_write_controls_error_presentation(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |view, _window, cx| {
        let first = view.begin_recent_write(PathBuf::from("/first"));
        let second = view.begin_recent_write(PathBuf::from("/second"));
        assert!(view.apply_recent_write_result(second, Err("newest write failed".into()), cx,));
        assert!(!view.apply_recent_write_result(first, Ok(()), cx));
        assert!(
            view.inline_error()
                .is_some_and(|error| error.contains("newest write failed"))
        );

        let third = view.begin_recent_write(PathBuf::from("/third"));
        assert!(view.apply_recent_write_result(third, Ok(()), cx));
        assert_eq!(view.inline_error(), None);
    });
}

#[gpui::test]
fn every_accepted_open_attempts_a_locked_recent_write(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), Arc::clone(&recents));
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/first")), services, window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |view, window, cx| {
        view.open_repository(PathBuf::from("/second"), window, cx);
    });
    cx.run_until_parked();

    assert_eq!(recents.record_attempts.load(Ordering::SeqCst), 2);
}

#[gpui::test]
fn accepted_recent_writes_are_serialized_in_fifo_order(cx: &mut TestAppContext) {
    struct BlockingOrderedRecents {
        paths: Mutex<Vec<PathBuf>>,
        call_order: Mutex<Vec<PathBuf>>,
        first_started: mpsc::Sender<()>,
        release_first: Mutex<mpsc::Receiver<()>>,
    }

    impl RecentRepositoryStore for BlockingOrderedRecents {
        fn load(&self) -> Result<Vec<PathBuf>, String> {
            Ok(self.paths.lock().unwrap().clone())
        }

        fn record(&self, path: &Path) -> Result<(), String> {
            self.call_order.lock().unwrap().push(path.to_path_buf());
            if path == Path::new("/repo-a") {
                self.first_started.send(()).unwrap();
                self.release_first.lock().unwrap().recv().unwrap();
            }
            let mut paths = self.paths.lock().unwrap();
            paths.retain(|existing| existing != path);
            paths.insert(0, path.to_path_buf());
            Ok(())
        }
    }

    let (first_started_tx, first_started_rx) = mpsc::channel();
    let (release_first_tx, release_first_rx) = mpsc::channel();
    let recents = Arc::new(BlockingOrderedRecents {
        paths: Mutex::new(Vec::new()),
        call_order: Mutex::new(Vec::new()),
        first_started: first_started_tx,
        release_first: Mutex::new(release_first_rx),
    });
    let services = AppServices::new(
        Arc::new(FixtureLoader {
            result: Ok(snapshot("/repo")),
        }),
        Rc::new(FixturePicker::new(Ok(None))),
        recents.clone(),
    );
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));
    cx.run_until_parked();

    view.update_in(cx, |view, window, cx| {
        let first = view.begin_load(PathBuf::from("/repo-a"), RootLoadKind::Open);
        assert!(view.apply_load_result(first, Ok(snapshot("/repo-a")), cx));
        view.enqueue_recent_write(PathBuf::from("/repo-a"), window, cx);

        let second = view.begin_load(PathBuf::from("/repo-b"), RootLoadKind::Open);
        assert!(view.apply_load_result(second, Ok(snapshot("/repo-b")), cx));
        view.enqueue_recent_write(PathBuf::from("/repo-b"), window, cx);
    });

    let recents_for_release = Arc::clone(&recents);
    let release = thread::spawn(move || {
        first_started_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first recent write did not start");
        assert_eq!(
            recents_for_release.call_order.lock().unwrap().as_slice(),
            &[PathBuf::from("/repo-a")]
        );
        release_first_tx.send(()).unwrap();
    });
    cx.run_until_parked();
    release.join().unwrap();

    assert_eq!(
        recents.call_order.lock().unwrap().as_slice(),
        &[PathBuf::from("/repo-a"), PathBuf::from("/repo-b")]
    );
    assert_eq!(
        recents.paths.lock().unwrap().as_slice(),
        &[PathBuf::from("/repo-b"), PathBuf::from("/repo-a")]
    );
    assert_eq!(
        cx.update(|_, app| view.read(app).recent_repositories().to_vec()),
        vec![PathBuf::from("/repo-b"), PathBuf::from("/repo-a")]
    );
}

#[gpui::test]
fn arrow_keys_move_selection_without_changing_the_checked_out_branch(cx: &mut TestAppContext) {
    cx.update(super::init);
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("up up up");

    let (selection, current) = cx.update(|_, app| {
        let workspace = view.read(app).workspace().unwrap();
        (
            workspace.state().selected_branch().map(str::to_string),
            workspace.state().snapshot().current_branch.clone(),
        )
    });
    assert_eq!(selection.as_deref(), Some("feature-b"));
    assert_eq!(current, "feature-a");
}

#[gpui::test]
fn keyboard_workspace_controls_activate_once_and_skip_disabled_controls(cx: &mut TestAppContext) {
    struct CountingLoader {
        calls: Arc<AtomicUsize>,
    }

    impl SnapshotLoader for CountingLoader {
        fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(snapshot("/repo"))
        }
    }

    cx.update(super::init);
    let load_calls = Arc::new(AtomicUsize::new(0));
    let picker_calls = Arc::new(AtomicUsize::new(0));
    let services = AppServices::with_hydration(
        Arc::new(CountingLoader {
            calls: Arc::clone(&load_calls),
        }),
        Rc::new(FixturePicker::with_calls(
            Ok(None),
            Arc::clone(&picker_calls),
        )),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(load_calls.load(Ordering::SeqCst), 1);

    view.update_in(cx, |view, _window, cx| {
        let _ = view.begin_load(PathBuf::from("/repo"), RootLoadKind::Refresh);
        cx.notify();
    });
    cx.update(|window, _| {
        window.focus_next();
        window.focus_next();
        window.focus_next();
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(load_calls.load(Ordering::SeqCst), 1);
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    assert_eq!(load_calls.load(Ordering::SeqCst), 1);
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    assert_eq!(load_calls.load(Ordering::SeqCst), 1);
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn disabled_inspector_control_label_keeps_the_reason_visible() {
    let availability = ActionAvailability {
        enabled: false,
        reason: Some("Already checked out".into()),
    };

    assert_eq!(
        inspector_pane::control_label("Checkout", &availability),
        "Checkout — Already checked out"
    );
}

#[gpui::test]
fn keyboard_refresh_activates_once_for_enter_and_space(cx: &mut TestAppContext) {
    struct CountingLoader {
        calls: Arc<AtomicUsize>,
    }

    impl SnapshotLoader for CountingLoader {
        fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(snapshot("/repo"))
        }
    }

    cx.update(super::init);
    let calls = Arc::new(AtomicUsize::new(0));
    let picker_calls = Arc::new(AtomicUsize::new(0));
    let services = AppServices::with_hydration(
        Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
        }),
        Rc::new(FixturePicker::with_calls(
            Ok(None),
            Arc::clone(&picker_calls),
        )),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
    );
    let (_view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    cx.update(|window, _| {
        window.focus_next();
        window.focus_next();
        window.focus_next();
    });
    cx.simulate_keystrokes("enter");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);

    cx.simulate_keystrokes("space");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: true,
    });
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    cx.update(|window, _| window.focus_next());
    cx.simulate_keystrokes("enter");
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 0);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);

    cx.simulate_keystrokes("space");
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: true,
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 1);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    assert_eq!(picker_calls.load(Ordering::SeqCst), 2);
}

#[gpui::test]
fn keyboard_stack_row_activates_once_for_enter_and_space(cx: &mut TestAppContext) {
    cx.update(super::init);
    let services = services(
        Ok(snapshot("/repo")),
        Ok(None),
        Arc::new(FixtureRecents::default()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    let initial_generation =
        cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());

    cx.update(|window, _| {
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
        window.focus_next();
    });
    cx.simulate_keystrokes("enter");
    let generation = cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());
    assert_eq!(generation, initial_generation);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
        is_held: true,
    });
    let generation = cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());
    assert_eq!(generation, initial_generation);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("enter").unwrap(),
    });
    let (selection, generation) = cx.update(|_, app| {
        let state = view.read(app).workspace().unwrap().state();
        (
            state.selected_branch().map(str::to_string),
            state.generation(),
        )
    });
    assert_eq!(selection.as_deref(), Some("feature-b"));
    assert_eq!(generation, initial_generation + 2);

    cx.simulate_keystrokes("space");
    let generation = cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());
    assert_eq!(generation, initial_generation + 2);
    cx.simulate_event(KeyDownEvent {
        keystroke: Keystroke::parse("space").unwrap(),
        is_held: true,
    });
    let generation = cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());
    assert_eq!(generation, initial_generation + 2);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse("space").unwrap(),
    });
    let generation = cx.update(|_, app| view.read(app).workspace().unwrap().state().generation());
    assert_eq!(generation, initial_generation + 4);
}

#[gpui::test]
fn stale_root_repository_results_are_rejected(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents::default());
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    let (first, second) = view.update_in(cx, |view, _window, _cx| {
        let first = view.begin_load(PathBuf::from("/first"), RootLoadKind::Open);
        let second = view.begin_load(PathBuf::from("/second"), RootLoadKind::Open);
        (first, second)
    });
    view.update_in(cx, |view, _window, cx| {
        assert!(!view.apply_load_result(first, Ok(snapshot("/first")), cx));
        assert!(view.apply_load_result(second, Ok(snapshot("/second")), cx));
    });

    assert_eq!(
        cx.update(|_, app| view.read(app).mode_kind()),
        AppModeKind::Workspace
    );
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .unwrap()
                .state()
                .snapshot()
                .repository_root
                .clone()
        }),
        PathBuf::from("/second")
    );
}

#[gpui::test]
fn repeated_refresh_requests_spawn_only_one_snapshot_read(cx: &mut TestAppContext) {
    struct CountingLoader {
        calls: Arc<AtomicUsize>,
    }

    impl SnapshotLoader for CountingLoader {
        fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(snapshot("/repo"))
        }
    }

    let calls = Arc::new(AtomicUsize::new(0));
    let services = AppServices::with_hydration(
        Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
        }),
        Rc::new(FixturePicker::new(Ok(None))),
        Arc::new(FixtureRecents::default()),
        Arc::new(FixtureHydration::immediate_no_remote()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    view.update_in(cx, |view, window, cx| {
        view.refresh_repository(window, cx);
        view.refresh_repository(window, cx);
        assert!(view.workspace().unwrap().refresh_is_loading());
    });
    cx.run_until_parked();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!cx.update(|_, app| { view.read(app).workspace().unwrap().refresh_is_loading() }));
}

#[gpui::test]
fn pane_preferences_are_loaded_for_the_opened_repository(cx: &mut TestAppContext) {
    let preferences = Arc::new(FixtureWorkspacePreferences::with(
        "/repo",
        WorkspacePreferences {
            visibility: PaneVisibility {
                stack: true,
                changes: false,
                inspector: true,
            },
            widths: PaneWidths {
                stack: 0.4,
                changes: 0.2,
                inspector: 0.4,
            },
        },
    ));
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        preferences as Arc<dyn WorkspacePreferenceStore>,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    let loaded = cx.update(|_, app| view.read(app).workspace().unwrap().preferences().clone());
    assert!(!loaded.visibility.changes);
    assert!(cx.debug_bounds(stack_pane::PANE_MARKER).is_some());
    assert!(cx.debug_bounds(changes_pane::PANE_MARKER).is_none());
    assert!(cx.debug_bounds(inspector_pane::PANE_MARKER).is_some());
}

#[gpui::test]
fn pane_invalid_loaded_preferences_fall_back_without_blocking_open(cx: &mut TestAppContext) {
    let preferences = Arc::new(FixtureWorkspacePreferences::with(
        "/repo",
        WorkspacePreferences {
            visibility: PaneVisibility {
                stack: false,
                changes: false,
                inspector: false,
            },
            widths: PaneWidths {
                stack: f32::NAN,
                changes: 0.0,
                inspector: 5.0,
            },
        },
    ));
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        preferences as Arc<dyn WorkspacePreferenceStore>,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });

    cx.run_until_parked();

    assert_eq!(
        cx.update(|_, app| view.read(app).workspace().unwrap().preferences().clone()),
        WorkspacePreferences::default()
    );
}

#[gpui::test]
fn pane_toggle_persists_and_refuses_to_hide_the_last_pane(cx: &mut TestAppContext) {
    cx.update(super::init);
    let preferences = Arc::new(FixtureWorkspacePreferences::with(
        "/repo",
        WorkspacePreferences {
            visibility: PaneVisibility {
                stack: true,
                changes: false,
                inspector: false,
            },
            ..WorkspacePreferences::default()
        },
    ));
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::clone(&preferences) as Arc<dyn WorkspacePreferenceStore>,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("1");
    assert!(cx.update(|_, app| {
        view.read(app)
            .workspace()
            .unwrap()
            .preferences()
            .visibility
            .stack
    }));
    assert!(preferences.saved().is_empty());

    cx.simulate_keystrokes("2");
    assert!(cx.update(|_, app| {
        view.read(app)
            .workspace()
            .unwrap()
            .preferences()
            .visibility
            .changes
    }));
    assert_eq!(preferences.saved().len(), 1);
    assert!(cx.debug_bounds(changes_pane::PANE_MARKER).is_some());
}

#[gpui::test]
fn pane_divider_resize_clamps_and_persists(cx: &mut TestAppContext) {
    use super::workspace::PaneDivider;

    let preferences = Arc::new(FixtureWorkspacePreferences::default());
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::clone(&preferences) as Arc<dyn WorkspacePreferenceStore>,
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |view, _window, cx| {
        assert!(view.resize_panes(PaneDivider::StackChanges, 1.0, true, cx));
    });

    let widths = cx.update(|_, app| view.read(app).workspace().unwrap().preferences().widths);
    assert!((widths.stack - 0.57).abs() < 0.001);
    assert!((widths.changes - 0.15).abs() < 0.001);
    assert_eq!(preferences.saved().len(), 1);
}

#[gpui::test]
fn pane_divider_drag_persists_on_pointer_release(cx: &mut TestAppContext) {
    let preferences = Arc::new(FixtureWorkspacePreferences::default());
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::clone(&preferences) as Arc<dyn WorkspacePreferenceStore>,
    );
    let (_view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    let divider = cx
        .debug_bounds("pane-divider-stack-changes")
        .expect("stack/changes divider was not rendered")
        .center();
    let moved = point(divider.x + px(180.0), divider.y);
    cx.simulate_mouse_move(divider, None, Modifiers::default());
    cx.simulate_mouse_down(divider, MouseButton::Left, Modifiers::default());
    cx.simulate_mouse_move(moved, Some(MouseButton::Left), Modifiers::default());
    assert!(preferences.saved().is_empty());
    cx.simulate_mouse_up(moved, MouseButton::Left, Modifiers::default());
    assert_eq!(preferences.saved().len(), 1);
}

#[gpui::test]
fn pane_search_filters_rows_and_escape_restores_selection(cx: &mut TestAppContext) {
    cx.update(super::init);
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::new(FixtureWorkspacePreferences::default()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    view.update_in(cx, |view, _window, _cx| {
        assert!(view.workspace_mut().unwrap().select_branch("feature-b"));
    });

    cx.simulate_keystrokes("/");
    cx.simulate_input("FEATURE-B");
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .unwrap()
                .state()
                .search_query()
                .to_string()
        }),
        "FEATURE-B"
    );
    assert_eq!(
        cx.update(|_, app| {
            view.read(app)
                .workspace()
                .unwrap()
                .state()
                .filtered_branches()
                .iter()
                .map(|branch| branch.name.clone())
                .collect::<Vec<_>>()
        }),
        vec!["feature-b".to_string()]
    );

    cx.simulate_keystrokes("escape");
    assert_eq!(
        cx.update(|_, app| {
            let state = view.read(app).workspace().unwrap().state();
            (
                state.search_query().to_string(),
                state.selected_branch().map(str::to_string),
            )
        }),
        (String::new(), Some("feature-b".to_string()))
    );
}

#[gpui::test]
fn pane_search_no_match_and_text_editing_take_priority_over_shortcuts(cx: &mut TestAppContext) {
    cx.update(super::init);
    let services = services_with_workspace_preferences(
        Ok(snapshot("/repo")),
        Arc::new(FixtureWorkspacePreferences::default()),
    );
    let (view, cx) = cx.add_window_view(|window, cx| {
        AppView::new(Some(PathBuf::from("/repo")), services, window, cx)
    });
    cx.run_until_parked();

    cx.simulate_keystrokes("/");
    cx.simulate_input("nobody");

    assert!(cx.debug_bounds("stack-search-no-results").is_some());
    assert_eq!(
        cx.update(|_, app| {
            let app = view.read(app);
            (
                app.workspace().unwrap().state().search_query().to_string(),
                app.operation_overlay().is_some(),
            )
        }),
        ("nobody".to_string(), false)
    );
}

#[test]
fn menu_models_reuse_workspace_actions_and_cover_the_main_command_set() {
    let menus = crate::native_menus();
    assert_eq!(
        menus
            .iter()
            .map(|menu| menu.name.as_ref())
            .collect::<Vec<_>>(),
        vec!["Stax", "File", "Edit", "View", "Branch", "Stack"]
    );

    let actions = menus
        .iter()
        .flat_map(|menu| menu.items.iter())
        .filter_map(|item| match item {
            MenuItem::Action { name, action, .. } => {
                Some((name.to_string(), action.name().to_string()))
            }
            MenuItem::Separator | MenuItem::Submenu(_) | MenuItem::SystemMenu(_) => None,
        })
        .collect::<Vec<_>>();
    for (label, action_suffix) in [
        ("Open Repository…", "OpenRepository"),
        ("Refresh Repository", "RefreshRepository"),
        ("Undo", "UndoLatest"),
        ("Redo", "RedoLatest"),
        ("Search Stack", "FocusStackSearch"),
        ("Show/Hide Stack", "ToggleStackPane"),
        ("Show/Hide Changes", "ToggleChangesPane"),
        ("Show/Hide Inspector", "ToggleInspectorPane"),
        ("Checkout Selected", "CheckoutSelected"),
        ("Create Branch…", "CreateBranch"),
        ("Rename Branch…", "RenameSelected"),
        ("Delete Branch…", "DeleteSelected"),
        ("Move Branch…", "MoveSelected"),
        ("Reorder Stack…", "ReorderSelectedStack"),
        ("Restack Selected", "RestackSelected"),
        ("Restack All", "RestackAll"),
        ("Submit Stack…", "SubmitStack"),
        ("Open Pull Request", "OpenPullRequest"),
    ] {
        assert!(
            actions
                .iter()
                .any(|(name, action)| name == label && action.ends_with(action_suffix)),
            "missing {label} backed by {action_suffix}"
        );
    }
}
