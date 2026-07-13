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
use gpui::{
    Div, InteractiveElement as _, ParentElement as _, StatefulInteractiveElement as _, Styled as _,
    div, px,
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
                        .rounded_lg()
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_raised)
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
    div()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_4()
        .p_4()
        .child(
            div()
                .w_full()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .debug_selector(|| "inspector-branch-identity".into())
                        .w_full()
                        .min_w_0()
                        .truncate()
                        .font_family(MONOSPACE_FONT)
                        .text_sm()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(branch.name.clone()),
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
        )
        .child(render_branch_health(branch, theme))
        .child(render_details(workspace.state().details(), theme))
        .child(render_pull_request(branch, theme))
        .child(render_ci(workspace.state().ci(), branch, theme))
        .child(render_actions(workspace, theme, cx))
}

fn render_branch_health(branch: &BranchSummary, theme: Theme) -> Div {
    let (label, color) = if branch.is_trunk {
        ("Trunk branch", theme.text_muted)
    } else if branch.needs_restack {
        ("Restack needed", theme.warning)
    } else {
        ("Restack health: up to date", theme.success)
    };

    section(
        "Stack health",
        theme,
        div().text_sm().text_color(color).child(label.to_string()),
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
            let remote = if details.has_remote {
                format!(
                    "Remote: {} unpushed · {} unpulled",
                    details.unpushed, details.unpulled
                )
            } else {
                "Remote: not published".to_string()
            };
            let mut ready = div()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .child(format!(
                    "Divergence: {} ahead · {} behind",
                    details.ahead, details.behind
                ))
                .child(remote)
                .child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.text_muted)
                        .child(format!("COMMITS · {}", details.commits.len())),
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
                        .font_family(MONOSPACE_FONT)
                        .text_xs()
                        .child(format!("• {commit}"))
                }));
            }
            ready
        }
    };

    section("Branch details", theme, body)
}

fn render_pull_request(branch: &BranchSummary, theme: Theme) -> Div {
    let body = match branch.pr_number {
        Some(number) => div().text_sm().child(format!(
            "PR #{number} · {}",
            branch.pr_state.as_deref().unwrap_or("state unavailable")
        )),
        None => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .child("No pull request cached.")
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Submit and Open PR become available in phase 2."),
            ),
    };
    section("Pull request", theme, body)
}

fn render_ci(ci: &LoadState<CiSummary>, branch: &BranchSummary, theme: Theme) -> Div {
    let body = match ci {
        LoadState::Idle => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .child(
                branch
                    .ci_state
                    .as_ref()
                    .map(|state| format!("Cached CI: {state}"))
                    .unwrap_or_else(|| "CI details not loaded.".into()),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.text_muted)
                    .child("Select a published branch or refresh to load live checks."),
            ),
        LoadState::Loading => div()
            .text_sm()
            .text_color(theme.text_muted)
            .child("Loading CI checks…"),
        LoadState::Failed(error) => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .text_color(theme.danger)
            .child("CI checks are unavailable.")
            .child(error.clone())
            .child("Resolve the message above, then refresh the repository."),
        LoadState::Ready(summary) => div()
            .flex()
            .flex_col()
            .gap_1()
            .text_sm()
            .child(format!(
                "Status: {}",
                summary.overall_status.as_deref().unwrap_or("unknown")
            ))
            .child(format!(
                "{} passed · {} failed · {} running · {} queued · {} skipped",
                summary.passed, summary.failed, summary.running, summary.queued, summary.skipped
            )),
    };
    section("Continuous integration", theme, body)
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
        ControlKind::Secondary,
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

    div()
        .flex()
        .flex_col()
        .gap_2()
        .pt_1()
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child("STACK ACTIONS"),
        )
        .child(checkout)
        .child(rename)
        .child(delete)
        .child(move_subtree)
        .child(reorder)
        .child(restack)
        .child(open_pr)
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

fn section(title: &str, theme: Theme, body: Div) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .pb_3()
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text_muted)
                .child(title.to_uppercase()),
        )
        .child(body)
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
