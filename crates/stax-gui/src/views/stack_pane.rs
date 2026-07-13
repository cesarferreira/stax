use super::stack_topology::{TopologyRow, layout as layout_topology};
use super::text_input::BranchNameInput;
use super::{AppView, WorkspaceView, activate_control, control_focus_style};
use crate::theme::{MONOSPACE_FONT, Theme};
use gpui::{
    Context, Div, Entity, InteractiveElement as _, ParentElement as _, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, div, px, uniform_list,
};
use stax::application::BranchSummary;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

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

pub fn render(
    workspace: &WorkspaceView,
    search_input: Option<Entity<BranchNameInput>>,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Div {
    let branch_count = workspace.state().filtered_branches().len();
    let topology = Arc::new(
        layout_topology(&workspace.state().snapshot().branches)
            .into_iter()
            .map(|row| (row.branch_name.clone(), row))
            .collect::<HashMap<_, _>>(),
    );
    let topology_width = topology
        .values()
        .next()
        .map(|row| {
            row.segments
                .iter()
                .map(|segment| segment.glyph.chars().count())
                .sum::<usize>()
        })
        .unwrap_or(1);

    let mut pane = div()
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
        );
    if let Some(search_input) = search_input {
        pane = pane.child(
            div()
                .debug_selector(|| "stack-search".into())
                .h(px(42.0))
                .flex_none()
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .border_b_1()
                .border_color(theme.border)
                .bg(theme.surface_raised)
                .font_family(MONOSPACE_FONT)
                .text_sm()
                .text_color(theme.text_muted)
                .child("/")
                .child(
                    div()
                        .min_w_0()
                        .flex_1()
                        .text_color(theme.text)
                        .child(search_input),
                ),
        );
    }
    if branch_count == 0 {
        pane.child(
            div()
                .debug_selector(|| "stack-search-no-results".into())
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(theme.text_muted)
                .child("No branches match this search"),
        )
    } else {
        pane.child(
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
                                .filtered_branches()
                                .get(index)
                                .cloned()
                                .cloned()
                                .map(|branch| (index, branch))
                        })
                        .collect();

                    rows.into_iter()
                        .map(|(index, branch)| {
                            let topology_row = topology.get(&branch.name).cloned();
                            render_branch_row(
                                branch,
                                topology_row,
                                topology_width,
                                selected.as_deref(),
                                index,
                                theme,
                                cx,
                            )
                        })
                        .collect()
                }),
            )
            .track_scroll(workspace.stack_scroll_handle().clone())
            .flex_1()
            .min_h_0(),
        )
    }
}

fn render_branch_row(
    branch: BranchSummary,
    topology: Option<TopologyRow>,
    topology_width: usize,
    selected: Option<&str>,
    branch_index: usize,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Stateful<Div> {
    let is_selected = selected == Some(branch.name.as_str());
    let branch_name = branch.name.clone();
    let statuses = branch_status_parts(&branch);

    let row =
        div()
            .id(SharedString::from(format!("stack-branch-{}", branch.name)))
            .debug_selector(|| format!("stack-branch-{}", branch.name))
            .focusable()
            .tab_index(branch_index as isize + 20)
            .focus(move |style| control_focus_style(style, theme))
            .h(px(54.0))
            .w_full()
            .min_w_0()
            .flex()
            .items_center()
            .gap_2()
            .pl_2()
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
                    .debug_selector(|| "stack-topology-gutter".into())
                    .flex_none()
                    .w(px(topology_width as f32 * 7.5))
                    .flex()
                    .items_center()
                    .font_family(MONOSPACE_FONT)
                    .text_xs()
                    .children(topology.into_iter().flat_map(|row| row.segments).map(
                        move |segment| {
                            div()
                                .flex_none()
                                .text_color(
                                    segment
                                        .lane
                                        .map(|lane| topology_color(lane, theme))
                                        .unwrap_or(theme.text_muted),
                                )
                                .child(segment.glyph)
                        },
                    )),
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
            );

    activate_control(row, cx, move |app, window, cx| {
        app.select_branch(&branch_name, window, cx);
    })
}

fn topology_color(lane: usize, theme: Theme) -> gpui::Hsla {
    [theme.accent, theme.success, theme.warning][lane % 3]
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
    use super::{StatusTone, branch_status_parts};
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
}
