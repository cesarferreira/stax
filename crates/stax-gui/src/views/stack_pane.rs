use super::{AppView, WorkspaceView};
use crate::theme::{MONOSPACE_FONT, Theme};
use gpui::{
    Context, Div, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px, uniform_list,
};
use stax::application::BranchSummary;
use std::ops::Range;

pub const PANE_MARKER: &str = "stax-stack-pane";
pub const PANE_HEADING: &str = "Stack";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusTone {
    Muted,
    Accent,
    Success,
    Warning,
    Danger,
}

pub fn render(workspace: &WorkspaceView, theme: Theme, cx: &mut Context<AppView>) -> Div {
    let branch_count = workspace.state().snapshot().branches.len();

    div()
        .debug_selector(|| PANE_MARKER.into())
        .size_full()
        .min_w_0()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.surface)
        .child(
            div()
                .h(px(43.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(PANE_HEADING),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child(format!("{branch_count} branches")),
                ),
        )
        .child(
            uniform_list(
                "stack-branch-list",
                branch_count,
                cx.processor(move |app, range: Range<usize>, _window, cx| {
                    let Some(workspace) = app.workspace() else {
                        return Vec::new();
                    };
                    let selected = workspace.state().selected_branch().map(str::to_string);
                    let rows: Vec<_> = range
                        .filter_map(|index| {
                            workspace
                                .state()
                                .snapshot()
                                .branches
                                .get(index)
                                .cloned()
                                .map(|branch| (index, branch))
                        })
                        .collect();

                    rows.into_iter()
                        .map(|(index, branch)| {
                            render_branch_row(branch, selected.as_deref(), index, theme, cx)
                        })
                        .collect()
                }),
            )
            .track_scroll(workspace.stack_scroll_handle().clone())
            .flex_1()
            .min_h_0(),
        )
}

fn render_branch_row(
    branch: BranchSummary,
    selected: Option<&str>,
    branch_index: usize,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let is_selected = selected == Some(branch.name.as_str());
    let branch_name = branch.name.clone();
    let topology = topology_label(&branch);
    let statuses = branch_status_parts(&branch);
    let indentation = px(10.0 + branch.column.min(8) as f32 * 14.0);

    div()
        .id(SharedString::from(format!("stack-branch-{}", branch.name)))
        .focusable()
        .tab_index(branch_index as isize + 20)
        .focus(move |style| style.border_color(theme.focus))
        .h(px(54.0))
        .w_full()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .pl(indentation)
        .pr_2()
        .border_1()
        .border_color(if is_selected {
            theme.accent
        } else {
            theme.border
        })
        .bg(if is_selected {
            theme.surface_selected
        } else {
            theme.surface
        })
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_selected))
        .child(
            div()
                .flex_none()
                .w(px(38.0))
                .font_family(MONOSPACE_FONT)
                .text_xs()
                .text_color(if branch.is_current {
                    theme.accent
                } else {
                    theme.text_muted
                })
                .child(topology),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_1()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .truncate()
                        .font_family(MONOSPACE_FONT)
                        .text_sm()
                        .font_weight(if is_selected {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(branch.name.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .children(statuses.into_iter().map(|(label, tone)| {
                            div()
                                .text_xs()
                                .text_color(status_color(tone, theme))
                                .child(label)
                        })),
                ),
        )
        .on_click(cx.listener(move |app, _, _window, cx| {
            app.select_branch(&branch_name, cx);
        }))
}

fn topology_label(branch: &BranchSummary) -> String {
    let connector = if branch.is_trunk { "◆" } else { "●" };
    if branch.column == 0 {
        connector.to_string()
    } else {
        format!(
            "{}╰{connector}",
            "│".repeat(branch.column.saturating_sub(1))
        )
    }
}

fn branch_status_parts(branch: &BranchSummary) -> Vec<(String, StatusTone)> {
    let mut statuses = Vec::new();
    if branch.is_current {
        statuses.push(("Current".into(), StatusTone::Accent));
    }
    if branch.is_trunk {
        statuses.push(("Trunk".into(), StatusTone::Muted));
    }
    if branch.needs_restack {
        statuses.push(("Restack needed".into(), StatusTone::Warning));
    }
    if let Some(number) = branch.pr_number {
        let state = branch.pr_state.as_deref().unwrap_or("unknown");
        statuses.push((format!("PR #{number} · {state}"), StatusTone::Accent));
    }
    if let Some(state) = &branch.ci_state {
        let tone = match state.to_ascii_lowercase().as_str() {
            "success" | "passed" => StatusTone::Success,
            "failure" | "failed" | "error" => StatusTone::Danger,
            "pending" | "queued" | "running" | "in_progress" => StatusTone::Warning,
            _ => StatusTone::Muted,
        };
        statuses.push((format!("CI: {state}"), tone));
    }
    if statuses.is_empty() {
        statuses.push(("Local branch".into(), StatusTone::Muted));
    }
    statuses
}

fn status_color(tone: StatusTone, theme: Theme) -> gpui::Hsla {
    match tone {
        StatusTone::Muted => theme.text_muted,
        StatusTone::Accent => theme.accent,
        StatusTone::Success => theme.success,
        StatusTone::Warning => theme.warning,
        StatusTone::Danger => theme.danger,
    }
}

#[cfg(test)]
mod tests {
    use super::{StatusTone, branch_status_parts, topology_label};
    use stax::application::BranchSummary;

    fn branch() -> BranchSummary {
        BranchSummary {
            name: "feature".into(),
            parent: Some("main".into()),
            column: 2,
            is_current: true,
            is_trunk: false,
            needs_restack: true,
            pr_number: Some(17),
            pr_state: Some("open".into()),
            ci_state: Some("failure".into()),
        }
    }

    #[test]
    fn branch_metadata_is_expressed_with_text_not_color_alone() {
        let statuses = branch_status_parts(&branch());
        assert!(statuses.contains(&("Current".into(), StatusTone::Accent)));
        assert!(statuses.contains(&("Restack needed".into(), StatusTone::Warning)));
        assert!(statuses.contains(&("PR #17 · open".into(), StatusTone::Accent)));
        assert!(statuses.contains(&("CI: failure".into(), StatusTone::Danger)));
    }

    #[test]
    fn topology_uses_column_connectors() {
        assert_eq!(topology_label(&branch()), "│╰●");
    }
}
