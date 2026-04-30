use crate::tui::app::{App, BranchDisplay};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the details panel (bottom left)
pub fn render_details(f: &mut Frame, app: &App, area: Rect) {
    let branch = app.selected_branch();

    let content = if let Some(branch) = branch {
        build_details_content(app, branch)
    } else {
        vec![Line::from("No branch selected")]
    };

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                " Summary ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .border_style(Style::default().fg(Color::Cyan)),
    );

    f.render_widget(paragraph, area);
}

fn build_details_content(app: &App, branch: &BranchDisplay) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(vec![
        Span::styled(
            truncate_middle(&branch.name, 32),
            Style::default()
                .fg(if branch.is_trunk {
                    Color::Blue
                } else {
                    Color::White
                })
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        badge_span("current", branch.is_current, Color::Green),
        badge_span("trunk", branch.is_trunk, Color::Blue),
        badge_span("needs restack", branch.needs_restack, Color::Red),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Next: ", Style::default().fg(Color::DarkGray)),
        primary_action_span(branch),
    ]));

    lines.push(Line::from(vec![
        Span::styled("Actions: ", Style::default().fg(Color::DarkGray)),
        Span::raw(branch_actions(branch)),
    ]));

    if !branch.details_loaded && !branch.is_trunk {
        lines.push(Line::from(vec![Span::styled(
            "Loading branch details...",
            Style::default().fg(Color::Yellow),
        )]));
    } else if let Some(parent) = &branch.parent {
        lines.push(Line::from(vec![
            Span::styled("Base: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                truncate_middle(parent, 26),
                Style::default().fg(Color::Blue),
            ),
            Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}c", branch.commits.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}↑", branch.ahead),
                Style::default().fg(Color::Green),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{}↓", branch.behind),
                Style::default().fg(Color::Red),
            ),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Remote: ", Style::default().fg(Color::DarkGray)),
        Span::styled(remote_summary(branch), remote_summary_style(branch)),
        Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
        Span::styled("PR: ", Style::default().fg(Color::DarkGray)),
        pr_summary_span(branch),
        Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
        Span::styled("CI: ", Style::default().fg(Color::DarkGray)),
        ci_summary_span(branch),
    ]));

    let (live_ci, dimmed) = app.ci_summary_line(branch);
    lines.push(Line::from(vec![Span::styled(
        live_ci,
        if dimmed {
            Style::default().fg(Color::DarkGray)
        } else {
            Style::default().fg(Color::Yellow)
        },
    )]));

    lines.push(Line::from(vec![Span::styled(
        "Recent commits",
        Style::default().add_modifier(Modifier::BOLD),
    )]));

    if branch.commits.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "• No branch-specific commits yet",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        for commit in branch.commits.iter().take(2) {
            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::DarkGray)),
                Span::raw(truncate(commit, 72)),
            ]));
        }

        if branch.commits.len() > 2 {
            lines.push(Line::from(vec![Span::styled(
                format!("+{} more commits", branch.commits.len() - 2),
                Style::default().fg(Color::DarkGray),
            )]));
        }
    }

    lines
}

fn primary_action_span(branch: &BranchDisplay) -> Span<'static> {
    let (label, color) = if !branch.is_current {
        (
            "Press Enter to checkout this branch".to_string(),
            Color::Green,
        )
    } else if branch.is_trunk {
        (
            "Create a branch with n to start a stack".to_string(),
            Color::Cyan,
        )
    } else if branch.needs_restack {
        (
            format!(
                "Press r to restack onto {}",
                truncate_middle(branch.parent.as_deref().unwrap_or("its parent"), 24)
            ),
            Color::Yellow,
        )
    } else if branch.pr_number.is_none() || !branch.has_remote || branch.unpushed > 0 {
        (
            "Press s to submit and sync the branch".to_string(),
            Color::Green,
        )
    } else if branch.ci_state.as_deref() == Some("failure")
        || branch.ci_state.as_deref() == Some("error")
    {
        (
            "Press p to inspect the PR and fix CI".to_string(),
            Color::Red,
        )
    } else if branch.pr_number.is_some() {
        (
            "Press p to open the PR in your browser".to_string(),
            Color::Cyan,
        )
    } else {
        ("No action needed right now".to_string(), Color::DarkGray)
    };

    Span::styled(label, Style::default().fg(color))
}

fn branch_actions(branch: &BranchDisplay) -> String {
    if !branch.is_current {
        if branch.pr_number.is_some() {
            "Enter checkout  •  p open PR".to_string()
        } else {
            "Enter checkout".to_string()
        }
    } else if branch.is_trunk {
        "n create branch  •  / search".to_string()
    } else {
        let mut actions = vec!["s submit".to_string(), "o reorder".to_string()];

        if branch.needs_restack {
            actions.insert(0, "r restack".to_string());
        }

        if branch.pr_number.is_some() {
            actions.push("p open PR".to_string());
        }

        actions.push("e rename".to_string());
        actions.join("  •  ")
    }
}

fn remote_summary(branch: &BranchDisplay) -> String {
    if branch.is_trunk {
        "local".to_string()
    } else if !branch.details_loaded {
        "loading".to_string()
    } else if !branch.has_remote {
        "not pushed".to_string()
    } else if branch.unpushed == 0 && branch.unpulled == 0 {
        "synced".to_string()
    } else if branch.unpushed > 0 && branch.unpulled > 0 {
        format!("{}↑ {}↓", branch.unpushed, branch.unpulled)
    } else if branch.unpushed > 0 {
        format!("{}↑", branch.unpushed)
    } else {
        format!("{}↓", branch.unpulled)
    }
}

fn remote_summary_style(branch: &BranchDisplay) -> Style {
    if branch.is_trunk {
        Style::default().fg(Color::DarkGray)
    } else if !branch.details_loaded || !branch.has_remote {
        Style::default().fg(Color::Yellow)
    } else if branch.unpulled > 0 {
        Style::default().fg(Color::Magenta)
    } else if branch.unpushed > 0 {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    }
}

fn pr_summary_span(branch: &BranchDisplay) -> Span<'static> {
    if let Some(pr_num) = branch.pr_number {
        let state = branch
            .pr_state
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        let color = match state.to_lowercase().as_str() {
            "open" => Color::Green,
            "closed" => Color::Red,
            "merged" => Color::Magenta,
            _ => Color::Yellow,
        };
        Span::styled(format!("#{} {}", pr_num, state), Style::default().fg(color))
    } else if branch.is_trunk {
        Span::styled("n/a", Style::default().fg(Color::DarkGray))
    } else {
        Span::styled("not created", Style::default().fg(Color::Yellow))
    }
}

fn ci_summary_span(branch: &BranchDisplay) -> Span<'static> {
    let (label, color) = match branch.ci_state.as_deref() {
        Some("success") => ("pass", Color::Green),
        Some("failure") | Some("error") => ("fail", Color::Red),
        Some("pending") => ("pending", Color::Yellow),
        Some(other) => (other, Color::DarkGray),
        None => ("?", Color::DarkGray),
    };
    Span::styled(label.to_string(), Style::default().fg(color))
}

fn badge_span(label: &str, show: bool, color: Color) -> Span<'static> {
    if show {
        Span::styled(
            format!("[{}] ", label),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    }
}

fn truncate(text: &str, max_len: usize) -> String {
    if text.chars().count() <= max_len {
        text.to_string()
    } else {
        let mut truncated = text
            .chars()
            .take(max_len.saturating_sub(1))
            .collect::<String>();
        truncated.push('…');
        truncated
    }
}

fn truncate_middle(text: &str, max_len: usize) -> String {
    let chars = text.chars().collect::<Vec<_>>();
    if chars.len() <= max_len {
        return text.to_string();
    }

    if max_len <= 1 {
        return "…".to_string();
    }

    let left_len = (max_len - 1) / 2;
    let right_len = max_len - 1 - left_len;
    let left = chars.iter().take(left_len).collect::<String>();
    let right = chars
        .iter()
        .skip(chars.len().saturating_sub(right_len))
        .collect::<String>();
    format!("{}…{}", left, right)
}

#[cfg(test)]
mod tests {
    use super::{branch_actions, primary_action_span, remote_summary, truncate, truncate_middle};
    use crate::tui::app::BranchDisplay;

    fn branch() -> BranchDisplay {
        BranchDisplay {
            name: "feature/demo".to_string(),
            parent: Some("main".to_string()),
            column: 0,
            is_current: true,
            is_trunk: false,
            ahead: 2,
            behind: 0,
            needs_restack: false,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number: None,
            pr_state: None,
            ci_state: None,
            commits: vec!["add thing".to_string()],
            details_loaded: true,
        }
    }

    #[test]
    fn suggests_checkout_for_non_current_branch() {
        let mut branch = branch();
        branch.is_current = false;
        assert_eq!(
            primary_action_span(&branch).content,
            "Press Enter to checkout this branch"
        );
    }

    #[test]
    fn suggests_submit_for_unpublished_branch() {
        let branch = branch();
        assert_eq!(
            primary_action_span(&branch).content,
            "Press s to submit and sync the branch"
        );
    }

    #[test]
    fn branch_actions_prioritize_restack_when_needed() {
        let mut branch = branch();
        branch.needs_restack = true;
        assert!(branch_actions(&branch).starts_with("r restack"));
    }

    #[test]
    fn remote_summary_reports_bidirectional_drift() {
        let mut branch = branch();
        branch.has_remote = true;
        branch.unpushed = 2;
        branch.unpulled = 1;
        assert_eq!(remote_summary(&branch), "2↑ 1↓");
    }

    #[test]
    fn truncate_adds_ellipsis() {
        assert_eq!(truncate("abcdefghijklmnopqrstuvwxyz", 10), "abcdefghi…");
    }

    #[test]
    fn truncate_middle_preserves_prefix_and_suffix() {
        assert_eq!(
            truncate_middle("feature/super-long-branch-name/demo", 12),
            "featu…e/demo"
        );
    }
}
