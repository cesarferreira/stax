#[cfg(test)]
use super::PaneMarkers;
use super::{
    AppView, ControlKind, activate_control, changes_pane, control_button, inspector_pane,
    stack_pane,
};
use crate::state::{SelectionDirection, WorkspaceState};
use crate::theme::{SYSTEM_UI_FONT, Theme};
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, ScrollStrategy, Styled as _,
    UniformListScrollHandle, div, px, relative,
};
use stax::application::{
    BranchDetails, BranchDiff, BranchSummary, CiSummary, DetailRequestToken, RepositorySnapshot,
};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshState {
    Idle,
    Loading,
    Failed(String),
}

#[derive(Debug, Clone)]
pub struct WorkspaceView {
    state: WorkspaceState,
    refresh: RefreshState,
    notice: Option<String>,
    storage_notice: Option<String>,
    stack_scroll_handle: UniformListScrollHandle,
    stack_scroll_target: Option<usize>,
}

impl WorkspaceView {
    pub fn from_snapshot(snapshot: RepositorySnapshot) -> Self {
        let mut workspace = Self {
            state: WorkspaceState::new(snapshot),
            refresh: RefreshState::Idle,
            notice: None,
            storage_notice: None,
            stack_scroll_handle: UniformListScrollHandle::new(),
            stack_scroll_target: None,
        };
        workspace.scroll_selection_into_view();
        workspace
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

    pub fn render(&self, theme: Theme, cx: &mut Context<AppView>) -> Div {
        div()
            .debug_selector(|| "stax-workspace".into())
            .size_full()
            .min_w_0()
            .flex()
            .flex_col()
            .font_family(SYSTEM_UI_FONT)
            .bg(theme.window)
            .text_color(theme.text)
            .child(self.render_toolbar(theme, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .child(
                        stack_pane::render(self, theme, cx)
                            .w(relative(0.29))
                            .flex_none(),
                    )
                    .child(
                        changes_pane::render(self, theme, cx)
                            .w(relative(0.43))
                            .flex_none(),
                    )
                    .child(
                        inspector_pane::render(self, theme, cx)
                            .w(relative(0.28))
                            .flex_none(),
                    ),
            )
    }

    fn render_toolbar(&self, theme: Theme, cx: &mut Context<AppView>) -> Div {
        let snapshot = self.state.snapshot();
        let repository_name = repository_name(&snapshot.repository_root);
        let refresh_label = match &self.refresh {
            RefreshState::Idle => "Refresh Repository",
            RefreshState::Loading => "Refreshing…",
            RefreshState::Failed(_) => "Retry Refresh",
        };
        let refresh_enabled = self.refresh != RefreshState::Loading;

        let refresh = control_button(
            "toolbar-refresh",
            refresh_label,
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

        let open = activate_control(
            control_button(
                "toolbar-open",
                "Open Repository",
                ControlKind::Secondary,
                true,
                theme,
            ),
            cx,
            |app, window, cx| app.pick_repository(window, cx),
        );

        let disabled_action = control_button(
            "toolbar-submit-stack",
            "Submit Stack — Phase 2",
            ControlKind::Secondary,
            false,
            theme,
        );

        let mut toolbar = div()
            .h(px(58.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_4()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.surface_raised)
            .child(
                div()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(repository_name),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_xs()
                                    .text_color(theme.text_muted)
                                    .child(snapshot.repository_root.display().to_string()),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.border)
                            .bg(theme.surface)
                            .text_xs()
                            .child(format!("Current branch: {}", snapshot.current_branch)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(theme.accent)
                            .bg(theme.surface_selected)
                            .text_xs()
                            .text_color(theme.accent)
                            .child("Read-only · Phase 1"),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(disabled_action)
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

    fn scroll_selection_into_view(&mut self) {
        let Some(selected) = self.state.selected_branch() else {
            return;
        };
        let Some(index) = self
            .state
            .snapshot()
            .branches
            .iter()
            .position(|branch| branch.name == selected)
        else {
            return;
        };

        self.stack_scroll_target = Some(index);
        self.stack_scroll_handle
            .scroll_to_item(index, ScrollStrategy::Center);
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
