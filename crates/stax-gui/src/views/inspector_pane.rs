use super::{ControlKind, WorkspaceView, control_button};
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
    _cx: &mut gpui::Context<super::AppView>,
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
        Some(branch) => render_selected(workspace, branch, theme),
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
        .bg(theme.surface)
        .child(
            div()
                .h(px(43.0))
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
                .child(content),
        )
}

fn render_selected(workspace: &WorkspaceView, branch: &BranchSummary, theme: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_4()
        .p_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .font_family(MONOSPACE_FONT)
                        .text_base()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(branch.name.clone()),
                )
                .child(
                    div()
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
        .child(render_disabled_actions(theme))
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

fn render_disabled_actions(theme: Theme) -> Div {
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
                .child("STACK ACTIONS · READ-ONLY"),
        )
        .child(control_button(
            "inspector-checkout",
            "Checkout — Available in phase 2",
            ControlKind::Secondary,
            false,
            theme,
        ))
        .child(control_button(
            "inspector-restack",
            "Restack — Available in phase 2",
            ControlKind::Secondary,
            false,
            theme,
        ))
        .child(control_button(
            "inspector-submit",
            "Submit — Available in phase 2",
            ControlKind::Secondary,
            false,
            theme,
        ))
        .child(control_button(
            "inspector-open-pr",
            "Open PR — Available in phase 2",
            ControlKind::Secondary,
            false,
            theme,
        ))
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
