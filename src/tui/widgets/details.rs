use crate::tui::app::{App, BranchDisplay};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the details panel (right side)
pub fn render_details(f: &mut Frame, app: &App, area: Rect) {
    let branch = app.selected_branch();

    let content = if let Some(branch) = branch {
        build_details_content(branch)
    } else {
        vec![Line::from("No branch selected")]
    };

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" Details ")
            .border_style(Style::default().fg(Color::White)),
    );

    f.render_widget(paragraph, area);
}

fn build_details_content(branch: &BranchDisplay) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Branch name (header)
    lines.push(Line::from(vec![Span::styled(
        branch.name.clone(),
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
    )]));

    // Separator
    lines.push(Line::from("─".repeat(30)));
    lines.push(Line::from(""));

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
            Span::raw(" • "),
            Span::styled(state, Style::default().fg(state_color)),
        ]));

        // PR URL
        if let Some(url) = &branch.pr_url {
            lines.push(Line::from(vec![
                Span::styled(url.clone(), Style::default().fg(Color::Blue)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // Status indicators
    let mut status_parts = Vec::new();

    if branch.has_remote {
        status_parts.push(Span::styled("☁ remote", Style::default().fg(Color::Cyan)));
        status_parts.push(Span::raw("  "));
    }

    if branch.is_current {
        status_parts.push(Span::styled(
            "◉ current",
            Style::default().fg(Color::Green),
        ));
        status_parts.push(Span::raw("  "));
    }

    if branch.needs_restack {
        status_parts.push(Span::styled(
            "⟳ needs restack",
            Style::default().fg(Color::Yellow),
        ));
    }

    if !status_parts.is_empty() {
        lines.push(Line::from(status_parts));
        lines.push(Line::from(""));
    }

    // Ahead/behind
    if branch.ahead > 0 || branch.behind > 0 {
        let mut parts = Vec::new();

        if branch.ahead > 0 {
            parts.push(Span::styled(
                format!("{}↑ ahead", branch.ahead),
                Style::default().fg(Color::Green),
            ));
        }

        if branch.ahead > 0 && branch.behind > 0 {
            parts.push(Span::raw("  "));
        }

        if branch.behind > 0 {
            parts.push(Span::styled(
                format!("{}↓ behind", branch.behind),
                Style::default().fg(Color::Red),
            ));
        }

        lines.push(Line::from(parts));
        lines.push(Line::from(""));
    }

    // Commits
    if !branch.commits.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            format!("Commits ({}):", branch.commits.len()),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]));

        for commit in &branch.commits {
            // Truncate long commit messages
            let msg = if commit.len() > 40 {
                format!("{}...", &commit[..37])
            } else {
                commit.clone()
            };

            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::DarkGray)),
                Span::raw(msg),
            ]));
        }
    }

    lines
}
