use super::stack_topology::{TopologyCell, TopologyNode, TopologyRow, layout as layout_topology};
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
const BRANCH_ROW_HEIGHT: f32 = 48.0;
const TOPOLOGY_LANE_WIDTH: f32 = 16.0;
const TOPOLOGY_RAIL_WIDTH: f32 = 1.0;
const TOPOLOGY_NODE_SIZE: f32 = 7.0;

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
    let filtered_branches = workspace
        .state()
        .filtered_branches()
        .iter()
        .map(|branch| (*branch).clone())
        .collect::<Vec<_>>();
    let branch_count = filtered_branches.len();
    let topology = Arc::new(topology_for_filtered_branches(
        &workspace.state().snapshot().branches,
        &filtered_branches,
    ));
    let topology_width = topology
        .values()
        .next()
        .map(|row| row.cells.len())
        .unwrap_or(1);

    let mut pane = div()
        .debug_selector(|| PANE_MARKER.into())
        .size_full()
        .min_w_0()
        .flex()
        .flex_col()
        .bg(theme.sidebar)
        .child(
            div()
                .h(px(46.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_between()
                .px_3()
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
                .h(px(44.0))
                .flex_none()
                .flex()
                .items_center()
                .px_2()
                .child(
                    div()
                        .h(px(32.0))
                        .w_full()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .rounded_lg()
                        .bg(theme.surface_hover)
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

fn topology_for_filtered_branches(
    full_branches: &[BranchSummary],
    filtered_branches: &[BranchSummary],
) -> HashMap<String, TopologyRow> {
    let full_topology = layout_topology(full_branches)
        .into_iter()
        .map(|row| (row.branch_name.clone(), row))
        .collect::<HashMap<_, _>>();

    filtered_branches
        .iter()
        .filter_map(|branch| {
            full_topology
                .get(&branch.name)
                .cloned()
                .map(|row| (branch.name.clone(), row))
        })
        .collect()
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

    let row = div()
        .id(SharedString::from(format!("stack-branch-{}", branch.name)))
        .debug_selector(|| format!("stack-branch-{}", branch.name))
        .focusable()
        .tab_index(branch_index as isize + 20)
        .focus(move |style| control_focus_style(style, theme))
        .h(px(BRANCH_ROW_HEIGHT))
        .mx_2()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded_lg()
        .border_1()
        .border_color(if is_selected {
            theme.surface_selected
        } else {
            theme.sidebar
        })
        .bg(if is_selected {
            theme.surface_selected
        } else {
            theme.sidebar
        })
        .cursor_pointer()
        .hover(move |style| style.bg(theme.surface_hover))
        .child(
            div()
                .debug_selector(|| "stack-topology-gutter".into())
                .flex_none()
                .w(px(topology_width as f32 * TOPOLOGY_LANE_WIDTH))
                .h_full()
                .flex()
                .children(
                    topology
                        .into_iter()
                        .flat_map(|row| row.cells)
                        .map(move |cell| render_topology_cell(cell, theme, is_selected)),
                ),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_1()
                .flex_col()
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
                .child(div().flex().items_center().gap_1().truncate().children(
                    statuses.into_iter().map(|(label, tone)| {
                        div()
                            .text_xs()
                            .text_color(status_color(tone, theme))
                            .child(label)
                    }),
                )),
        );

    activate_control(row, cx, move |app, window, cx| {
        app.select_branch(&branch_name, window, cx);
    })
}

fn render_topology_cell(cell: TopologyCell, theme: Theme, selected: bool) -> Div {
    let color = theme.topology_lane(cell.lane);
    let center_x = (TOPOLOGY_LANE_WIDTH - TOPOLOGY_RAIL_WIDTH) / 2.0;
    let center_y = (BRANCH_ROW_HEIGHT - TOPOLOGY_RAIL_WIDTH) / 2.0;
    let half_lane = TOPOLOGY_LANE_WIDTH / 2.0;
    let half_row = BRANCH_ROW_HEIGHT / 2.0;
    let mut lane = div()
        .debug_selector(|| "stack-topology-cell".into())
        .relative()
        .flex_none()
        .w(px(TOPOLOGY_LANE_WIDTH))
        .h(px(BRANCH_ROW_HEIGHT));

    if cell.top {
        lane = lane.child(
            div()
                .debug_selector(|| "stack-topology-vertical-rail".into())
                .absolute()
                .top_0()
                .left(px(center_x))
                .w(px(TOPOLOGY_RAIL_WIDTH))
                .h(px(half_row))
                .bg(color),
        );
    }
    if cell.bottom {
        lane = lane.child(
            div()
                .debug_selector(|| "stack-topology-vertical-rail".into())
                .absolute()
                .top(px(half_row))
                .left(px(center_x))
                .w(px(TOPOLOGY_RAIL_WIDTH))
                .bottom_0()
                .bg(color),
        );
    }
    if cell.left {
        lane = lane.child(
            div()
                .debug_selector(|| "stack-topology-horizontal-rail".into())
                .absolute()
                .top(px(center_y))
                .left_0()
                .w(px(half_lane))
                .h(px(TOPOLOGY_RAIL_WIDTH))
                .bg(color),
        );
    }
    if cell.right {
        lane = lane.child(
            div()
                .debug_selector(|| "stack-topology-horizontal-rail".into())
                .absolute()
                .top(px(center_y))
                .left(px(half_lane))
                .right_0()
                .h(px(TOPOLOGY_RAIL_WIDTH))
                .bg(color),
        );
    }
    if let Some(node) = cell.node {
        let node_background = if selected {
            theme.surface_selected
        } else {
            theme.sidebar
        };
        let mut node_view = div()
            .debug_selector(|| "stack-topology-node".into())
            .absolute()
            .top(px((BRANCH_ROW_HEIGHT - TOPOLOGY_NODE_SIZE) / 2.0))
            .left(px((TOPOLOGY_LANE_WIDTH - TOPOLOGY_NODE_SIZE) / 2.0))
            .size(px(TOPOLOGY_NODE_SIZE))
            .rounded_full()
            .border_1()
            .border_color(color)
            .bg(node_background);
        if node == TopologyNode::Current {
            node_view = node_view.bg(color);
        }
        lane = lane.child(node_view);
    }

    lane
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
    use super::{StatusTone, branch_status_parts, topology_for_filtered_branches};
    use crate::views::stack_topology::TopologyNode;
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
    fn filtering_reuses_rows_from_the_full_topology() {
        let full = vec![
            BranchSummary {
                name: "nested".into(),
                parent: Some("side".into()),
                column: 2,
                is_current: false,
                is_trunk: false,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            },
            BranchSummary {
                name: "side".into(),
                parent: Some("main".into()),
                column: 1,
                is_current: false,
                is_trunk: false,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            },
            BranchSummary {
                name: "main".into(),
                parent: None,
                column: 0,
                is_current: true,
                is_trunk: true,
                needs_restack: false,
                pr_number: None,
                pr_state: None,
                ci_state: None,
            },
        ];
        let filtered = vec![full[1].clone()];

        let rows = topology_for_filtered_branches(&full, &filtered);
        let side = rows.get("side").unwrap();
        assert!(side.cells[0].top && side.cells[0].bottom);
        assert_eq!(side.cells[1].node, Some(TopologyNode::Branch));
        assert!(side.cells[1].right);
        assert!(side.cells[2].top && side.cells[2].left);
    }
}
