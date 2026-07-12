use super::{AppView, ControlKind, activate_control, control_button, text_input::BranchNameInput};
use crate::theme::{MONOSPACE_FONT, Theme};
use gpui::{Context, Div, Entity, ParentElement as _, Styled as _, div, px};
use stax::application::{PullRequestMode, RestackScope};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationOverlay {
    CreateBranch {
        parent: String,
        validation_error: Option<String>,
    },
    ConfirmRestack {
        scope: RestackScope,
        affected_branches: Vec<String>,
        auto_stash: bool,
    },
    ConfirmStashAndRestack {
        scope: RestackScope,
        dirty_worktrees: Vec<PathBuf>,
    },
    ConfirmSubmit {
        current_branch: String,
        affected_branches: Vec<String>,
        mode: PullRequestMode,
    },
}

pub fn render(
    overlay: &OperationOverlay,
    branch_input: Option<Entity<BranchNameInput>>,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .bottom_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.overlay_scrim)
        .child(card(overlay, branch_input, theme, cx))
}

fn card(
    overlay: &OperationOverlay,
    branch_input: Option<Entity<BranchNameInput>>,
    theme: Theme,
    cx: &mut Context<AppView>,
) -> Div {
    let (title, body, primary) = match overlay {
        OperationOverlay::CreateBranch {
            parent,
            validation_error,
        } => {
            let mut body = div()
                .flex()
                .flex_col()
                .gap_3()
                .child(line("Parent", parent, theme));
            if let Some(input) = branch_input {
                body = body.child(
                    div()
                        .border_1()
                        .border_color(theme.border_strong)
                        .rounded_md()
                        .bg(theme.surface)
                        .child(input),
                );
            }
            if let Some(error) = validation_error {
                body = body.child(
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(error.clone()),
                );
            }
            ("Create branch", body, "Create")
        }
        OperationOverlay::ConfirmRestack {
            scope,
            affected_branches,
            auto_stash,
        } => (
            "Confirm restack",
            branch_list(
                format!("Scope: {}", restack_scope_label(scope)),
                affected_branches,
            )
            .child("Rebase rewrites local commits.")
            .child(format!("Auto stash: {auto_stash}")),
            "Restack",
        ),
        OperationOverlay::ConfirmStashAndRestack {
            scope,
            dirty_worktrees,
        } => (
            "Stash and restack",
            path_list(
                format!("Scope: {}", restack_scope_label(scope)),
                dirty_worktrees,
            )
            .child("Stashes are kept if a conflict stops the rebase."),
            "Stash and Restack",
        ),
        OperationOverlay::ConfirmSubmit {
            current_branch,
            affected_branches,
            mode,
        } => (
            "Confirm submit",
            branch_list(
                format!("Current stack: {current_branch}"),
                affected_branches,
            )
            .child(format!(
                "New pull requests: {}",
                pull_request_mode_label(*mode)
            ))
            .child("This pushes branches and may create or update remote pull requests."),
            "Submit",
        ),
    };

    let cancel = activate_control(
        control_button(
            "operation-overlay-cancel",
            "Cancel",
            ControlKind::Secondary,
            true,
            theme,
        ),
        cx,
        |app, window, cx| app.dismiss_overlay(window, cx),
    );
    let confirm = activate_control(
        control_button(
            "operation-overlay-confirm",
            primary,
            ControlKind::Primary,
            true,
            theme,
        ),
        cx,
        |app, window, cx| app.confirm_overlay(window, cx),
    );

    div()
        .w(px(430.0))
        .max_w(px(520.0))
        .flex()
        .flex_col()
        .gap_4()
        .rounded_lg()
        .border_1()
        .border_color(theme.border_strong)
        .bg(theme.surface_raised)
        .p_4()
        .shadow_lg()
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(body.text_sm().text_color(theme.text))
        .child(
            div()
                .flex()
                .justify_end()
                .gap_2()
                .child(cancel)
                .child(confirm),
        )
}

fn line(label: &str, value: &str, theme: Theme) -> Div {
    div()
        .flex()
        .gap_2()
        .child(
            div()
                .text_color(theme.text_muted)
                .child(format!("{label}:")),
        )
        .child(div().font_family(MONOSPACE_FONT).child(value.to_string()))
}

fn branch_list(heading: String, branches: &[String]) -> Div {
    div().flex().flex_col().gap_2().child(heading).children(
        branches
            .iter()
            .map(|branch| div().font_family(MONOSPACE_FONT).child(branch.clone())),
    )
}

fn path_list(heading: String, paths: &[PathBuf]) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(heading)
        .children(paths.iter().map(|path| {
            div()
                .font_family(MONOSPACE_FONT)
                .child(path.display().to_string())
        }))
}

fn restack_scope_label(scope: &RestackScope) -> String {
    match scope {
        RestackScope::Branch(branch) => format!("branch {branch}"),
        RestackScope::StackContaining(branch) => format!("stack containing {branch}"),
        RestackScope::ThroughBranch(branch) => format!("through {branch}"),
        RestackScope::All => "all branches".to_string(),
    }
}

fn pull_request_mode_label(mode: PullRequestMode) -> &'static str {
    match mode {
        PullRequestMode::Draft => "Draft",
        PullRequestMode::Ready => "Ready",
    }
}
