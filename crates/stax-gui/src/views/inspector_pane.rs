use super::{
    ControlKind, WorkspaceView, activate_control,
    app::{
        DeleteSelected, MoveSelected, OpenPullRequest, RenameSelected, ReorderSelectedStack,
        RestackSelected,
    },
    control_button,
};
use crate::state::LoadState;
use crate::theme::{MONOSPACE_FONT, Theme};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    Div, Hsla, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _,
    Styled as _, div, px,
};
use stax::application::{BranchDetails, BranchSummary, CiSummary};

pub const PANE_MARKER: &str = "stax-inspector-pane";
pub const PANE_HEADING: &str = "Inspector";

pub fn render(
    workspace: &WorkspaceView,
    theme: Theme,
    cx: &mut gpui::Context<super::AppView>,
) -> Div {
    let selected = workspace.state().selected_branch().and_then(|selected| {
        workspace
            .state()
            .snapshot()
            .branches
            .iter()
            .find(|branch| branch.name == selected)
    });

    let content = match selected {
        Some(branch) => render_selected(workspace, branch, theme, cx),
        None => div()
            .flex()
            .flex_1()
            .items_center()
            .justify_center()
            .p_5()
            .text_center()
            .text_sm()
            .text_color(theme.text_muted)
            .child("No branch is available to inspect."),
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
                .px_3()
                .border_b_1()
                .border_color(theme.border)
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(PANE_HEADING),
                ),
        )
        .child(
            div()
                .id("inspector-scroll")
                .flex()
                .flex_1()
                .min_h_0()
                .flex_col()
                .overflow_y_scroll()
                .p_3()
                .child(
                    div()
                        .debug_selector(|| "inspector-card".into())
                        .w_full()
                        .min_w_0()
                        .child(content),
                ),
        )
}

fn render_selected(
    workspace: &WorkspaceView,
    branch: &BranchSummary,
    theme: Theme,
    cx: &mut gpui::Context<super::AppView>,
) -> Div {
    let branch_context = if branch.is_trunk {
        "Trunk"
    } else if branch.is_current {
        "Current branch"
    } else {
        "Selected branch"
    };

    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .debug_selector(|| "inspector-branch-strip".into())
                .w_full()
                .min_w_0()
                .flex()
                .bg(theme.accent.alpha(0.08))
                .child(div().flex_none().w(px(3.0)).bg(theme.accent))
                .child(
                    div()
                        .w_full()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .p_3()
                        .child(
                            div()
                                .w_full()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .debug_selector(|| "inspector-branch-identity".into())
                                        .min_w_0()
                                        .truncate()
                                        .font_family(MONOSPACE_FONT)
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(theme.accent)
                                        .child(branch.name.clone()),
                                )
                                .child(status_badge(branch_context, theme.accent, theme)),
                        )
                        .child(
                            div()
                                .debug_selector(|| "inspector-parent-identity".into())
                                .w_full()
                                .min_w_0()
                                .truncate()
                                .font_family(MONOSPACE_FONT)
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(format!(
                                    "Parent: {}",
                                    branch.parent.as_deref().unwrap_or("No parent (trunk)")
                                )),
                        ),
                ),
        )
        .child(render_branch_health(branch, theme))
        .child(render_details(workspace.state().details(), theme))
        .child(render_pull_request(branch, theme))
        .child(render_actions(workspace, theme, cx))
        .child(render_ci(workspace.state().ci(), branch, theme))
}

fn render_branch_health(branch: &BranchSummary, theme: Theme) -> Div {
    let (label, color) = if branch.is_trunk {
        ("Trunk branch", theme.text_muted)
    } else if branch.needs_restack {
        ("Restack needed", theme.warning)
    } else {
        ("Restack health: up to date", theme.success)
    };

    status_section(
        "inspector-health-section",
        "Stack health",
        label,
        color,
        theme,
    )
}

fn render_details(details: &LoadState<BranchDetails>, theme: Theme) -> Div {
    let body = match details {
        LoadState::Idle => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .child("Branch details not loaded.")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Select a branch or refresh the repository to load details."),
            ),
        LoadState::Loading => div()
            .text_sm()
            .text_color(theme.text_muted)
            .child("Loading divergence and commits…"),
        LoadState::Failed(error) => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .text_color(theme.danger)
            .child("Branch details failed to load.")
            .child(error.clone())
            .child("Use Refresh Repository to retry."),
        LoadState::Ready(details) => {
            let mut ready = div()
                .flex()
                .flex_col()
                .gap_3()
                .text_sm()
                .child(
                    div()
                        .debug_selector(|| "inspector-metrics".into())
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(metric_row(
                            metric("Ahead", details.ahead, theme.accent, theme),
                            metric(
                                "Behind",
                                details.behind,
                                if details.behind == 0 {
                                    theme.success
                                } else {
                                    theme.warning
                                },
                                theme,
                            ),
                        ))
                        .child(metric_row(
                            metric(
                                "Unpushed",
                                details.unpushed,
                                if details.unpushed == 0 {
                                    theme.text_muted
                                } else {
                                    theme.warning
                                },
                                theme,
                            ),
                            metric(
                                "Unpulled",
                                details.unpulled,
                                if details.unpulled == 0 {
                                    theme.success
                                } else {
                                    theme.danger
                                },
                                theme,
                            ),
                        )),
                )
                .when(!details.has_remote, |ready| {
                    ready.child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("This branch has not been published to a remote."),
                    )
                })
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(format!("Commits · {}", details.commits.len())),
                );
            if details.commits.is_empty() {
                ready = ready.child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child("No commits ahead of parent."),
                );
            } else {
                ready = ready.children(details.commits.iter().map(|commit| {
                    div()
                        .pl_2()
                        .py_1()
                        .font_family(MONOSPACE_FONT)
                        .text_xs()
                        .text_color(theme.text)
                        .child(commit.clone())
                }));
            }
            ready
        }
    };

    section(
        "inspector-details-section",
        "Branch details",
        theme,
        theme.accent,
        body,
    )
}

fn render_pull_request(branch: &BranchSummary, theme: Theme) -> Div {
    let (body, color) = match branch.pr_number {
        Some(number) => {
            let state = branch.pr_state.as_deref().unwrap_or("state unavailable");
            let color = match state.to_ascii_lowercase().as_str() {
                "merged" => theme.success,
                "closed" => theme.danger,
                _ => theme.accent,
            };
            (
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .font_family(MONOSPACE_FONT)
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(format!("PR #{number}")),
                    )
                    .child(status_badge(state, color, theme)),
                color,
            )
        }
        None => (
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .child("No pull request cached.")
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.text_muted)
                        .child("Submit the stack to publish this branch."),
                ),
            theme.border,
        ),
    };
    section("inspector-pr-section", "Pull request", theme, color, body)
}

fn render_ci(ci: &LoadState<CiSummary>, branch: &BranchSummary, theme: Theme) -> Div {
    let (body, color) = match ci {
        LoadState::Idle => {
            let status = branch.ci_state.as_deref().unwrap_or("Not loaded");
            let color = status_color(status, theme);
            (
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(status_badge(status, color, theme))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.text_muted)
                            .child("Refresh to load the latest checks."),
                    ),
                color,
            )
        }
        LoadState::Loading => (
            div()
                .text_sm()
                .text_color(theme.warning)
                .child("Loading CI checks…"),
            theme.warning,
        ),
        LoadState::Failed(error) => (
            div()
                .flex()
                .flex_col()
                .gap_1()
                .text_sm()
                .text_color(theme.danger)
                .child("CI checks are unavailable.")
                .child(error.clone())
                .child("Resolve the message above, then refresh the repository."),
            theme.danger,
        ),
        LoadState::Ready(summary) => {
            let status = summary.overall_status.as_deref().unwrap_or("unknown");
            let color = status_color(status, theme);
            (
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(status_badge(status, color, theme))
                    .child(metric_row(
                        metric("Passed", summary.passed, theme.success, theme),
                        metric("Failed", summary.failed, theme.danger, theme),
                    ))
                    .child(metric_row(
                        metric("Running", summary.running, theme.warning, theme),
                        metric("Queued", summary.queued, theme.text_muted, theme),
                    ))
                    .when(summary.skipped > 0, |body| {
                        body.child(
                            div()
                                .text_xs()
                                .text_color(theme.text_muted)
                                .child(format!("{} skipped", summary.skipped)),
                        )
                    }),
                color,
            )
        }
    };
    section(
        "inspector-ci-section",
        "Continuous integration",
        theme,
        color,
        body,
    )
}

fn render_actions(
    workspace: &WorkspaceView,
    theme: Theme,
    cx: &mut gpui::Context<super::AppView>,
) -> Div {
    let actions = workspace.state().interaction_state();
    let checkout = control_button(
        "inspector-checkout",
        control_label("Checkout", &actions.checkout),
        ControlKind::Secondary,
        actions.checkout.enabled,
        theme,
    );
    let checkout = if actions.checkout.enabled {
        activate_control(checkout, cx, |app, window, cx| {
            app.checkout_selected_branch(window, cx);
        })
    } else {
        checkout
    };
    let restack = control_button(
        "inspector-restack",
        control_label("Restack", &actions.restack),
        ControlKind::Secondary,
        actions.restack.enabled,
        theme,
    );
    let restack = if actions.restack.enabled {
        activate_control(restack, cx, |app, window, cx| {
            app.restack_selected_action(&RestackSelected, window, cx);
        })
    } else {
        restack
    };
    let rename = control_button(
        "inspector-rename",
        control_label("Rename", &actions.rename),
        ControlKind::Secondary,
        actions.rename.enabled,
        theme,
    );
    let rename = if actions.rename.enabled {
        activate_control(rename, cx, |app, window, cx| {
            app.rename_action(&RenameSelected, window, cx);
        })
    } else {
        rename
    };
    let delete = control_button(
        "inspector-delete",
        control_label("Delete", &actions.delete),
        ControlKind::Secondary,
        actions.delete.enabled,
        theme,
    );
    let delete = if actions.delete.enabled {
        activate_control(delete, cx, |app, window, cx| {
            app.delete_action(&DeleteSelected, window, cx);
        })
    } else {
        delete
    };
    let move_subtree = control_button(
        "inspector-move",
        control_label("Move", &actions.move_subtree),
        ControlKind::Secondary,
        actions.move_subtree.enabled,
        theme,
    );
    let move_subtree = if actions.move_subtree.enabled {
        activate_control(move_subtree, cx, |app, window, cx| {
            app.move_action(&MoveSelected, window, cx);
        })
    } else {
        move_subtree
    };
    let reorder = control_button(
        "inspector-reorder",
        control_label("Reorder stack", &actions.reorder),
        ControlKind::Secondary,
        actions.reorder.enabled,
        theme,
    );
    let reorder = if actions.reorder.enabled {
        activate_control(reorder, cx, |app, window, cx| {
            app.reorder_action(&ReorderSelectedStack, window, cx);
        })
    } else {
        reorder
    };
    let open_pr = control_button(
        "inspector-open-pr",
        control_label("Open PR", &actions.open_pr),
        ControlKind::Primary,
        actions.open_pr.enabled,
        theme,
    );
    let open_pr = if actions.open_pr.enabled {
        activate_control(open_pr, cx, |app, window, cx| {
            app.open_pull_request_action(&OpenPullRequest, window, cx);
        })
    } else {
        open_pr
    };

    section(
        "inspector-actions-section",
        "Actions",
        theme,
        theme.accent,
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(checkout)
            .child(rename)
            .child(delete)
            .child(move_subtree)
            .child(reorder)
            .child(restack)
            .child(open_pr),
    )
}

pub(super) fn control_label(
    label: &str,
    availability: &crate::state::ActionAvailability,
) -> String {
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

fn section(selector: &'static str, title: &str, theme: Theme, accent: Hsla, body: Div) -> Div {
    div()
        .debug_selector(move || selector.into())
        .flex()
        .flex_col()
        .gap_2()
        .border_t_1()
        .border_color(theme.border)
        .pt_3()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(div().w(px(3.0)).h(px(12.0)).bg(accent))
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(title.to_string()),
                ),
        )
        .child(body)
}

fn status_section(
    selector: &'static str,
    title: &str,
    label: &str,
    color: Hsla,
    theme: Theme,
) -> Div {
    section(
        selector,
        title,
        theme,
        color,
        div()
            .text_sm()
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(color)
            .child(label.to_string()),
    )
}

fn status_badge(label: &str, color: Hsla, theme: Theme) -> Div {
    div()
        .flex_none()
        .rounded_sm()
        .border_1()
        .border_color(color.alpha(0.5))
        .bg(color.alpha(0.12))
        .px_2()
        .py_1()
        .text_xs()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(color)
        .child(label.to_string())
        .when(label.is_empty(), |badge| badge.text_color(theme.text_muted))
}

fn metric_row(left: Div, right: Div) -> Div {
    div().flex().gap_4().child(left).child(right)
}

fn metric(label: &str, value: usize, color: Hsla, theme: Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .items_center()
        .gap_2()
        .py_1()
        .child(
            div()
                .font_family(MONOSPACE_FONT)
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(color)
                .child(value.to_string()),
        )
        .child(
            div()
                .text_xs()
                .text_color(theme.text_muted)
                .child(label.to_string()),
        )
}

fn status_color(status: &str, theme: Theme) -> Hsla {
    let status = status.to_ascii_lowercase();
    if status.contains("success") || status.contains("pass") {
        theme.success
    } else if status.contains("fail") || status.contains("error") {
        theme.danger
    } else if status.contains("run") || status.contains("queue") || status.contains("pending") {
        theme.warning
    } else {
        theme.text_muted
    }
}

#[cfg(test)]
mod tests {
    use super::{render_ci, render_details};
    use crate::state::LoadState;
    use crate::theme::Theme;
    use stax::application::{BranchDetails, BranchSummary, CiSummary};

    #[test]
    fn every_details_load_state_builds_without_panicking() {
        let theme = Theme::light();
        let states = [
            LoadState::Idle,
            LoadState::Loading,
            LoadState::Failed("offline".into()),
            LoadState::Ready(BranchDetails {
                ahead: 1,
                behind: 2,
                has_remote: true,
                unpushed: 1,
                unpulled: 0,
                commits: vec!["Add cockpit".into()],
            }),
        ];

        for state in states {
            let _ = render_details(&state, theme);
        }
    }

    #[test]
    fn every_ci_load_state_builds_without_panicking() {
        let theme = Theme::dark();
        let branch = BranchSummary {
            name: "feature".into(),
            parent: Some("main".into()),
            column: 0,
            is_current: true,
            is_trunk: false,
            needs_restack: false,
            pr_number: None,
            pr_state: None,
            ci_state: Some("success".into()),
        };
        let states = [
            LoadState::Idle,
            LoadState::Loading,
            LoadState::Failed("authentication required".into()),
            LoadState::Ready(CiSummary {
                overall_status: Some("success".into()),
                total: 2,
                passed: 2,
                failed: 0,
                running: 0,
                queued: 0,
                skipped: 0,
                started_at: None,
                completed_at: None,
                average_secs: Some(60),
            }),
        ];

        for state in states {
            let _ = render_ci(&state, &branch, theme);
        }
    }
}
