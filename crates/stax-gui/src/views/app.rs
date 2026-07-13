#[cfg(test)]
use super::{changes_pane, inspector_pane, stack_pane};
use super::{operation_overlay, text_input::BranchNameInput};
use super::{
    welcome::WelcomeView,
    workspace::{PaneDivider, PaneKind, WorkspaceView},
};
use crate::hydration::{
    BranchHydrationService, CiHydrationRequest, DetailsHydrationRequest, DiffHydrationRequest,
    HydrationCoordinator, NativeBranchHydrationService,
};
use crate::operation::{
    BrowserService, NativeBrowserService, NativeOperationService, OperationService,
};
#[cfg(test)]
use crate::preferences::TransientWorkspacePreferences;
use crate::preferences::{RecentRepositories, WorkspacePreferenceStore, WorkspacePreferencesFile};
use crate::state::{InteractionState, SelectionDirection};
use crate::theme::{SYSTEM_UI_FONT, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, AppContext as _, ClickEvent, ClipboardItem, Context, Div, Entity, FocusHandle, Focusable,
    InteractiveElement as _, IntoElement, KeyBinding, MouseMoveEvent, MouseUpEvent,
    ParentElement as _, PathPromptOptions, Pixels, Render, SharedString, Stateful,
    StatefulInteractiveElement as _, StyleRefinement, Styled as _, Subscription, Window, actions,
    div, px,
};
use stax::application::{
    BranchDiff, DetailRequestToken, OperationError, OperationErrorDetails, OperationErrorKind,
    OperationEvent, OperationRequest, OperationResult, OperationSideEffects, PullRequestMode,
    RepositorySession, RepositorySnapshot, RestackScope,
};
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
        RefreshRepository,
        CheckoutSelected,
        CreateBranch,
        RenameSelected,
        DeleteSelected,
        MoveSelected,
        ReorderSelectedStack,
        UndoLatest,
        RedoLatest,
        RestackSelected,
        RestackAll,
        SubmitStack,
        OpenPullRequest,
        ConfirmOverlay,
        DismissOverlay,
        DismissOperationBanner,
        OpenReceiptUrl,
        ToggleStackPane,
        ToggleChangesPane,
        ToggleInspectorPane,
        FocusStackSearch,
        ClearStackSearch,
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
    #[allow(dead_code)]
    operation: Arc<dyn OperationService>,
    #[allow(dead_code)]
    browser: Arc<dyn BrowserService>,
    workspace_preferences: Arc<dyn WorkspacePreferenceStore>,
}

impl AppServices {
    pub fn new(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
    ) -> Self {
        Self::with_all_services(
            loader,
            picker,
            recents,
            Arc::new(NativeBranchHydrationService),
            Arc::new(NativeOperationService),
            Arc::new(NativeBrowserService),
            Arc::new(WorkspacePreferencesFile::default()),
        )
    }

    #[cfg(test)]
    pub(super) fn with_hydration(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
        hydration: Arc<dyn BranchHydrationService>,
    ) -> Self {
        Self::with_operation_services(
            loader,
            picker,
            recents,
            hydration,
            Arc::new(NativeOperationService),
            Arc::new(NativeBrowserService),
        )
    }

    #[cfg(test)]
    pub(super) fn with_operation_services(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
        hydration: Arc<dyn BranchHydrationService>,
        operation: Arc<dyn OperationService>,
        browser: Arc<dyn BrowserService>,
    ) -> Self {
        Self::with_all_services(
            loader,
            picker,
            recents,
            hydration,
            operation,
            browser,
            Arc::new(TransientWorkspacePreferences::default()),
        )
    }

    #[cfg(test)]
    pub(super) fn with_workspace_preferences(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
        hydration: Arc<dyn BranchHydrationService>,
        workspace_preferences: Arc<dyn WorkspacePreferenceStore>,
    ) -> Self {
        Self::with_all_services(
            loader,
            picker,
            recents,
            hydration,
            Arc::new(NativeOperationService),
            Arc::new(NativeBrowserService),
            workspace_preferences,
        )
    }

    fn with_all_services(
        loader: Arc<dyn SnapshotLoader>,
        picker: Rc<dyn RepositoryPicker>,
        recents: Arc<dyn RecentRepositoryStore>,
        hydration: Arc<dyn BranchHydrationService>,
        operation: Arc<dyn OperationService>,
        browser: Arc<dyn BrowserService>,
        workspace_preferences: Arc<dyn WorkspacePreferenceStore>,
    ) -> Self {
        Self {
            loader,
            picker,
            recents,
            hydration,
            operation,
            browser,
            workspace_preferences,
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
    operation_overlay: Option<operation_overlay::OperationOverlay>,
    branch_input: Option<Entity<BranchNameInput>>,
    branch_input_text: String,
    branch_input_observation: Option<Subscription>,
    search_input: Option<Entity<BranchNameInput>>,
    search_input_observation: Option<Subscription>,
    overlay_return_focus: Option<FocusHandle>,
    pane_drag: Option<PaneDrag>,
    #[cfg(test)]
    copied_diagnostics: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct PaneDrag {
    divider: PaneDivider,
    last_x: Pixels,
    changed: bool,
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
            operation_overlay: None,
            branch_input: None,
            branch_input_text: String::new(),
            branch_input_observation: None,
            search_input: None,
            search_input_observation: None,
            overlay_return_focus: None,
            pane_drag: None,
            #[cfg(test)]
            copied_diagnostics: None,
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

    pub(crate) fn workspace_mut(&mut self) -> Option<&mut WorkspaceView> {
        match &mut self.mode {
            AppMode::Workspace(workspace) => Some(workspace),
            AppMode::Welcome(_) | AppMode::Opening(_) | AppMode::Error(_) => None,
        }
    }

    fn persist_workspace_preferences(&mut self) {
        let Some(workspace) = self.workspace() else {
            return;
        };
        let repository = workspace.state().snapshot().repository_root.clone();
        let preferences = workspace.preferences().clone();
        if let Err(error) = self
            .services
            .workspace_preferences
            .save(&repository, &preferences)
        {
            self.set_inline_error(format!("Could not save workspace layout: {error}"));
        }
    }

    fn toggle_pane(&mut self, pane: PaneKind, cx: &mut Context<Self>) {
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.toggle_pane(pane))
        {
            self.persist_workspace_preferences();
            cx.notify();
        }
    }

    fn install_search_input(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(BranchNameInput::new_search);
        let observation = cx.observe(&input, |app, input, cx| {
            let query = input.read(cx).text().to_string();
            if let Some(workspace) = app.workspace_mut()
                && workspace.state().search_query() != query
            {
                workspace.set_search_query(query);
            }
            cx.notify();
        });
        self.search_input = Some(input);
        self.search_input_observation = Some(observation);
    }

    fn focus_stack_search_action(
        &mut self,
        _: &FocusStackSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace().is_none() {
            return;
        }
        if self
            .workspace()
            .is_some_and(|workspace| !workspace.preferences().visibility.stack)
        {
            self.toggle_pane(PaneKind::Stack, cx);
        }
        if let Some(input) = &self.search_input {
            input.read(cx).focus_handle().focus(window);
        }
    }

    fn clear_stack_search_action(
        &mut self,
        _: &ClearStackSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(input) = &self.search_input {
            input.update(cx, |input, cx| input.set_text(String::new(), cx));
        } else if let Some(workspace) = self.workspace_mut() {
            workspace.set_search_query(String::new());
        }
        self.focus_handle.focus(window);
        cx.notify();
    }

    pub(super) fn resize_panes(
        &mut self,
        divider: PaneDivider,
        delta: f32,
        persist: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let changed = self
            .workspace_mut()
            .is_some_and(|workspace| workspace.resize_panes(divider, delta));
        if changed {
            if persist {
                self.persist_workspace_preferences();
            }
            cx.notify();
        }
        changed
    }

    pub(super) fn begin_pane_drag(&mut self, divider: PaneDivider, x: Pixels) {
        self.pane_drag = Some(PaneDrag {
            divider,
            last_x: x,
            changed: false,
        });
    }

    fn pane_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.pane_drag else {
            return;
        };
        let width = window.viewport_size().width;
        if width <= px(0.0) {
            return;
        }
        let delta = (event.position.x - drag.last_x) / width;
        drag.last_x = event.position.x;
        drag.changed |= self.resize_panes(drag.divider, delta, false, cx);
        self.pane_drag = Some(drag);
    }

    fn pane_drag_end(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        if self.pane_drag.take().is_some_and(|drag| drag.changed) {
            self.persist_workspace_preferences();
        }
    }

    #[cfg(test)]
    pub fn pane_markers(&self) -> Option<PaneMarkers> {
        self.workspace().map(WorkspaceView::pane_markers)
    }

    #[cfg(test)]
    pub fn completion_transition_count(&self) -> usize {
        self.workspace()
            .map(|workspace| workspace.state().completion_transition_count())
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn operation_progress_log(&self) -> Vec<usize> {
        self.workspace()
            .map(|workspace| workspace.state().operation_progress_log())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub fn snapshot_refresh_count(&self) -> usize {
        self.workspace()
            .map(|workspace| workspace.state().snapshot_refresh_count())
            .unwrap_or(0)
    }

    #[cfg(test)]
    pub fn last_receipt(&self) -> Option<stax::application::OperationReceipt> {
        self.workspace()
            .and_then(|workspace| workspace.state().last_receipt().cloned())
    }

    #[cfg(test)]
    pub fn branch_input_text(&self) -> String {
        self.branch_input_text.clone()
    }

    #[cfg(test)]
    pub fn operation_overlay(&self) -> Option<&operation_overlay::OperationOverlay> {
        self.operation_overlay.as_ref()
    }

    #[cfg(test)]
    pub fn active_operation(&self) -> Option<&crate::state::ActiveOperation> {
        self.workspace()
            .and_then(|workspace| workspace.state().active_operation())
    }

    #[cfg(test)]
    pub fn operation_error(&self) -> Option<&stax::application::OperationError> {
        self.workspace()
            .and_then(|workspace| workspace.state().operation_error())
    }

    #[cfg(test)]
    pub fn snapshot(&self) -> &RepositorySnapshot {
        self.workspace()
            .expect("workspace should be loaded")
            .state()
            .snapshot()
    }

    #[cfg(test)]
    pub fn current_branch(&self) -> String {
        self.snapshot().current_branch.clone()
    }

    #[cfg(test)]
    pub fn selected_branch(&self) -> String {
        self.workspace()
            .and_then(|workspace| workspace.state().selected_branch())
            .expect("branch should be selected")
            .to_string()
    }

    #[cfg(test)]
    pub fn banner_is_visible(&self) -> bool {
        self.workspace().is_some_and(|workspace| {
            let state = workspace.state();
            state.active_operation().is_some()
                || state.operation_error().is_some()
                || state.last_receipt().is_some()
        })
    }

    #[cfg(test)]
    pub fn copied_diagnostics(&self) -> Option<String> {
        self.copied_diagnostics.clone()
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
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.open_repository.enabled)
        {
            return;
        }
        self.start_load(path, RootLoadKind::Open, window, cx);
    }

    pub fn refresh_repository(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.refresh.enabled)
        {
            return;
        }
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

    fn selected_branch_name(&self) -> Option<String> {
        self.workspace()
            .and_then(|workspace| workspace.state().selected_branch())
            .map(str::to_string)
    }

    fn non_trunk_branches(&self) -> Vec<String> {
        self.workspace()
            .map(|workspace| {
                workspace
                    .state()
                    .snapshot()
                    .branches
                    .iter()
                    .filter(|branch| !branch.is_trunk)
                    .map(|branch| branch.name.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn interaction_state(&self) -> Option<InteractionState> {
        self.workspace()
            .map(|workspace| workspace.state().interaction_state())
    }

    fn open_overlay(
        &mut self,
        overlay: operation_overlay::OperationOverlay,
        branch_input: Option<Entity<BranchNameInput>>,
        cx: &mut Context<Self>,
    ) {
        if self.overlay_return_focus.is_none() {
            self.overlay_return_focus = Some(self.focus_handle.clone());
        }
        self.operation_overlay = Some(overlay);
        self.branch_input = branch_input.clone();
        self.branch_input_observation = branch_input.as_ref().map(|input| {
            self.branch_input_text = input.read(cx).text().to_string();
            cx.observe(input, |app, input, cx| {
                app.branch_input_text = input.read(cx).text().to_string();
            })
        });
        cx.notify();
    }

    fn clear_overlay(&mut self) {
        self.operation_overlay = None;
        self.branch_input = None;
        self.branch_input_text.clear();
        self.branch_input_observation = None;
    }

    fn move_overlay_selection(&mut self, direction: SelectionDirection) -> bool {
        let Some(overlay) = self.operation_overlay.as_mut() else {
            return false;
        };
        match overlay {
            operation_overlay::OperationOverlay::PickMoveParent {
                candidates,
                selected,
                ..
            } => match direction {
                SelectionDirection::Previous => {
                    *selected = selected.saturating_sub(1);
                }
                SelectionDirection::Next => {
                    *selected = selected
                        .saturating_add(1)
                        .min(candidates.len().saturating_sub(1));
                }
            },
            operation_overlay::OperationOverlay::ReorderStack {
                proposed, moving, ..
            } => {
                let target = match direction {
                    SelectionDirection::Previous => moving.checked_sub(1),
                    SelectionDirection::Next => moving
                        .checked_add(1)
                        .filter(|target| *target < proposed.len()),
                };
                if let Some(target) = target {
                    proposed.swap(*moving, target);
                    *moving = target;
                }
            }
            _ => {}
        }
        true
    }

    fn restore_overlay_focus(&mut self, window: &mut Window) {
        if let Some(focus) = self.overlay_return_focus.take() {
            focus.focus(window);
        }
    }

    fn open_create_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.create.enabled)
        {
            return;
        }
        let Some(parent) = self.selected_branch_name() else {
            return;
        };
        let input = cx.new(|cx| BranchNameInput::new(String::new(), window, cx));
        self.open_overlay(
            operation_overlay::OperationOverlay::CreateBranch {
                parent,
                validation_error: None,
            },
            Some(input),
            cx,
        );
    }

    fn open_rename_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.rename.enabled)
        {
            return;
        }
        let Some(branch) = self.selected_branch_name() else {
            return;
        };
        let input = cx.new(|cx| BranchNameInput::new(String::new(), window, cx));
        self.open_overlay(
            operation_overlay::OperationOverlay::RenameBranch {
                branch,
                validation_error: None,
            },
            Some(input),
            cx,
        );
    }

    fn open_delete_overlay(&mut self, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.delete.enabled)
        {
            return;
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        let Some(branch) = workspace.state().selected_branch().map(str::to_string) else {
            return;
        };
        let descendants = workspace.state().descendants_of(&branch);
        self.open_overlay(
            operation_overlay::OperationOverlay::ConfirmDelete {
                branch,
                descendants,
            },
            None,
            cx,
        );
    }

    fn open_move_overlay(&mut self, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.move_subtree.enabled)
        {
            return;
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        let Some(source) = workspace.state().selected_branch().map(str::to_string) else {
            return;
        };
        let candidates = workspace.state().move_parent_candidates(&source);
        self.open_overlay(
            operation_overlay::OperationOverlay::PickMoveParent {
                source,
                candidates,
                query: String::new(),
                selected: 0,
            },
            None,
            cx,
        );
    }

    fn open_reorder_overlay(&mut self, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.reorder.enabled)
        {
            return;
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        let Some(branch) = workspace.state().selected_branch() else {
            return;
        };
        let Some(original) = workspace.state().linear_stack_order(branch) else {
            return;
        };
        self.open_overlay(
            operation_overlay::OperationOverlay::ReorderStack {
                proposed: original.clone(),
                original,
                moving: 0,
            },
            None,
            cx,
        );
    }

    fn open_history_overlay(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some() {
            return;
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        let actions = workspace.state().interaction_state();
        if (redo && !actions.redo.enabled) || (!redo && !actions.undo.enabled) {
            return;
        }
        let Some(transaction) = workspace
            .state()
            .last_receipt()
            .and_then(|receipt| receipt.transaction.as_ref())
        else {
            return;
        };
        let overlay = if redo {
            operation_overlay::OperationOverlay::ConfirmRedo {
                operation_id: transaction.id.clone(),
                branches: transaction.branches.clone(),
            }
        } else {
            operation_overlay::OperationOverlay::ConfirmUndo {
                operation_id: transaction.id.clone(),
                branches: transaction.branches.clone(),
            }
        };
        self.open_overlay(overlay, None, cx);
    }

    fn open_restack_overlay(
        &mut self,
        scope: RestackScope,
        affected_branches: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.restack.enabled && !actions.restack_all.enabled)
        {
            return;
        }
        self.open_overlay(
            operation_overlay::OperationOverlay::ConfirmRestack {
                scope,
                affected_branches,
                auto_stash: false,
            },
            None,
            cx,
        );
    }

    fn open_submit_overlay(&mut self, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some()
            || self
                .interaction_state()
                .is_none_or(|actions| !actions.submit.enabled)
        {
            return;
        }
        let Some(workspace) = self.workspace() else {
            return;
        };
        self.open_overlay(
            operation_overlay::OperationOverlay::ConfirmSubmit {
                current_branch: workspace.state().snapshot().current_branch.clone(),
                affected_branches: self.non_trunk_branches(),
                mode: PullRequestMode::Draft,
            },
            None,
            cx,
        );
    }

    pub(super) fn dismiss_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .workspace()
            .and_then(|workspace| workspace.state().active_operation())
            .is_some()
        {
            return;
        }
        self.clear_overlay();
        self.restore_overlay_focus(window);
        cx.notify();
    }

    pub(super) fn confirm_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(overlay) = self.operation_overlay.clone() else {
            return;
        };
        match overlay {
            operation_overlay::OperationOverlay::CreateBranch { parent, .. } => {
                let name = self.branch_input_text.trim().to_string();
                if name.is_empty() {
                    self.operation_overlay =
                        Some(operation_overlay::OperationOverlay::CreateBranch {
                            parent,
                            validation_error: Some("Enter a branch name.".into()),
                        });
                    cx.notify();
                    return;
                }
                self.clear_overlay();
                self.start_operation(OperationRequest::CreateBranch { name, parent }, window, cx);
            }
            operation_overlay::OperationOverlay::RenameBranch { branch, .. } => {
                let new_name = self.branch_input_text.trim().to_string();
                if new_name.is_empty() {
                    self.operation_overlay =
                        Some(operation_overlay::OperationOverlay::RenameBranch {
                            branch,
                            validation_error: Some("Enter a branch name.".into()),
                        });
                    cx.notify();
                    return;
                }
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::RenameBranch { branch, new_name },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmDelete { branch, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::DeleteBranch {
                        branch,
                        force: true,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::PickMoveParent {
                source,
                candidates,
                selected,
                ..
            } => {
                let Some(new_parent) = candidates.get(selected).cloned() else {
                    return;
                };
                let mut branches = vec![source.clone()];
                if let Some(workspace) = self.workspace() {
                    branches.extend(workspace.state().descendants_of(&source));
                }
                self.operation_overlay = Some(operation_overlay::OperationOverlay::ConfirmMove {
                    source,
                    new_parent,
                    branches,
                    auto_stash: false,
                });
                cx.notify();
            }
            operation_overlay::OperationOverlay::ConfirmMove {
                source,
                new_parent,
                auto_stash,
                ..
            } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::MoveSubtree {
                        source,
                        new_parent,
                        auto_stash,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ReorderStack {
                original, proposed, ..
            } => {
                self.operation_overlay =
                    Some(operation_overlay::OperationOverlay::ConfirmReorder {
                        original,
                        proposed,
                        auto_stash: false,
                    });
                cx.notify();
            }
            operation_overlay::OperationOverlay::ConfirmReorder {
                original,
                proposed,
                auto_stash,
            } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::ReorderStack {
                        original_order: original,
                        proposed_order: proposed,
                        auto_stash,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmUndo { operation_id, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::UndoTransaction {
                        operation_id: Some(operation_id),
                        update_remote: false,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmRedo { operation_id, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::RedoTransaction {
                        operation_id: Some(operation_id),
                        update_remote: false,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmRestack { scope, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::Restack {
                        scope,
                        auto_stash: false,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmStashAndRestack { scope, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::Restack {
                        scope,
                        auto_stash: true,
                    },
                    window,
                    cx,
                );
            }
            operation_overlay::OperationOverlay::ConfirmSubmit { mode, .. } => {
                self.clear_overlay();
                self.start_operation(
                    OperationRequest::SubmitStack {
                        new_pull_requests: mode,
                    },
                    window,
                    cx,
                );
            }
        }
    }

    #[allow(dead_code)]
    pub fn start_operation(
        &mut self,
        request: OperationRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((token, repository_root)) = self.workspace_mut().and_then(|workspace| {
            let repository_root = workspace.state().snapshot().repository_root.clone();
            workspace
                .begin_operation(request.clone())
                .map(|token| (token, repository_root))
        }) else {
            return;
        };

        let (sender, receiver) = async_channel::bounded(32);
        let operation = Arc::clone(&self.services.operation);
        let background = cx.background_executor().clone();
        let retained_result =
            background.spawn(operation.execute(repository_root.clone(), request, sender));
        cx.notify();

        cx.spawn_in(window, async move |this, cx| {
            let mut streamed_terminal = None;
            while let Ok(event) = receiver.recv().await {
                let accepted_terminal = this
                    .update_in(cx, |view, _window, cx| {
                        let accepted = view
                            .workspace_mut()
                            .and_then(|workspace| workspace.apply_operation_event(&token, event));
                        if accepted.is_some() {
                            cx.notify();
                        }
                        accepted
                    })
                    .ok()
                    .flatten();
                if accepted_terminal.is_some() {
                    streamed_terminal = accepted_terminal;
                }
            }

            let result = retained_result.await;
            let _ = this.update_in(cx, |view, window, cx| {
                view.finish_operation_from_retained_result(
                    &token,
                    repository_root,
                    streamed_terminal,
                    result,
                    window,
                    cx,
                );
            });
        })
        .detach();
    }

    #[allow(dead_code)]
    fn finish_operation_from_retained_result(
        &mut self,
        token: &crate::state::OperationToken,
        repository_root: PathBuf,
        streamed_terminal: Option<OperationEvent>,
        result: OperationResult,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        debug_assert!(streamed_terminal.as_ref().is_none_or(|event| {
            matches!(
                (event, &result),
                (OperationEvent::Completed(_), Ok(_)) | (OperationEvent::Failed(_), Err(_))
            )
        }));
        let stash_overlay = match &result {
            Err(error)
                if error.kind == OperationErrorKind::DirtyWorktree
                    && matches!(
                        error.request,
                        OperationRequest::Restack {
                            auto_stash: false,
                            ..
                        }
                    ) =>
            {
                let scope = match &error.request {
                    OperationRequest::Restack { scope, .. } => scope.clone(),
                    _ => RestackScope::All,
                };
                let dirty_worktrees = match &error.details {
                    OperationErrorDetails::Rebase { worktree, .. } => vec![worktree.clone()],
                    _ => vec![repository_root.clone()],
                };
                Some(
                    operation_overlay::OperationOverlay::ConfirmStashAndRestack {
                        scope,
                        dirty_worktrees,
                    },
                )
            }
            Err(error)
                if error.kind == OperationErrorKind::DirtyWorktree
                    && matches!(
                        error.request,
                        OperationRequest::MoveSubtree {
                            auto_stash: false,
                            ..
                        }
                    ) =>
            {
                let OperationRequest::MoveSubtree {
                    source, new_parent, ..
                } = &error.request
                else {
                    unreachable!()
                };
                let mut branches = vec![source.clone()];
                if let Some(workspace) = self.workspace() {
                    branches.extend(workspace.state().descendants_of(source));
                }
                Some(operation_overlay::OperationOverlay::ConfirmMove {
                    source: source.clone(),
                    new_parent: new_parent.clone(),
                    branches,
                    auto_stash: true,
                })
            }
            Err(error)
                if error.kind == OperationErrorKind::DirtyWorktree
                    && matches!(
                        error.request,
                        OperationRequest::ReorderStack {
                            auto_stash: false,
                            ..
                        }
                    ) =>
            {
                let OperationRequest::ReorderStack {
                    original_order,
                    proposed_order,
                    ..
                } = &error.request
                else {
                    unreachable!()
                };
                Some(operation_overlay::OperationOverlay::ConfirmReorder {
                    original: original_order.clone(),
                    proposed: proposed_order.clone(),
                    auto_stash: true,
                })
            }
            _ => None,
        };
        let Some(effect) = self
            .workspace_mut()
            .and_then(|workspace| workspace.finish_operation(token, result))
        else {
            return;
        };
        if let Some(overlay) = stash_overlay {
            self.open_overlay(overlay, None, cx);
            return;
        }
        self.restore_overlay_focus(window);
        if let Some(url) = effect.open_url
            && let Err(error) = self.services.browser.open_url(&url, cx)
        {
            self.present_browser_error(url, error);
        }
        if effect.refresh_snapshot {
            self.start_load(repository_root, RootLoadKind::Refresh, window, cx);
        }
        cx.notify();
    }

    pub fn select_branch(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.navigation.enabled)
        {
            return;
        }
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
                self.clear_overlay();
                self.search_input = None;
                self.search_input_observation = None;
                self.overlay_return_focus = None;
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
                    let preferences = self
                        .services
                        .workspace_preferences
                        .load(&snapshot.repository_root);
                    self.action_error = None;
                    self.mode = AppMode::Workspace(Box::new(
                        WorkspaceView::from_snapshot_with_preferences(snapshot, preferences),
                    ));
                    self.install_search_input(cx);
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
        if self.move_overlay_selection(SelectionDirection::Previous) {
            cx.notify();
            return;
        }
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.navigation.enabled)
        {
            return;
        }
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.move_selection(SelectionDirection::Previous))
        {
            self.hydrate_selection(window, cx);
            cx.notify();
        }
    }

    fn select_next(&mut self, _: &SelectNextBranch, window: &mut Window, cx: &mut Context<Self>) {
        if self.move_overlay_selection(SelectionDirection::Next) {
            cx.notify();
            return;
        }
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.navigation.enabled)
        {
            return;
        }
        if self
            .workspace_mut()
            .is_some_and(|workspace| workspace.move_selection(SelectionDirection::Next))
        {
            self.hydrate_selection(window, cx);
            cx.notify();
        }
    }

    fn toggle_stack_pane_action(
        &mut self,
        _: &ToggleStackPane,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_pane(PaneKind::Stack, cx);
    }

    fn toggle_changes_pane_action(
        &mut self,
        _: &ToggleChangesPane,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_pane(PaneKind::Changes, cx);
    }

    fn toggle_inspector_pane_action(
        &mut self,
        _: &ToggleInspectorPane,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_pane(PaneKind::Inspector, cx);
    }

    fn open_action(&mut self, _: &OpenRepository, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.open_repository.enabled)
        {
            return;
        }
        self.pick_repository(window, cx);
    }

    fn refresh_action(
        &mut self,
        _: &RefreshRepository,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .interaction_state()
            .is_some_and(|actions| !actions.refresh.enabled)
        {
            return;
        }
        self.refresh_repository(window, cx);
    }

    pub(super) fn checkout_action(
        &mut self,
        _: &CheckoutSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.focus_handle.is_focused(window) {
            return;
        }
        self.checkout_selected_branch(window, cx);
    }

    pub(super) fn checkout_selected_branch(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.operation_overlay.is_some() {
            return;
        }
        if self
            .interaction_state()
            .is_none_or(|actions| !actions.checkout.enabled)
        {
            return;
        }
        let Some(branch) = self.selected_branch_name() else {
            return;
        };
        self.start_operation(OperationRequest::Checkout { branch }, window, cx);
    }

    pub(super) fn create_action(
        &mut self,
        _: &CreateBranch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_create_overlay(window, cx);
    }

    pub(super) fn rename_action(
        &mut self,
        _: &RenameSelected,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_rename_overlay(window, cx);
    }

    pub(super) fn delete_action(
        &mut self,
        _: &DeleteSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_delete_overlay(cx);
    }

    pub(super) fn move_action(
        &mut self,
        _: &MoveSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_move_overlay(cx);
    }

    pub(super) fn reorder_action(
        &mut self,
        _: &ReorderSelectedStack,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_reorder_overlay(cx);
    }

    pub(super) fn undo_action(
        &mut self,
        _: &UndoLatest,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_history_overlay(false, cx);
    }

    pub(super) fn redo_action(
        &mut self,
        _: &RedoLatest,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_history_overlay(true, cx);
    }

    pub(super) fn restack_selected_action(
        &mut self,
        _: &RestackSelected,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .interaction_state()
            .is_none_or(|actions| !actions.restack.enabled)
        {
            return;
        }
        let Some(branch) = self.selected_branch_name() else {
            return;
        };
        self.open_restack_overlay(
            RestackScope::StackContaining(branch),
            self.non_trunk_branches(),
            cx,
        );
    }

    fn restack_all_action(&mut self, _: &RestackAll, _window: &mut Window, cx: &mut Context<Self>) {
        if self
            .interaction_state()
            .is_none_or(|actions| !actions.restack_all.enabled)
        {
            return;
        }
        self.open_restack_overlay(RestackScope::All, self.non_trunk_branches(), cx);
    }

    pub(super) fn submit_action(
        &mut self,
        _: &SubmitStack,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_submit_overlay(cx);
    }

    pub(super) fn open_pull_request_action(
        &mut self,
        _: &OpenPullRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .interaction_state()
            .is_none_or(|actions| !actions.open_pr.enabled)
        {
            return;
        }
        let Some(branch) = self.selected_branch_name() else {
            return;
        };
        self.start_operation(
            OperationRequest::ResolvePullRequestUrl { branch },
            window,
            cx,
        );
    }

    fn confirm_overlay_action(
        &mut self,
        _: &ConfirmOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.operation_overlay.is_none() {
            self.checkout_action(&CheckoutSelected, window, cx);
            return;
        }
        self.confirm_overlay(window, cx);
    }

    fn dismiss_overlay_action(
        &mut self,
        _: &DismissOverlay,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_overlay(window, cx);
    }

    fn dismiss_operation_banner_action(
        &mut self,
        _: &DismissOperationBanner,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_operation_banner(cx);
    }

    pub(super) fn dismiss_operation_banner(&mut self, cx: &mut Context<Self>) {
        if let Some(workspace) = self.workspace_mut()
            && workspace.state().active_operation().is_none()
        {
            workspace.state_mut().dismiss_operation_presentation();
            cx.notify();
        }
    }

    pub(super) fn open_url_from_presentation(&mut self, url: String, cx: &mut Context<Self>) {
        if let Err(error) = self.services.browser.open_url(&url, cx) {
            self.present_browser_error(url, error);
        }
        cx.notify();
    }

    pub(super) fn copy_operation_diagnostics(&mut self, cx: &mut Context<Self>) {
        let Some(diagnostic) = self
            .workspace()
            .and_then(|workspace| workspace.state().operation_error())
            .map(|error| error.diagnostic_chain.clone())
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(diagnostic.clone()));
        #[cfg(test)]
        {
            self.copied_diagnostics = Some(diagnostic);
        }
        cx.notify();
    }

    fn present_browser_error(&mut self, url: String, diagnostic: String) {
        let Some(workspace) = self.workspace_mut() else {
            return;
        };
        let receipt = workspace.state().last_receipt().cloned();
        let request = receipt.as_ref().map_or_else(
            || OperationRequest::ResolvePullRequestUrl {
                branch: workspace
                    .state()
                    .selected_branch()
                    .unwrap_or("selected branch")
                    .to_string(),
            },
            |receipt| receipt.request.clone(),
        );
        workspace
            .state_mut()
            .present_operation_error(OperationError {
                request,
                kind: OperationErrorKind::UnsupportedCapability,
                details: OperationErrorDetails::None,
                primary: "Could not open the pull request URL.".into(),
                action: format!("Copy and open this URL in a browser: {url}"),
                diagnostic_chain: diagnostic,
                receipt,
                side_effects: OperationSideEffects::None,
            });
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
        let search_input = self.search_input.clone();
        let actions = self.interaction_state();
        let has_workspace = actions.is_some();
        let can_open = actions
            .as_ref()
            .is_none_or(|actions| actions.open_repository.enabled);
        let has_overlay = self.operation_overlay.is_some();
        let content = match &self.mode {
            AppMode::Welcome(welcome) | AppMode::Opening(welcome) | AppMode::Error(welcome) => {
                welcome.render(theme, cx)
            }
            AppMode::Workspace(workspace) => workspace.render(search_input, theme, cx),
        };

        let mut root = div()
            .id("stax-app")
            .key_context("StaxApp")
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::dismiss_operation_banner_action))
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.navigation.enabled),
                |root| {
                    root.on_action(cx.listener(Self::select_previous))
                        .on_action(cx.listener(Self::select_next))
                },
            )
            .when(can_open, |root| {
                root.on_action(cx.listener(Self::open_action))
            })
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.refresh.enabled),
                |root| root.on_action(cx.listener(Self::refresh_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.checkout.enabled),
                |root| root.on_action(cx.listener(Self::checkout_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.create.enabled),
                |root| root.on_action(cx.listener(Self::create_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.rename.enabled),
                |root| root.on_action(cx.listener(Self::rename_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.delete.enabled),
                |root| root.on_action(cx.listener(Self::delete_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.move_subtree.enabled),
                |root| root.on_action(cx.listener(Self::move_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.reorder.enabled),
                |root| root.on_action(cx.listener(Self::reorder_action)),
            )
            .when(
                actions.as_ref().is_some_and(|actions| actions.undo.enabled),
                |root| root.on_action(cx.listener(Self::undo_action)),
            )
            .when(
                actions.as_ref().is_some_and(|actions| actions.redo.enabled),
                |root| root.on_action(cx.listener(Self::redo_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.restack.enabled),
                |root| root.on_action(cx.listener(Self::restack_selected_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.restack_all.enabled),
                |root| root.on_action(cx.listener(Self::restack_all_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.submit.enabled),
                |root| root.on_action(cx.listener(Self::submit_action)),
            )
            .when(
                actions
                    .as_ref()
                    .is_some_and(|actions| actions.open_pr.enabled),
                |root| root.on_action(cx.listener(Self::open_pull_request_action)),
            )
            .when(has_overlay, |root| {
                root.on_action(cx.listener(Self::confirm_overlay_action))
                    .on_action(cx.listener(Self::dismiss_overlay_action))
            })
            .when(has_workspace, |root| {
                root.on_action(cx.listener(Self::toggle_stack_pane_action))
                    .on_action(cx.listener(Self::toggle_changes_pane_action))
                    .on_action(cx.listener(Self::toggle_inspector_pane_action))
                    .on_action(cx.listener(Self::focus_stack_search_action))
                    .on_action(cx.listener(Self::clear_stack_search_action))
            })
            .on_mouse_move(cx.listener(Self::pane_drag_move))
            .on_mouse_up(gpui::MouseButton::Left, cx.listener(Self::pane_drag_end))
            .size_full()
            .relative()
            .border_1()
            .border_color(if self.focus_handle.is_focused(window) {
                theme.focus
            } else {
                theme.border
            })
            .font_family(SYSTEM_UI_FONT)
            .bg(theme.window)
            .text_color(theme.text)
            .child(content);
        if let Some(overlay) = &self.operation_overlay {
            root = root.child(operation_overlay::render(
                overlay,
                self.branch_input.clone(),
                theme,
                cx,
            ));
        }
        root
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlKind {
    Primary,
    Secondary,
}

pub fn control_button(
    id: &'static str,
    label: impl Into<SharedString>,
    kind: ControlKind,
    enabled: bool,
    theme: Theme,
) -> Stateful<Div> {
    let label = label.into();
    let focus_color = match kind {
        ControlKind::Primary => theme.focus_on_accent,
        ControlKind::Secondary => theme.focus,
    };
    let base = div()
        .id(id)
        .debug_selector(move || id.into())
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
        .focus(move |style| style.border_2().border_color(focus_color))
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

pub fn activate_control(
    control: Stateful<Div>,
    cx: &Context<AppView>,
    handler: impl Fn(&mut AppView, &mut Window, &mut Context<AppView>) + 'static,
) -> Stateful<Div> {
    control.on_click(
        cx.listener(move |app: &mut AppView, _: &ClickEvent, window, cx| {
            cx.stop_propagation();
            handler(app, window, cx);
        }),
    )
}

pub fn control_focus_style(style: StyleRefinement, theme: Theme) -> StyleRefinement {
    style.border_2().border_color(theme.focus)
}

pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("up", SelectPreviousBranch, Some("StaxApp")),
        KeyBinding::new("down", SelectNextBranch, Some("StaxApp")),
        KeyBinding::new("enter", CheckoutSelected, Some("StaxApp")),
        KeyBinding::new("n", CreateBranch, Some("StaxApp")),
        KeyBinding::new("e", RenameSelected, Some("StaxApp")),
        KeyBinding::new("d", DeleteSelected, Some("StaxApp")),
        KeyBinding::new("m", MoveSelected, Some("StaxApp")),
        KeyBinding::new("o", ReorderSelectedStack, Some("StaxApp")),
        KeyBinding::new("cmd-z", UndoLatest, Some("StaxApp")),
        KeyBinding::new("cmd-shift-z", RedoLatest, Some("StaxApp")),
        KeyBinding::new("r", RestackSelected, Some("StaxApp")),
        KeyBinding::new("shift-r", RestackAll, Some("StaxApp")),
        KeyBinding::new("s", SubmitStack, Some("StaxApp")),
        KeyBinding::new("p", OpenPullRequest, Some("StaxApp")),
        KeyBinding::new("cmd-o", OpenRepository, Some("StaxApp")),
        KeyBinding::new("cmd-r", RefreshRepository, Some("StaxApp")),
        KeyBinding::new("1", ToggleStackPane, Some("StaxApp")),
        KeyBinding::new("2", ToggleChangesPane, Some("StaxApp")),
        KeyBinding::new("3", ToggleInspectorPane, Some("StaxApp")),
        KeyBinding::new("/", FocusStackSearch, Some("StaxApp")),
        KeyBinding::new("enter", ConfirmOverlay, Some("StaxApp")),
        KeyBinding::new("escape", DismissOverlay, Some("StaxApp")),
        KeyBinding::new(
            "backspace",
            super::text_input::Backspace,
            Some("BranchNameInput"),
        ),
        KeyBinding::new("delete", super::text_input::Delete, Some("BranchNameInput")),
        KeyBinding::new("left", super::text_input::Left, Some("BranchNameInput")),
        KeyBinding::new("right", super::text_input::Right, Some("BranchNameInput")),
        KeyBinding::new("home", super::text_input::Home, Some("BranchNameInput")),
        KeyBinding::new("end", super::text_input::End, Some("BranchNameInput")),
        KeyBinding::new("n", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("e", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("d", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("m", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("o", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("r", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("shift-r", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("s", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new("p", gpui::NoAction, Some("BranchNameInput")),
        KeyBinding::new(
            "backspace",
            super::text_input::Backspace,
            Some("StackSearchInput"),
        ),
        KeyBinding::new(
            "delete",
            super::text_input::Delete,
            Some("StackSearchInput"),
        ),
        KeyBinding::new("left", super::text_input::Left, Some("StackSearchInput")),
        KeyBinding::new("right", super::text_input::Right, Some("StackSearchInput")),
        KeyBinding::new("home", super::text_input::Home, Some("StackSearchInput")),
        KeyBinding::new("end", super::text_input::End, Some("StackSearchInput")),
        KeyBinding::new("escape", ClearStackSearch, Some("StackSearchInput")),
        KeyBinding::new("n", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("e", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("d", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("m", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("o", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("r", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("shift-r", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("s", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("p", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("1", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("2", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("3", gpui::NoAction, Some("StackSearchInput")),
        KeyBinding::new("/", gpui::NoAction, Some("StackSearchInput")),
    ]);
}
