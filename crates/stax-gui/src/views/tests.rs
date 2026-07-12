use super::{
    AppModeKind, AppServices, AppView, PaneMarkers, PickerFuture, RecentRepositoryStore,
    RepositoryPicker, RootLoadKind, SnapshotLoader,
};
use gpui::{App, TestAppContext};
use stax::application::{BranchSummary, RepositorySnapshot};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct FixtureLoader {
    result: Result<RepositorySnapshot, String>,
}

impl SnapshotLoader for FixtureLoader {
    fn load(&self, _path: &Path) -> Result<RepositorySnapshot, String> {
        self.result.clone()
    }
}

struct FixturePicker {
    result: Result<Option<PathBuf>, String>,
}

impl RepositoryPicker for FixturePicker {
    fn pick(&self, _cx: &mut App) -> PickerFuture {
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
    AppServices::new(
        Arc::new(FixtureLoader { result: loader }),
        Rc::new(FixturePicker { result: picker }),
        recents,
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
fn recent_repositories_load_after_the_welcome_first_paint(cx: &mut TestAppContext) {
    let recents = Arc::new(FixtureRecents {
        paths: Mutex::new(vec![PathBuf::from("/recent/repo")]),
        ..Default::default()
    });
    let services = services(Ok(snapshot("/repo")), Ok(None), recents);
    let (view, cx) = cx.add_window_view(|window, cx| AppView::new(None, services, window, cx));

    cx.run_until_parked();
    assert_eq!(
        cx.update(|_, app| view.read(app).recent_repositories().to_vec()),
        vec![PathBuf::from("/recent/repo")]
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
        Rc::new(FixturePicker { result: Ok(None) }),
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
    let services = AppServices::new(
        Arc::new(CountingLoader {
            calls: Arc::clone(&calls),
        }),
        Rc::new(FixturePicker { result: Ok(None) }),
        Arc::new(FixtureRecents::default()),
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
