use super::{AppView, WorkspaceView};
use crate::state::LoadState;
use crate::theme::{MONOSPACE_FONT, Theme};
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, SharedString, Styled as _, div, px,
    uniform_list,
};
use stax::application::{BranchDiff, DiffLineKind};
use std::ops::Range;

pub const PANE_MARKER: &str = "stax-changes-pane";
pub const PANE_HEADING: &str = "Changes";
const DIFFSTAT_ROW_HEIGHT: f32 = 24.0;
const DIFFSTAT_MAX_VISIBLE_ROWS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DiffstatLayout {
    total_rows: usize,
    visible_rows: usize,
    is_scrollable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffDisplayState {
    Idle,
    Loading,
    Failed,
    ReadyEmpty,
    Ready,
}

pub fn render(workspace: &WorkspaceView, theme: Theme, cx: &mut Context<AppView>) -> Div {
    let selected = workspace
        .state()
        .selected_branch()
        .unwrap_or("No branch selected");
    let state = diff_display_state(workspace.state().diff());
    let mut heading_status = div().min_w_0().flex().items_center().justify_end().gap_2();
    if workspace.state().diff_is_refreshing() {
        heading_status = heading_status.child(
            div()
                .debug_selector(|| "changes-refreshing".into())
                .flex_none()
                .text_xs()
                .text_color(theme.text_muted)
                .child("Refreshing…"),
        );
    }
    heading_status = heading_status.child(
        div()
            .min_w_0()
            .truncate()
            .font_family(MONOSPACE_FONT)
            .text_xs()
            .text_color(theme.text_muted)
            .child(selected.to_string()),
    );

    let body = match (state, workspace.state().diff()) {
        (DiffDisplayState::Idle, _) => state_message(
            "Changes are not loaded",
            "Select a branch or refresh the repository to load its patch.",
            theme,
        ),
        (DiffDisplayState::Loading, _) => state_message(
            "Loading changes…",
            "The stack remains available while the patch loads.",
            theme,
        ),
        (DiffDisplayState::Failed, LoadState::Failed(error)) => state_message(
            "Changes could not be loaded",
            &format!("{error} · Use Refresh Repository to retry."),
            theme,
        ),
        (DiffDisplayState::ReadyEmpty, _) => state_message(
            "No changes against parent",
            "This branch currently has an empty diff.",
            theme,
        ),
        (DiffDisplayState::Ready, LoadState::Ready(diff)) => render_ready_diff(diff, theme, cx),
        _ => state_message("Changes unavailable", "Refresh Repository to retry.", theme),
    };

    div()
        .debug_selector(|| PANE_MARKER.into())
        .size_full()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.window)
        .child(
            div()
                .h(px(44.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .px_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(PANE_HEADING),
                )
                .child(heading_status),
        )
        .child(body)
}

fn render_ready_diff(diff: &BranchDiff, theme: Theme, cx: &mut Context<AppView>) -> Div {
    let line_count = diff.lines.len();
    let additions: usize = diff.stat.iter().map(|line| line.additions).sum();
    let deletions: usize = diff.stat.iter().map(|line| line.deletions).sum();
    let diffstat_layout = diffstat_layout(diff);
    let file_summary = if diffstat_layout.is_scrollable {
        format!(
            "{} changed files · scroll summary",
            diffstat_layout.total_rows
        )
    } else {
        format!("{} changed files", diffstat_layout.total_rows)
    };

    let mut diffstat = div()
        .debug_selector(|| "changes-file-summary".into())
        .flex_none()
        .flex()
        .flex_col()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(file_summary)
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .child(
                            div()
                                .text_color(theme.diff_addition)
                                .child(format!("+{additions} additions")),
                        )
                        .child(
                            div()
                                .text_color(theme.diff_deletion)
                                .child(format!("−{deletions} deletions")),
                        ),
                ),
        );

    if diff.stat.is_empty() {
        diffstat = diffstat.child(
            div()
                .px_3()
                .pb_2()
                .text_xs()
                .text_color(theme.text_muted)
                .child("No file summary available."),
        );
    } else {
        diffstat = diffstat.child(
            uniform_list(
                "changes-diffstat-files",
                diffstat_layout.total_rows,
                cx.processor(move |app, range: Range<usize>, _window, _cx| {
                    let Some(workspace) = app.workspace() else {
                        return Vec::new();
                    };
                    let LoadState::Ready(diff) = workspace.state().diff() else {
                        return Vec::new();
                    };

                    range
                        .filter_map(|index| diff.stat.get(index).map(|line| (index, line)))
                        .map(|(index, line)| {
                            div()
                                .id(SharedString::from(format!("diffstat-file-{index}")))
                                .h(px(DIFFSTAT_ROW_HEIGHT))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .px_3()
                                .font_family(MONOSPACE_FONT)
                                .text_xs()
                                .child(div().min_w_0().truncate().child(line.file.clone()))
                                .child(
                                    div()
                                        .flex_none()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            div()
                                                .text_color(theme.diff_addition)
                                                .child(format!("+{}", line.additions)),
                                        )
                                        .child(
                                            div()
                                                .text_color(theme.diff_deletion)
                                                .child(format!("−{}", line.deletions)),
                                        ),
                                )
                        })
                        .collect()
                }),
            )
            .h(px(diffstat_layout.visible_rows as f32 * DIFFSTAT_ROW_HEIGHT))
            .flex_none(),
        );
    }

    div()
        .flex()
        .flex_1()
        .min_h_0()
        .flex_col()
        .gap_3()
        .px_3()
        .pb_3()
        .child(diffstat)
        .child(
            uniform_list(
                "changes-patch-lines",
                line_count,
                cx.processor(move |app, range: Range<usize>, _window, _cx| {
                    let Some(workspace) = app.workspace() else {
                        return Vec::new();
                    };
                    let LoadState::Ready(diff) = workspace.state().diff() else {
                        return Vec::new();
                    };

                    range
                        .filter_map(|index| diff.lines.get(index).map(|line| (index, line)))
                        .map(|(index, line)| {
                            div()
                                .id(SharedString::from(format!("patch-line-{index}")))
                                .h(px(21.0))
                                .w_full()
                                .px_3()
                                .font_family(MONOSPACE_FONT)
                                .text_xs()
                                .text_color(diff_line_color(line.kind, theme))
                                .whitespace_nowrap()
                                .child(line.content.clone())
                        })
                        .collect()
                }),
            )
            .flex_1()
            .min_h_0()
            .bg(theme.window),
        )
}

fn diffstat_layout(diff: &BranchDiff) -> DiffstatLayout {
    let total_rows = diff.stat.len();
    let visible_rows = total_rows.min(DIFFSTAT_MAX_VISIBLE_ROWS);
    DiffstatLayout {
        total_rows,
        visible_rows,
        is_scrollable: total_rows > visible_rows,
    }
}

fn state_message(title: &str, detail: &str, theme: Theme) -> Div {
    div()
        .flex()
        .flex_1()
        .min_h_0()
        .items_center()
        .justify_center()
        .p_5()
        .child(
            div()
                .max_w(px(340.0))
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_center()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(detail.to_string()),
                ),
        )
}

fn diff_display_state(state: &LoadState<BranchDiff>) -> DiffDisplayState {
    match state {
        LoadState::Idle => DiffDisplayState::Idle,
        LoadState::Loading => DiffDisplayState::Loading,
        LoadState::Failed(_) => DiffDisplayState::Failed,
        LoadState::Ready(diff) if diff.stat.is_empty() && diff.lines.is_empty() => {
            DiffDisplayState::ReadyEmpty
        }
        LoadState::Ready(_) => DiffDisplayState::Ready,
    }
}

fn diff_line_color(kind: DiffLineKind, theme: Theme) -> gpui::Hsla {
    match kind {
        DiffLineKind::Addition => theme.diff_addition,
        DiffLineKind::Deletion => theme.diff_deletion,
        DiffLineKind::Hunk => theme.diff_hunk,
        DiffLineKind::Header => theme.accent,
        DiffLineKind::Context => theme.text_muted,
    }
}

#[cfg(test)]
mod tests {
    use super::{DIFFSTAT_MAX_VISIBLE_ROWS, DiffDisplayState, diff_display_state, diffstat_layout};
    use crate::state::LoadState;
    use stax::application::{BranchDiff, DiffLine, DiffLineKind, DiffStatLine};

    #[test]
    fn diff_load_states_have_stable_distinct_presentations() {
        assert_eq!(diff_display_state(&LoadState::Idle), DiffDisplayState::Idle);
        assert_eq!(
            diff_display_state(&LoadState::Loading),
            DiffDisplayState::Loading
        );
        assert_eq!(
            diff_display_state(&LoadState::Failed("offline".into())),
            DiffDisplayState::Failed
        );
        assert_eq!(
            diff_display_state(&LoadState::Ready(BranchDiff {
                stat: Vec::new(),
                lines: Vec::new(),
            })),
            DiffDisplayState::ReadyEmpty
        );
        assert_eq!(
            diff_display_state(&LoadState::Ready(BranchDiff {
                stat: Vec::new(),
                lines: vec![DiffLine {
                    content: "+new".into(),
                    kind: DiffLineKind::Addition,
                }],
            })),
            DiffDisplayState::Ready
        );
    }

    #[test]
    fn large_diffstat_is_bounded_and_keeps_the_patch_region_available() {
        let diff = BranchDiff {
            stat: (0..500)
                .map(|index| DiffStatLine {
                    file: format!("src/file-{index:03}.rs"),
                    additions: index,
                    deletions: index / 2,
                })
                .collect(),
            lines: vec![DiffLine {
                content: "+patch remains visible".into(),
                kind: DiffLineKind::Addition,
            }],
        };

        let layout = diffstat_layout(&diff);

        assert_eq!(layout.total_rows, 500);
        assert_eq!(layout.visible_rows, DIFFSTAT_MAX_VISIBLE_ROWS);
        assert!(layout.is_scrollable);
    }
}
