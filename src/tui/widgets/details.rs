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
        build_details_content(branch)
    } else {
        vec![Line::from("No branch selected")]
    };

    // Details panel is never focused, so always use dim styling
    let paragraph = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Details ", Style::default().fg(Color::DarkGray)))
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(paragraph, area);
}

fn build_details_content(branch: &BranchDisplay) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Parent info
    if let Some(parent) = &branch.parent {
        lines.push(Line::from(vec![
            Span::styled("Parent: ", Style::default().fg(Color::DarkGray)),
            Span::styled(parent.clone(), Style::default().fg(Color::Blue)),
        ]));
    }

    // PR info
    if let Some(pr_num) = branch.pr_number {
        let state = branch.pr_state.clone().unwrap_or_else(|| "unknown".to_string());
        let state_color = match state.to_lowercase().as_str() {
            "open" => Color::Green,
            "closed" => Color::Red,
            "merged" => Color::Magenta,
            _ => Color::Yellow,
        };

        lines.push(Line::from(vec![
            Span::styled("PR: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("#{}", pr_num), Style::default().fg(Color::Cyan)),
            Span::raw(" "),
            Span::styled(state, Style::default().fg(state_color)),
        ]));

        if let Some(url) = &branch.pr_url {
            lines.push(Line::from(vec![
                Span::styled(url.clone(), Style::default().fg(Color::Blue)),
            ]));
        }
    }

    // Ahead/behind
    if branch.ahead > 0 || branch.behind > 0 {
        let mut parts = Vec::new();

        if branch.ahead > 0 {
            parts.push(Span::styled(
                format!("{}↑", branch.ahead),
                Style::default().fg(Color::Green),
            ));
            parts.push(Span::raw(" ahead"));
        }

        if branch.ahead > 0 && branch.behind > 0 {
            parts.push(Span::raw("  "));
        }

        if branch.behind > 0 {
            parts.push(Span::styled(
                format!("{}↓", branch.behind),
                Style::default().fg(Color::Red),
            ));
            parts.push(Span::raw(" behind"));
        }

        lines.push(Line::from(parts));
    }

    // Status indicators
    let mut status_parts = Vec::new();

    if branch.has_remote {
        status_parts.push(Span::styled("☁ remote", Style::default().fg(Color::Cyan)));
    }

    if branch.is_current {
        if !status_parts.is_empty() {
            status_parts.push(Span::raw("  "));
        }
        status_parts.push(Span::styled("◉ current", Style::default().fg(Color::Green)));
    }

    if branch.needs_restack {
        if !status_parts.is_empty() {
            status_parts.push(Span::raw("  "));
        }
        status_parts.push(Span::styled("⟳ needs restack", Style::default().fg(Color::Yellow)));
    }

    if !status_parts.is_empty() {
        lines.push(Line::from(status_parts));
    }

    // Commits
    if !branch.commits.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![Span::styled(
            format!("Commits ({}):", branch.commits.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )]));

        for commit in branch.commits.iter().take(3) {
            let msg = if commit.len() > 35 {
                format!("{}...", &commit[..32])
            } else {
                commit.clone()
            };

            lines.push(Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::DarkGray)),
                Span::raw(msg),
            ]));
        }

        if branch.commits.len() > 3 {
            lines.push(Line::from(vec![Span::styled(
                format!("  +{} more", branch.commits.len() - 3),
                Style::default().fg(Color::DarkGray),
            )]));
        }
    }

    lines
}
