#[cfg(test)]
use super::PaneMarkers;
use super::text_input::BranchNameInput;
use super::{
    AppView, ControlKind, activate_control,
    app::{CreateBranch, RedoLatest, SubmitStack, UndoLatest},
    changes_pane, control_button, inspector_pane, stack_pane,
};
use crate::preferences::WorkspacePreferences;
use crate::state::{ActionAvailability, SelectionDirection, WorkspaceState};
use crate::theme::{SYSTEM_UI_FONT, Theme};
use gpui::{
    Context, CursorStyle, Div, Entity, InteractiveElement as _, MouseButton, ParentElement as _,
    ScrollStrategy, StatefulInteractiveElement as _, Styled as _, UniformListScrollHandle, div, px,
    relative,
};
use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, OperationError,
    OperationOutcome, OperationProgress, OperationReceipt, OperationStage, PullRequestChange,
    RepositorySnapshot, TransactionStatus,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshState {
    Idle,
    Loading,
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneKind {
    Stack,
    Changes,
    Inspector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaneDivider {
    StackChanges,
    StackInspector,
    ChangesInspector,
}

impl PaneDivider {
    fn panes(self) -> (PaneKind, PaneKind) {
        match self {
            Self::StackChanges => (PaneKind::Stack, PaneKind::Changes),
            Self::StackInspector => (PaneKind::Stack, PaneKind::Inspector),
            Self::ChangesInspector => (PaneKind::Changes, PaneKind::Inspector),
        }
    }

    fn between(left: PaneKind, right: PaneKind) -> Self {
        match (left, right) {
            (PaneKind::Stack, PaneKind::Changes) => Self::StackChanges,
            (PaneKind::Stack, PaneKind::Inspector) => Self::StackInspector,
            (PaneKind::Changes, PaneKind::Inspector) => Self::ChangesInspector,
            _ => unreachable!("pane order is fixed"),
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::StackChanges => "pane-divider-stack-changes",
            Self::StackInspector => "pane-divider-stack-inspector",
            Self::ChangesInspector => "pane-divider-changes-inspector",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceView {
    state: WorkspaceState,
    refresh: RefreshState,
    notice: Option<String>,
    storage_notice: Option<String>,
    preferences: WorkspacePreferences,
    stack_scroll_handle: UniformListScrollHandle,
    stack_scroll_target: Option<usize>,
}

impl WorkspaceView {
    #[cfg(test)]
    pub fn from_snapshot(snapshot: RepositorySnapshot) -> Self {
        Self::from_snapshot_with_preferences(snapshot, WorkspacePreferences::default())
    }

    pub fn from_snapshot_with_preferences(
        snapshot: RepositorySnapshot,
        preferences: WorkspacePreferences,
    ) -> Self {
        let preferences = preferences.normalized().unwrap_or_default();
        let mut workspace = Self {
            state: WorkspaceState::new(snapshot),
            refresh: RefreshState::Idle,
            notice: None,
            storage_notice: None,
            preferences,
            stack_scroll_handle: UniformListScrollHandle::new(),
            stack_scroll_target: None,
        };
        workspace.scroll_selection_into_view();
        workspace
    }

    pub fn preferences(&self) -> &WorkspacePreferences {
        &self.preferences
    }

    pub(super) fn toggle_pane(&mut self, pane: PaneKind) -> bool {
        let currently_visible = self.pane_visible(pane);
        if currently_visible && self.preferences.visibility.visible_count() == 1 {
            return false;
        }
        match pane {
            PaneKind::Stack => self.preferences.visibility.stack = !currently_visible,
            PaneKind::Changes => self.preferences.visibility.changes = !currently_visible,
            PaneKind::Inspector => self.preferences.visibility.inspector = !currently_visible,
        }
        true
    }

    pub(super) fn resize_panes(&mut self, divider: PaneDivider, delta: f32) -> bool {
        if !delta.is_finite() {
            return false;
        }
        let (left, right) = divider.panes();
        if !self.pane_visible(left) || !self.pane_visible(right) {
            return false;
        }
        let left_width = self.pane_width(left);
        let right_width = self.pane_width(right);
        let total = left_width + right_width;
        let lower = 0.15_f32.max(total - 0.70);
        let upper = 0.70_f32.min(total - 0.15);
        let next_left = (left_width + delta).clamp(lower, upper);
        if (next_left - left_width).abs() < f32::EPSILON {
            return false;
        }
        self.set_pane_width(left, next_left);
        self.set_pane_width(right, total - next_left);
        true
    }

    fn pane_visible(&self, pane: PaneKind) -> bool {
        match pane {
            PaneKind::Stack => self.preferences.visibility.stack,
            PaneKind::Changes => self.preferences.visibility.changes,
            PaneKind::Inspector => self.preferences.visibility.inspector,
        }
    }

    fn pane_width(&self, pane: PaneKind) -> f32 {
        match pane {
            PaneKind::Stack => self.preferences.widths.stack,
            PaneKind::Changes => self.preferences.widths.changes,
            PaneKind::Inspector => self.preferences.widths.inspector,
        }
    }

    fn set_pane_width(&mut self, pane: PaneKind, width: f32) {
        match pane {
            PaneKind::Stack => self.preferences.widths.stack = width,
            PaneKind::Changes => self.preferences.widths.changes = width,
            PaneKind::Inspector => self.preferences.widths.inspector = width,
        }
    }

    fn visible_panes(&self) -> Vec<PaneKind> {
        [PaneKind::Stack, PaneKind::Changes, PaneKind::Inspector]
            .into_iter()
            .filter(|pane| self.pane_visible(*pane))
            .collect()
    }

    pub fn state(&self) -> &WorkspaceState {
        &self.state
    }

    #[allow(dead_code)]
    pub(crate) fn state_mut(&mut self) -> &mut WorkspaceState {
        &mut self.state
    }

    #[cfg(test)]
    pub fn pane_markers(&self) -> PaneMarkers {
        PaneMarkers::all()
    }

    pub fn select_branch(&mut self, name: &str) -> bool {
        let selected = self.state.select_branch(name).is_some();
        if selected {
            self.scroll_selection_into_view();
        }
        selected
    }

    pub fn move_selection(&mut self, direction: SelectionDirection) -> bool {
        let moved = self.state.move_selection(direction);
        if moved {
            self.scroll_selection_into_view();
        }
        moved
    }

    pub(super) fn set_search_query(&mut self, query: impl Into<String>) {
        self.state.set_search_query(query);
        self.scroll_selection_into_view();
    }

    pub(super) fn begin_hydration(&mut self) -> Option<(DetailRequestToken, BranchSummary)> {
        self.state.begin_hydration()
    }

    pub(super) fn apply_details(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDetails, String>,
    ) -> bool {
        self.state.apply_details(token, result)
    }

    pub(super) fn apply_cached_diff(
        &mut self,
        token: DetailRequestToken,
        diff: BranchDiff,
    ) -> bool {
        self.state.apply_cached_diff(token, diff)
    }

    pub(super) fn apply_diff(
        &mut self,
        token: DetailRequestToken,
        result: Result<BranchDiff, String>,
    ) -> bool {
        self.state.apply_diff(token, result)
    }

    pub(super) fn apply_ci(
        &mut self,
        token: DetailRequestToken,
        result: Result<CiSummary, String>,
    ) -> bool {
        self.state.apply_ci(token, result)
    }

    pub fn begin_refresh(&mut self) {
        self.refresh = RefreshState::Loading;
        self.notice = None;
    }

    pub fn apply_snapshot(&mut self, snapshot: RepositorySnapshot) {
        self.state.replace_snapshot(snapshot);
        self.scroll_selection_into_view();
        self.refresh = RefreshState::Idle;
        self.notice = None;
    }

    pub fn fail_refresh(&mut self, error: String) {
        self.refresh = RefreshState::Failed(error);
    }

    pub fn set_notice(&mut self, notice: String) {
        self.notice = Some(notice);
    }

    pub fn set_storage_notice(&mut self, notice: Option<String>) {
        self.storage_notice = notice;
    }

    pub fn refresh_is_loading(&self) -> bool {
        self.refresh == RefreshState::Loading
    }

    #[allow(dead_code)]
    pub fn begin_operation(
        &mut self,
        request: stax::application::OperationRequest,
    ) -> Option<crate::state::OperationToken> {
        self.state.begin_operation(request)
    }

    #[allow(dead_code)]
    pub fn apply_operation_event(
        &mut self,
        token: &crate::state::OperationToken,
        event: stax::application::OperationEvent,
    ) -> Option<stax::application::OperationEvent> {
        self.state.apply_operation_event(token, event)
    }

    #[allow(dead_code)]
    pub fn finish_operation(
        &mut self,
        token: &crate::state::OperationToken,
        result: stax::application::OperationResult,
    ) -> Option<crate::state::CompletionEffect> {
        self.state.finish_operation(token, result)
    }

    pub fn stack_scroll_handle(&self) -> &UniformListScrollHandle {
        &self.stack_scroll_handle
    }

    #[cfg(test)]
    pub fn stack_scroll_target(&self) -> Option<usize> {
        self.stack_scroll_target
    }

    pub fn inline_error(&self) -> Option<&str> {
        let refresh_error = match &self.refresh {
            RefreshState::Failed(error) => Some(error.as_str()),
            RefreshState::Idle | RefreshState::Loading => None,
        };
        self.notice
            .as_deref()
            .or(refresh_error)
            .or(self.storage_notice.as_deref())
    }

    pub fn render(
        &self,
        search_input: Option<Entity<BranchNameInput>>,
        theme: Theme,
        cx: &mut Context<AppView>,
    ) -> Div {
        let mut root = div()
            .debug_selector(|| "stax-workspace".into())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .font_family(SYSTEM_UI_FONT)
            .bg(theme.window)
            .text_color(theme.text)
            .child(self.render_toolbar(theme, cx));
        if let Some(banner) = self.render_operation_banner(theme, cx) {
            root = root.child(banner);
        }
        let panes = self.visible_panes();
        let visible_total: f32 = panes.iter().map(|pane| self.pane_width(*pane)).sum();
        let mut pane_row = div().flex().flex_1().min_h_0().min_w_0();
        for (index, pane) in panes.iter().copied().enumerate() {
            if index > 0 {
                let divider = PaneDivider::between(panes[index - 1], pane);
                pane_row = pane_row.child(render_pane_divider(divider, theme, cx));
            }
            let width = self.pane_width(pane) / visible_total;
            let content = match pane {
                PaneKind::Stack => stack_pane::render(self, search_input.clone(), theme, cx),
                PaneKind::Changes => changes_pane::render(self, theme, cx),
                PaneKind::Inspector => inspector_pane::render(self, theme, cx),
            };
            pane_row = pane_row.child(content.w(relative(width)).min_w_0());
        }
        root.child(pane_row)
    }

    fn render_toolbar(&self, theme: Theme, cx: &mut Context<AppView>) -> Div {
        let snapshot = self.state.snapshot();
        let actions = self.state.interaction_state();
        let repository_name = repository_name(&snapshot.repository_root);
        let refresh_label = match &self.refresh {
            RefreshState::Idle => "Refresh Repository",
            RefreshState::Loading => "Refreshing…",
            RefreshState::Failed(_) => "Retry Refresh",
        };
        let refresh_enabled = self.refresh != RefreshState::Loading && actions.refresh.enabled;

        let refresh = control_button(
            "toolbar-refresh",
            control_label(refresh_label, &actions.refresh),
            ControlKind::Secondary,
            refresh_enabled,
            theme,
        );
        let refresh = if refresh_enabled {
            activate_control(refresh, cx, |app, window, cx| {
                app.refresh_repository(window, cx);
            })
        } else {
            refresh
        };

        let open = control_button(
            "toolbar-open",
            control_label("Open Repository", &actions.open_repository),
            ControlKind::Secondary,
            actions.open_repository.enabled,
            theme,
        );
        let open = if actions.open_repository.enabled {
            activate_control(open, cx, |app, window, cx| app.pick_repository(window, cx))
        } else {
            open
        };

        let create = control_button(
            "toolbar-create-branch",
            control_label("Create Branch", &actions.create),
            ControlKind::Secondary,
            actions.create.enabled,
            theme,
        );
        let create = if actions.create.enabled {
            activate_control(create, cx, |app, window, cx| {
                app.create_action(&CreateBranch, window, cx);
            })
        } else {
            create
        };

        let submit = control_button(
            "toolbar-submit-stack",
            control_label("Submit Stack", &actions.submit),
            ControlKind::Primary,
            actions.submit.enabled,
            theme,
        );
        let submit = if actions.submit.enabled {
            activate_control(submit, cx, |app, window, cx| {
                app.submit_action(&SubmitStack, window, cx);
            })
        } else {
            submit
        };

        let mut toolbar = div()
            .h(px(50.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.window)
            .child(
                div().min_w_0().flex().items_center().gap_2().child(
                    div()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .child(
                            div()
                                .truncate()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(repository_name),
                        )
                        .child(div().text_color(theme.text_muted).child("/"))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .font_family(crate::theme::MONOSPACE_FONT)
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(snapshot.current_branch.clone()),
                        ),
                ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(create)
                    .child(submit)
                    .child(refresh)
                    .child(open),
            );

        if let Some(error) = self.inline_error() {
            toolbar = toolbar.child(
                div()
                    .max_w(px(300.0))
                    .truncate()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(format!("Action needed: {error}")),
            );
        }

        toolbar
    }

    fn render_operation_banner(&self, theme: Theme, cx: &mut Context<AppView>) -> Option<Div> {
        if let Some(active) = self.state.active_operation() {
            return Some(operation_banner(
                theme,
                "Operation running",
                active.progress.as_ref().map_or_else(
                    || {
                        div()
                            .debug_selector(|| "operation-progress".into())
                            .text_sm()
                            .child(format!("Starting {}", request_label(&active.request)))
                    },
                    |progress| render_progress(progress, theme),
                ),
                None,
            ));
        }
        if let Some(error) = self.state.operation_error() {
            return Some(operation_banner(
                theme,
                "Operation needs attention",
                render_error(error, theme, cx),
                Some(dismiss_button(theme, cx)),
            ));
        }
        self.state.last_receipt().map(|receipt| {
            operation_banner(
                theme,
                "Operation complete",
                render_receipt(receipt, theme, cx),
                Some(dismiss_button(theme, cx)),
            )
        })
    }

    fn scroll_selection_into_view(&mut self) {
        let Some(selected) = self.state.selected_branch() else {
            return;
        };
        let Some(index) = self
            .state
            .filtered_branches()
            .into_iter()
            .position(|branch| branch.name == selected)
        else {
            return;
        };

        self.stack_scroll_target = Some(index);
        self.stack_scroll_handle
            .scroll_to_item(index, ScrollStrategy::Center);
    }
}

fn render_pane_divider(
    divider: PaneDivider,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> gpui::Stateful<Div> {
    div()
        .id(divider.marker())
        .debug_selector(|| divider.marker().into())
        .w(px(5.0))
        .h_full()
        .flex_none()
        .flex()
        .justify_center()
        .cursor(CursorStyle::ResizeLeftRight)
        .hover(move |style| style.bg(theme.surface_hover))
        .child(div().w(px(1.0)).h_full().bg(theme.border))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |app, event: &gpui::MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                app.begin_pane_drag(divider, event.position.x);
            }),
        )
}

fn operation_banner(
    theme: Theme,
    title: &str,
    body: Div,
    action: Option<gpui::Stateful<Div>>,
) -> Div {
    let mut banner = div()
        .debug_selector(|| "operation-banner".into())
        .flex_none()
        .flex()
        .items_start()
        .justify_between()
        .gap_3()
        .px_4()
        .py_3()
        .border_b_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_selected)
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(body),
        );
    if let Some(action) = action {
        banner = banner.child(action);
    }
    banner
}

fn render_progress(progress: &OperationProgress, theme: Theme) -> Div {
    let total = progress
        .total
        .map(|total| format!(" / {total}"))
        .unwrap_or_default();
    let branch = progress
        .branch
        .as_ref()
        .map(|branch| format!(" · {branch}"))
        .unwrap_or_default();
    div()
        .debug_selector(|| "operation-progress".into())
        .flex()
        .flex_col()
        .gap_1()
        .text_sm()
        .child(format!(
            "{}{branch} · {}{total}",
            stage_label(progress.stage),
            progress.completed
        ))
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child(progress.message.clone()),
        )
}

fn render_error(error: &OperationError, theme: Theme, cx: &mut Context<AppView>) -> Div {
    let copy = activate_control(
        control_button(
            "operation-copy-diagnostics",
            "Copy Diagnostics",
            ControlKind::Secondary,
            true,
            theme,
        ),
        cx,
        |app, _window, cx| app.copy_operation_diagnostics(cx),
    );
    div()
        .flex()
        .flex_col()
        .gap_2()
        .text_sm()
        .child(div().text_color(theme.danger).child(error.primary.clone()))
        .child(error.action.clone())
        .child(format!("Kind: {:?}", error.kind))
        .child(copy)
}

fn render_receipt(receipt: &OperationReceipt, theme: Theme, cx: &mut Context<AppView>) -> Div {
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .text_sm()
        .child(receipt.summary.clone());
    if !receipt.affected_branches.is_empty() {
        body = body.child(format!(
            "Affected branches: {}",
            receipt.affected_branches.join(", ")
        ));
    }
    if !receipt.warnings.is_empty() {
        body = body.child(format!("Warnings: {}", receipt.warnings.len()));
    }
    if let Some(transaction) = &receipt.transaction {
        body = body.child(format!(
            "Transaction {} · {:?} · undo: {} · redo: {}",
            transaction.id, transaction.status, transaction.can_undo, transaction.can_redo
        ));
        if transaction.changed_remote_refs {
            body = body.child(format!(
                "Remote refs changed. Use `st undo {}` or `st redo {}` in the CLI to review remote restoration.",
                transaction.id, transaction.id
            ));
        } else {
            let mut controls = div().flex().gap_2();
            if transaction.can_undo {
                controls = controls.child(activate_control(
                    control_button(
                        "operation-receipt-undo",
                        "Undo",
                        ControlKind::Secondary,
                        true,
                        theme,
                    ),
                    cx,
                    |app, window, cx| app.undo_action(&UndoLatest, window, cx),
                ));
            }
            if transaction.can_redo {
                controls = controls.child(activate_control(
                    control_button(
                        "operation-receipt-redo",
                        "Redo",
                        ControlKind::Secondary,
                        true,
                        theme,
                    ),
                    cx,
                    |app, window, cx| app.redo_action(&RedoLatest, window, cx),
                ));
            }
            body = body.child(controls);
        }
    }
    if let OperationOutcome::Submitted { pull_requests } = &receipt.outcome {
        for (index, pull_request) in pull_requests.iter().enumerate() {
            let url = pull_request.url.clone();
            let label = format!(
                "{} PR #{} · {} · {}",
                pull_request.branch,
                pull_request.number,
                change_label(pull_request.change),
                pull_request.url
            );
            let row = div()
                .id(gpui::SharedString::from(format!(
                    "operation-receipt-url-{index}"
                )))
                .debug_selector(move || format!("operation-receipt-url-{index}"))
                .focusable()
                .tab_index(90 + index as isize)
                .cursor_pointer()
                .px_2()
                .py_1()
                .rounded_md()
                .border_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .text_color(theme.accent)
                .child(label);
            body = body.child(activate_control(row, cx, move |app, _window, cx| {
                app.open_url_from_presentation(url.clone(), cx);
            }));
        }
    }
    body
}

fn dismiss_button(theme: Theme, cx: &mut Context<AppView>) -> gpui::Stateful<Div> {
    activate_control(
        control_button(
            "operation-banner-dismiss",
            "Dismiss",
            ControlKind::Secondary,
            true,
            theme,
        ),
        cx,
        |app, _window, cx| app.dismiss_operation_banner(cx),
    )
}

fn control_label(label: &str, availability: &ActionAvailability) -> String {
    if availability.enabled {
        label.to_string()
    } else {
        availability
            .reason
            .as_ref()
            .map(|reason| format!("{label} — {reason}"))
            .unwrap_or_else(|| label.to_string())
    }
}

fn request_label(request: &stax::application::OperationRequest) -> &'static str {
    match request {
        stax::application::OperationRequest::Checkout { .. } => "checkout",
        stax::application::OperationRequest::CreateBranch { .. } => "branch creation",
        stax::application::OperationRequest::RenameBranch { .. } => "branch rename",
        stax::application::OperationRequest::DeleteBranch { .. } => "branch deletion",
        stax::application::OperationRequest::MoveSubtree { .. } => "subtree move",
        stax::application::OperationRequest::ReorderStack { .. } => "stack reorder",
        stax::application::OperationRequest::UndoTransaction { .. } => "undo",
        stax::application::OperationRequest::RedoTransaction { .. } => "redo",
        stax::application::OperationRequest::Restack { .. } => "restack",
        stax::application::OperationRequest::SubmitStack { .. } => "submit",
        stax::application::OperationRequest::ResolvePullRequestUrl { .. } => "pull request lookup",
    }
}

fn stage_label(stage: OperationStage) -> &'static str {
    match stage {
        OperationStage::Validating => "Validating",
        OperationStage::Preparing => "Preparing",
        OperationStage::CheckingOut => "Checking out",
        OperationStage::CreatingBranch => "Creating branch",
        OperationStage::RenamingBranch => "Renaming branch",
        OperationStage::DeletingBranch => "Deleting branch",
        OperationStage::MovingSubtree => "Moving subtree",
        OperationStage::ReorderingStack => "Reordering stack",
        OperationStage::UndoingTransaction => "Undoing operation",
        OperationStage::RedoingTransaction => "Redoing operation",
        OperationStage::Restacking => "Restacking",
        OperationStage::Pushing => "Pushing",
        OperationStage::UpdatingPullRequests => "Updating pull requests",
        OperationStage::ResolvingPullRequest => "Resolving pull request",
    }
}

fn change_label(change: PullRequestChange) -> &'static str {
    match change {
        PullRequestChange::Created => "Created",
        PullRequestChange::Updated => "Updated",
        PullRequestChange::Unchanged => "Unchanged",
    }
}

#[allow(dead_code)]
fn transaction_status_label(status: TransactionStatus) -> &'static str {
    match status {
        TransactionStatus::InProgress => "in progress",
        TransactionStatus::Succeeded => "succeeded",
        TransactionStatus::Failed => "failed",
    }
}

fn repository_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Repository")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{RefreshState, WorkspaceView};
    use stax::application::{BranchSummary, RepositorySnapshot};
    use std::path::PathBuf;

    fn empty_snapshot() -> RepositorySnapshot {
        RepositorySnapshot {
            repository_root: PathBuf::from("/repo"),
            current_branch: "main".into(),
            trunk: "main".into(),
            branches: Vec::new(),
        }
    }

    #[test]
    fn refresh_failure_is_actionable_without_replacing_workspace_state() {
        let mut workspace = WorkspaceView::from_snapshot(empty_snapshot());
        workspace.begin_refresh();
        assert_eq!(workspace.refresh, RefreshState::Loading);

        workspace.fail_refresh("repository is temporarily unavailable".into());

        assert_eq!(
            workspace.inline_error(),
            Some("repository is temporarily unavailable")
        );
        assert_eq!(
            workspace.state().snapshot().repository_root,
            PathBuf::from("/repo")
        );
    }

    #[test]
    fn selecting_a_branch_beyond_the_viewport_requests_stack_scrolling() {
        let mut snapshot = empty_snapshot();
        snapshot.branches = (0..100)
            .map(|index| BranchSummary {
                name: format!("branch-{index:03}"),
                parent: (index > 0).then(|| format!("branch-{:03}", index - 1)),
                column: index,
                is_current: index == 0,
                is_trunk: index == 0,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            })
            .collect();
        snapshot.current_branch = "branch-000".into();
        let mut workspace = WorkspaceView::from_snapshot(snapshot);

        assert!(workspace.select_branch("branch-080"));

        assert_eq!(workspace.stack_scroll_target(), Some(80));
        assert_eq!(
            workspace.stack_scroll_handle().logical_scroll_top_index(),
            80
        );
    }

    #[test]
    fn initial_current_branch_is_requested_visible_in_a_long_stack() {
        let mut snapshot = empty_snapshot();
        snapshot.branches = (0..100)
            .map(|index| BranchSummary {
                name: format!("branch-{index:03}"),
                parent: (index > 0).then(|| format!("branch-{:03}", index - 1)),
                column: index,
                is_current: index == 80,
                is_trunk: index == 0,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            })
            .collect();
        snapshot.current_branch = "branch-080".into();

        let workspace = WorkspaceView::from_snapshot(snapshot);

        assert_eq!(workspace.stack_scroll_target(), Some(80));
        assert_eq!(
            workspace.stack_scroll_handle().logical_scroll_top_index(),
            80
        );
    }

    #[test]
    fn refresh_failure_takes_precedence_over_older_storage_notice() {
        let mut workspace = WorkspaceView::from_snapshot(empty_snapshot());
        workspace.set_storage_notice(Some("recent repository write failed earlier".into()));
        workspace.begin_refresh();

        workspace.fail_refresh("latest refresh could not read the repository".into());

        assert_eq!(
            workspace.inline_error(),
            Some("latest refresh could not read the repository")
        );
    }

    #[test]
    fn latest_action_notice_takes_precedence_over_an_older_refresh_failure() {
        let mut workspace = WorkspaceView::from_snapshot(empty_snapshot());
        workspace.begin_refresh();
        workspace.fail_refresh("older refresh failure".into());

        workspace.set_notice("latest picker failure".into());

        assert_eq!(workspace.inline_error(), Some("latest picker failure"));
    }

    #[test]
    fn beginning_a_refresh_clears_the_previous_action_notice() {
        let mut workspace = WorkspaceView::from_snapshot(empty_snapshot());
        workspace.set_notice("old picker failure".into());

        workspace.begin_refresh();

        assert_eq!(workspace.inline_error(), None);
    }
}
