#[cfg(test)]
use super::{changes_pane, inspector_pane, stack_pane};
use super::{welcome::WelcomeView, workspace::WorkspaceView};
use crate::hydration::{
    BranchHydrationService, CiHydrationRequest, DetailsHydrationRequest, DiffHydrationRequest,
    HydrationCoordinator, NativeBranchHydrationService,
};
use crate::preferences::RecentRepositories;
use crate::state::SelectionDirection;
use crate::theme::{SYSTEM_UI_FONT, Theme};
use gpui::{
    App, Context, Div, ElementId, FocusHandle, Focusable, InteractiveElement as _, IntoElement,
    KeyBinding, ParentElement as _, PathPromptOptions, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Window, actions, div, px,
};
use stax::application::{BranchDiff, DetailRequestToken, RepositorySession, RepositorySnapshot};
use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

actions!(
    stax_gui,
    [
        SelectPreviousBranch,
        SelectNextBranch,
        OpenRepository,
        RefreshRepository
    ]
);

pub type PickerFuture = Pin<Box<dyn Future<Output = Result<Option<PathBuf>, String>> + 'static>>;

pub trait SnapshotLoader: Send + Sync {
    fn load(&self, path: &Path) -> Result<RepositorySnapshot, String>;
}

pub trait RepositoryPicker {
    fn pick(&self, cx: &mut App) -> PickerFuture;
}

pub trait RecentRepositoryStore: Send + Sync {
    fn load(&self) -> Result<Vec<PathBuf>, String>;
    fn record(&self, path: &Path) -> Result<(), String>;
}

#[derive(Clone)]
pub struct AppServices {
    loader: Arc<dyn SnapshotLoader>,
    picker: Rc<dyn RepositoryPicker>,
    recents: Arc<dyn RecentRepositoryStore>,
    hydration: Arc<dyn BranchHydrationService>,
}

impl AppServices {
    pub fn new(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
    ) -> Self {
        Self::with_hydration(
            loader,
            picker,
            recents,
            Arc::new(NativeBranchHydrationService),
        )
    }

    pub(super) fn with_hydration(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
        hydration: Arc<dyn BranchHydrationService>,
    ) -> Self {
        Self {
            loader,
            picker,
            recents,
            hydration,
        }
    }

    pub(super) fn native() -> Self {
        Self::new(
            Arc::new(NativeSnapshotLoader),
            Rc::new(NativeRepositoryPicker),
            Arc::new(RecentRepositories::default()),
        )
    }
}

struct NativeSnapshotLoader;

impl SnapshotLoader for NativeSnapshotLoader {
    fn load(&self, path: &Path) -> Result<RepositorySnapshot, String> {
        RepositorySession::open(path)
            .and_then(|session| session.snapshot())
            .map_err(|error| error.to_string())
    }
}

struct NativeRepositoryPicker;

impl RepositoryPicker for NativeRepositoryPicker {
    fn pick(&self, cx: &mut App) -> PickerFuture {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Open Repository".into()),
        });
        Box::pin(async move {
            receiver
                .await
                .map_err(|_| "Open Repository dialog closed unexpectedly".to_string())?
                .map_err(|error| format!("Open Repository dialog failed: {error}"))
                .map(|paths| paths.and_then(|paths| paths.into_iter().next()))
        })
    }
}

impl RecentRepositoryStore for RecentRepositories {
    fn load(&self) -> Result<Vec<PathBuf>, String> {
        RecentRepositories::load(self).map_err(|error| error.to_string())
    }

    fn record(&self, path: &Path) -> Result<(), String> {
        RecentRepositories::record(self, path).map_err(|error| error.to_string())
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppModeKind {
    Welcome,
    Opening,
    Workspace,
    Error,
}

enum AppMode {
    Welcome(WelcomeView),
    Opening(WelcomeView),
    Workspace(Box<WorkspaceView>),
    Error(WelcomeView),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WelcomeModeKind {
    Welcome,
    Opening,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootLoadKind {
    Open,
    Refresh,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootLoadToken {
    generation: u64,
    path: PathBuf,
    kind: RootLoadKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecentLoadToken {
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentWriteToken {
    generation: u64,
    path: PathBuf,
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneMarkers {
    pub stack: &'static str,
    pub changes: &'static str,
    pub inspector: &'static str,
}

#[cfg(test)]
impl PaneMarkers {
    pub const fn all() -> Self {
        Self {
            stack: stack_pane::PANE_MARKER,
            changes: changes_pane::PANE_MARKER,
            inspector: inspector_pane::PANE_MARKER,
        }
    }
}

pub struct AppView {
    mode: AppMode,
    focus_handle: FocusHandle,
    services: AppServices,
    recent_repositories: Vec<PathBuf>,
    action_error: Option<String>,
    recent_load_error: Option<String>,
    recent_write_error: Option<String>,
    recent_load_pending: bool,
    recent_load_generation: u64,
    recent_write_generation: u64,
    recent_write_queue: VecDeque<RecentWriteToken>,
    recent_write_in_flight: bool,
    load_generation: u64,
    hydration_coordinator: HydrationCoordinator,
}

impl AppView {
    pub fn new(
        repository: Option<PathBuf>,
        services: AppServices,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus_handle = cx.focus_handle().tab_index(0).tab_stop(true);
        focus_handle.focus(window);
        window.set_window_title("Stax");

        let mut view = Self {
            mode: AppMode::Welcome(WelcomeView::new(Vec::new(), None, None, false)),
            focus_handle,
            services,
            recent_repositories: Vec::new(),
            action_error: None,
            recent_load_error: None,
            recent_write_error: None,
            recent_load_pending: false,
            recent_load_generation: 0,
            recent_write_generation: 0,
            recent_write_queue: VecDeque::new(),
            recent_write_in_flight: false,
            load_generation: 0,
            hydration_coordinator: HydrationCoordinator::default(),
        };
        view.mode = AppMode::Welcome(view.welcome(None, None));
        view.load_recent_repositories(window, cx);
        if let Some(repository) = repository {
            view.open_repository(repository, window, cx);
        }
        view
    }

    #[cfg(test)]
    pub fn mode_kind(&self) -> AppModeKind {
        match self.mode {
            AppMode::Welcome(_) => AppModeKind::Welcome,
            AppMode::Opening(_) => AppModeKind::Opening,
            AppMode::Workspace(_) => AppModeKind::Workspace,
            AppMode::Error(_) => AppModeKind::Error,
        }
    }

    #[cfg(test)]
    pub fn inline_error(&self) -> Option<&str> {
        match &self.mode {
            AppMode::Welcome(welcome) | AppMode::Opening(welcome) | AppMode::Error(welcome) => {
                welcome.error()
            }
            AppMode::Workspace(workspace) => workspace.inline_error(),
        }
    }

    #[cfg(test)]
    pub fn recent_load_is_pending(&self) -> bool {
        self.recent_load_pending
    }

    #[cfg(test)]
    pub fn recent_repositories(&self) -> &[PathBuf] {
        &self.recent_repositories
    }

    pub fn workspace(&self) -> Option<&WorkspaceView> {
        match &self.mode {
            AppMode::Workspace(workspace) => Some(workspace),
            AppMode::Welcome(_) | AppMode::Opening(_) | AppMode::Error(_) => None,
        }
    }

    fn workspace_mut(&mut self) -> Option<&mut WorkspaceView> {
        match &mut self.mode {
            AppMode::Workspace(workspace) => Some(workspace),
            AppMode::Welcome(_) | AppMode::Opening(_) | AppMode::Error(_) => None,
        }
    }

    #[cfg(test)]
    pub fn pane_markers(&self) -> Option<PaneMarkers> {
        self.workspace().map(WorkspaceView::pane_markers)
    }

    pub fn pick_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picker = Rc::clone(&self.services.picker);
        let future = picker.pick(cx);
        cx.spawn_in(window, async move |this, cx| {
            let result = future.await;
            let _ = this.update_in(cx, |view, window, cx| match result {
                Ok(Some(path)) => view.open_repository(path, window, cx),
                Ok(None) => {}
                Err(error) => {
                    view.set_inline_error(error);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn open_repository(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.start_load(path, RootLoadKind::Open, window, cx);
    }

    pub fn refresh_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .workspace()
            .is_some_and(WorkspaceView::refresh_is_loading)
        {
            return;
        }
        let Some(path) = self
            .workspace()
            .map(|workspace| workspace.state().snapshot().repository_root.clone())
        else {
            return;
        };
        self.start_load(path, RootLoadKind::Refresh, window, cx);
    }

    pub fn select_branch(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.select_branch(name))
        {
            self.hydrate_selection(window, cx);
            cx.notify();
        }
    }

    pub(super) fn hydrate_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((token, branch)) = self
            .workspace_mut()
            .and_then(WorkspaceView::begin_hydration)
        else {
            return;
        };
        self.hydration_coordinator.clear_queued_ci();
        let details_request = self
            .hydration_coordinator
            .enqueue_details(token.clone(), branch.clone());
        let diff_request = self
            .hydration_coordinator
            .enqueue_diff(token.clone(), branch.parent.clone());
        if branch.parent.is_none()
            && let Some(workspace) = self.workspace_mut()
        {
            workspace.apply_diff(
                token.clone(),
                Ok(BranchDiff {
                    stat: Vec::new(),
                    lines: Vec::new(),
                }),
            );
        }
        if let Some(request) = details_request {
            self.start_details_hydration(request, window, cx);
        }
        if let Some(request) = diff_request {
            self.start_diff_hydration(request, window, cx);
        }
        cx.notify();
    }

    fn start_details_hydration(
        &mut self,
        request: DetailsHydrationRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hydration = Arc::clone(&self.services.hydration);
        let background = cx.background_executor().clone();
        let token = request.token;
        let future = hydration.load_details(token.repository.clone(), request.branch);
        cx.spawn_in(window, async move |this, cx| {
            let result = background.spawn(future).await;
            let _ = this.update_in(cx, |view, window, cx| {
                let follow_up = match &result {
                    Ok(details) if details.has_remote => Ok(true),
                    Ok(_) => Ok(false),
                    Err(error) => Err(format!(
                        "CI requires branch details: {error}. Fix the repository configuration, then refresh."
                    )),
                };
                let accepted = view.workspace_mut().is_some_and(|workspace| {
                    workspace.apply_details(token.clone(), result)
                });
                if accepted {
                    match follow_up {
                        Ok(true) => view.queue_ci_hydration(token, window, cx),
                        Ok(false) => {
                            if let Some(workspace) = view.workspace_mut() {
                                workspace.apply_ci(
                                    token,
                                    Err(
                                        "Push branch to see remote checks. Push the branch, then refresh."
                                            .to_string(),
                                    ),
                                );
                            }
                        }
                        Err(error) => {
                            if let Some(workspace) = view.workspace_mut() {
                                workspace.apply_ci(token, Err(error));
                            }
                        }
                    }
                }
                if let Some(next) = view.hydration_coordinator.finish_details() {
                    view.start_details_hydration(next, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn start_diff_hydration(
        &mut self,
        request: DiffHydrationRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = request.parent else {
            if let Some(next) = self.hydration_coordinator.finish_diff() {
                self.start_diff_hydration(next, window, cx);
            }
            return;
        };
        let hydration = Arc::clone(&self.services.hydration);
        let background = cx.background_executor().clone();
        let token = request.token;
        let repository = token.repository.clone();
        let branch = token.branch.clone();
        let cache_future =
            hydration.load_cached_diff(repository.clone(), branch.clone(), parent.clone());
        cx.spawn_in(window, async move |this, cx| {
            let cached = background.spawn(cache_future).await;
            let continue_fresh = this
                .update_in(cx, |view, window, cx| {
                    let applied = cached.ok().flatten().is_some_and(|diff| {
                        view.workspace_mut().is_some_and(|workspace| {
                            workspace.apply_cached_diff(token.clone(), diff)
                        })
                    });
                    let superseded = view.hydration_coordinator.diff_has_queued();
                    if superseded && let Some(next) = view.hydration_coordinator.finish_diff() {
                        view.start_diff_hydration(next, window, cx);
                    }
                    if applied || superseded {
                        cx.notify();
                    }
                    !superseded
                })
                .unwrap_or(false);
            if !continue_fresh {
                return;
            }

            let fresh_future = hydration.load_diff(repository, branch, parent);
            let result = background.spawn(fresh_future).await;
            let _ = this.update_in(cx, |view, window, cx| {
                let applied = view
                    .workspace_mut()
                    .is_some_and(|workspace| workspace.apply_diff(token, result));
                if let Some(next) = view.hydration_coordinator.finish_diff() {
                    view.start_diff_hydration(next, window, cx);
                }
                if applied {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn queue_ci_hydration(
        &mut self,
        token: DetailRequestToken,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(request) = self.hydration_coordinator.enqueue_ci(token) {
            self.start_ci_hydration(request, window, cx);
        }
    }

    fn start_ci_hydration(
        &mut self,
        request: CiHydrationRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hydration = Arc::clone(&self.services.hydration);
        let background = cx.background_executor().clone();
        let token = request.token;
        let repository = token.repository.clone();
        let branch = token.branch.clone();
        let future = hydration.load_ci(repository, branch);
        cx.spawn_in(window, async move |this, cx| {
            let result = background.spawn(future).await;
            let _ = this.update_in(cx, |view, window, cx| {
                let applied = view
                    .workspace_mut()
                    .is_some_and(|workspace| workspace.apply_ci(token, result));
                if let Some(next) = view.hydration_coordinator.finish_ci() {
                    view.start_ci_hydration(next, window, cx);
                }
                if applied {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub fn begin_load(&mut self, path: PathBuf, kind: RootLoadKind) -> RootLoadToken {
        self.load_generation = self
            .load_generation
            .checked_add(1)
            .expect("root repository load generation exhausted");
        match kind {
            RootLoadKind::Open => {
                self.action_error = None;
                self.mode = AppMode::Opening(self.welcome(None, Some(path.clone())));
            }
            RootLoadKind::Refresh => {
                if let Some(workspace) = self.workspace_mut() {
                    workspace.begin_refresh();
                }
            }
        }
        RootLoadToken {
            generation: self.load_generation,
            path,
            kind,
        }
    }

    pub fn apply_load_result(
        &mut self,
        token: RootLoadToken,
        result: Result<RepositorySnapshot, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if token.generation != self.load_generation {
            return false;
        }

        match token.kind {
            RootLoadKind::Open => match result {
                Ok(snapshot) => {
                    self.remember_recent(&snapshot.repository_root);
                    self.action_error = None;
                    self.mode =
                        AppMode::Workspace(Box::new(WorkspaceView::from_snapshot(snapshot)));
                    self.sync_storage_notice();
                }
                Err(error) => {
                    self.action_error = Some(error.clone());
                    self.mode = AppMode::Error(self.welcome(Some(error), None));
                }
            },
            RootLoadKind::Refresh => {
                let Some(workspace) = self.workspace_mut() else {
                    return false;
                };
                if workspace.state().snapshot().repository_root != token.path {
                    return false;
                }
                match result {
                    Ok(snapshot) => workspace.apply_snapshot(snapshot),
                    Err(error) => workspace.fail_refresh(error),
                }
            }
        }
        cx.notify();
        true
    }

    fn start_load(
        &mut self,
        path: PathBuf,
        kind: RootLoadKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let token = self.begin_load(path.clone(), kind);
        let loader = Arc::clone(&self.services.loader);
        let background = cx.background_executor().clone();
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = background.spawn(async move { loader.load(&path) }).await;
            let should_hydrate = result.is_ok();
            let recent_path = result
                .as_ref()
                .ok()
                .map(|snapshot| snapshot.repository_root.clone());
            let _ = this.update_in(cx, |view, window, cx| {
                let accepted = view.apply_load_result(token.clone(), result, cx);
                if accepted
                    && token.kind == RootLoadKind::Open
                    && let Some(path) = recent_path
                {
                    view.enqueue_recent_write(path, window, cx);
                }
                if accepted && should_hydrate {
                    view.hydrate_selection(window, cx);
                }
            });
        })
        .detach();
    }

    fn load_recent_repositories(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let token = self.begin_recent_load();
        let recents = Arc::clone(&self.services.recents);
        let background = cx.background_executor().clone();
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let result = background.spawn(async move { recents.load() }).await;
            let _ = this.update_in(cx, |view, _window, cx| {
                view.apply_recent_load_result(token, result, cx);
            });
        })
        .detach();
    }

    pub fn begin_recent_load(&mut self) -> RecentLoadToken {
        self.recent_load_generation = self
            .recent_load_generation
            .checked_add(1)
            .expect("recent repository load generation exhausted");
        self.recent_load_pending = true;
        self.sync_welcome_mode();
        RecentLoadToken {
            generation: self.recent_load_generation,
        }
    }

    pub fn apply_recent_load_result(
        &mut self,
        token: RecentLoadToken,
        result: Result<Vec<PathBuf>, String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if token.generation != self.recent_load_generation {
            return false;
        }

        self.recent_load_pending = false;
        match result {
            Ok(repositories) => {
                self.recent_load_error = None;
                for repository in repositories {
                    if !self.recent_repositories.contains(&repository) {
                        self.recent_repositories.push(repository);
                    }
                }
                self.recent_repositories.truncate(10);
            }
            Err(error) => self.recent_load_error = Some(error),
        }
        self.sync_storage_notice();
        self.sync_welcome_mode();
        cx.notify();
        true
    }

    pub fn begin_recent_write(&mut self, path: PathBuf) -> RecentWriteToken {
        self.recent_write_generation = self
            .recent_write_generation
            .checked_add(1)
            .expect("recent repository write generation exhausted");
        RecentWriteToken {
            generation: self.recent_write_generation,
            path,
        }
    }

    pub fn apply_recent_write_result(
        &mut self,
        token: RecentWriteToken,
        result: Result<(), String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if token.generation != self.recent_write_generation {
            return false;
        }

        self.recent_write_error = result.err().map(|error| {
            format!(
                "Repository opened, but its recent entry was not saved for {}: {error}",
                token.path.display()
            )
        });
        self.sync_storage_notice();
        self.sync_welcome_mode();
        cx.notify();
        true
    }

    pub(crate) fn enqueue_recent_write(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let token = self.begin_recent_write(path);
        self.recent_write_queue.push_back(token);
        self.start_next_recent_write(window, cx);
    }

    fn start_next_recent_write(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.recent_write_in_flight {
            return;
        }
        let Some(token) = self.recent_write_queue.pop_front() else {
            return;
        };

        self.recent_write_in_flight = true;
        let path = token.path.clone();
        let recents = Arc::clone(&self.services.recents);
        let background = cx.background_executor().clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = background.spawn(async move { recents.record(&path) }).await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.recent_write_in_flight = false;
                view.apply_recent_write_result(token, result, cx);
                view.start_next_recent_write(window, cx);
            });
        })
        .detach();
    }

    fn remember_recent(&mut self, path: &Path) {
        self.recent_repositories
            .retain(|repository| repository != path);
        self.recent_repositories.insert(0, path.to_path_buf());
        self.recent_repositories.truncate(10);
    }

    fn welcome(&self, error: Option<String>, opening: Option<PathBuf>) -> WelcomeView {
        let storage_error = self.storage_error();
        let error = match (storage_error, error.or_else(|| self.action_error.clone())) {
            (Some(storage), Some(action)) => {
                Some(format!("{action}\nRecent repositories: {storage}"))
            }
            (Some(storage), None) => Some(storage),
            (None, action) => action,
        };
        WelcomeView::new(
            self.recent_repositories.clone(),
            error,
            opening,
            self.recent_load_pending,
        )
    }

    fn set_inline_error(&mut self, error: String) {
        match &mut self.mode {
            AppMode::Welcome(welcome) | AppMode::Opening(welcome) | AppMode::Error(welcome) => {
                self.action_error = Some(error.clone());
                welcome.set_error(error);
            }
            AppMode::Workspace(workspace) => workspace.set_notice(error),
        }
    }

    fn storage_error(&self) -> Option<String> {
        match (&self.recent_load_error, &self.recent_write_error) {
            (Some(load), Some(write)) => Some(format!("{load}\n{write}")),
            (Some(load), None) => Some(load.clone()),
            (None, Some(write)) => Some(write.clone()),
            (None, None) => None,
        }
    }

    fn sync_storage_notice(&mut self) {
        let notice = self.storage_error();
        if let Some(workspace) = self.workspace_mut() {
            workspace.set_storage_notice(notice);
        }
    }

    fn sync_welcome_mode(&mut self) {
        let mode = match &self.mode {
            AppMode::Welcome(_) => Some((WelcomeModeKind::Welcome, None)),
            AppMode::Opening(welcome) => Some((
                WelcomeModeKind::Opening,
                welcome.opening_path().map(Path::to_path_buf),
            )),
            AppMode::Error(_) => Some((WelcomeModeKind::Error, None)),
            AppMode::Workspace(_) => None,
        };
        let Some((kind, opening)) = mode else {
            return;
        };
        let welcome = self.welcome(None, opening);
        self.mode = match kind {
            WelcomeModeKind::Welcome => AppMode::Welcome(welcome),
            WelcomeModeKind::Opening => AppMode::Opening(welcome),
            WelcomeModeKind::Error => AppMode::Error(welcome),
        };
    }

    fn select_previous(
        &mut self,
        _: &SelectPreviousBranch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.move_selection(SelectionDirection::Previous))
        {
            self.hydrate_selection(window, cx);
            cx.notify();
        }
    }

    fn select_next(&mut self, _: &SelectNextBranch, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.move_selection(SelectionDirection::Next))
        {
            self.hydrate_selection(window, cx);
            cx.notify();
        }
    }

    fn open_action(&mut self, _: &OpenRepository, window: &mut Window, cx: &mut Context<Self>) {
        self.pick_repository(window, cx);
    }

    fn refresh_action(
        &mut self,
        _: &RefreshRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_repository(window, cx);
    }
}

impl Focusable for AppView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());
        let content = match &self.mode {
            AppMode::Welcome(welcome) | AppMode::Opening(welcome) | AppMode::Error(welcome) => {
                welcome.render(theme, cx)
            }
            AppMode::Workspace(workspace) => workspace.render(theme, cx),
        };

        div()
            .id("stax-app")
            .key_context("StaxApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::select_previous))
            .on_action(cx.listener(Self::select_next))
            .on_action(cx.listener(Self::open_action))
            .on_action(cx.listener(Self::refresh_action))
            .size_full()
            .border_1()
            .border_color(if self.focus_handle.is_focused(window) {
                theme.focus
            } else {
                theme.border
            })
            .font_family(SYSTEM_UI_FONT)
            .bg(theme.window)
            .text_color(theme.text)
            .child(content)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Primary,
    Secondary,
}

pub fn control_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ControlKind,
    enabled: bool,
    theme: Theme,
) -> Stateful<Div> {
    let label = label.into();
    let base = div()
        .id(id)
        .h(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .px_3()
        .rounded_md()
        .border_1()
        .text_xs()
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(label);

    if !enabled {
        return base
            .border_color(theme.border)
            .bg(theme.disabled_surface)
            .text_color(theme.disabled_text);
    }

    let base = base
        .focusable()
        .tab_index(0)
        .cursor_pointer()
        .focus(move |style| style.border_color(theme.focus))
        .active(|style| style.opacity(0.82));
    match kind {
        ControlKind::Primary => base
            .border_color(theme.accent)
            .bg(theme.accent)
            .text_color(theme.accent_text)
            .hover(move |style| style.bg(theme.accent.alpha(0.88))),
        ControlKind::Secondary => base
            .border_color(theme.border_strong)
            .bg(theme.surface_raised)
            .text_color(theme.text)
            .hover(move |style| style.bg(theme.surface_selected)),
    }
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPreviousBranch, Some("StaxApp")),
        KeyBinding::new("down", SelectNextBranch, Some("StaxApp")),
        KeyBinding::new("cmd-o", OpenRepository, Some("StaxApp")),
        KeyBinding::new("cmd-r", RefreshRepository, Some("StaxApp")),
    ]);
}
