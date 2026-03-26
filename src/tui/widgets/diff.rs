use crate::tui::app::{App, BranchDisplay, DiffLineType, FocusedPane};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the diff panel (right side)
pub fn render_diff(f: &mut Frame, app: &App, area: Rect) {
    let branch = app.selected_branch();
    let is_focused = app.focused_pane == FocusedPane::Diff;

    let title = if let Some(b) = branch {
        if let Some(parent) = &b.parent {
            format!(" Patch: {} ← {} ", b.name, parent)
        } else {
            format!(" {} ", b.name)
        }
    } else {
        " Patch ".to_string()
    };

    let (border_color, title_style) = if is_focused {
        (
            Color::Cyan,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (Color::DarkGray, Style::default().fg(Color::DarkGray))
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(title, title_style))
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let header_lines = build_diff_header(app);
    let header_height = header_lines.len().max(1) as u16;
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(1)])
        .split(inner);

    f.render_widget(Paragraph::new(header_lines), chunks[0]);
    f.render_widget(Paragraph::new(build_patch_lines(app, branch)), chunks[1]);
}

fn build_diff_header(app: &App) -> Vec<Line<'static>> {
    if app.diff_stat.is_empty() {
        return vec![Line::from(vec![Span::styled(
            "No file summary available",
            Style::default().fg(Color::DarkGray),
        )])];
    }

    let total_add: usize = app.diff_stat.iter().map(|s| s.additions).sum();
    let total_del: usize = app.diff_stat.iter().map(|s| s.deletions).sum();
    let visible_stats = app.diff_stat.iter().take(4).collect::<Vec<_>>();
    let max_file_len = visible_stats
        .iter()
        .map(|s| s.file.len())
        .max()
        .unwrap_or(20)
        .min(36);

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("{} files", app.diff_stat.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("+{}", total_add), Style::default().fg(Color::Green)),
            Span::styled("  •  ", Style::default().fg(Color::DarkGray)),
            Span::styled(format!("-{}", total_del), Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![Span::styled(
            "Top changed files",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    for stat in &visible_stats {
        let file = if stat.file.len() > max_file_len {
            format!("...{}", &stat.file[stat.file.len() - max_file_len + 3..])
        } else {
            stat.file.clone()
        };
        let total_changes = stat.additions + stat.deletions;

        lines.push(Line::from(vec![
            Span::styled(
                format!("{:width$}", file, width = max_file_len),
                Style::default().fg(Color::White),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:>3}", total_changes),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled("  +", Style::default().fg(Color::DarkGray)),
            Span::styled(
                stat.additions.to_string(),
                Style::default().fg(Color::Green),
            ),
            Span::styled(" -", Style::default().fg(Color::DarkGray)),
            Span::styled(stat.deletions.to_string(), Style::default().fg(Color::Red)),
        ]));
    }

    if app.diff_stat.len() > visible_stats.len() {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "+{} more files in patch",
                app.diff_stat.len() - visible_stats.len()
            ),
            Style::default().fg(Color::DarkGray),
        )]));
    }

    lines
}

fn build_patch_lines(app: &App, branch: Option<&BranchDisplay>) -> Vec<Line<'static>> {
    if app.selected_diff.is_empty() {
        if branch.map(|b| b.is_trunk).unwrap_or(true) {
            return vec![Line::from(Span::styled(
                "No patch for trunk",
                Style::default().fg(Color::DarkGray),
            ))];
        }

        if app.diff_stat.is_empty() {
            return vec![Line::from(Span::styled(
                "No changes in this branch",
                Style::default().fg(Color::DarkGray),
            ))];
        }
    }

    app.selected_diff
        .iter()
        .skip(app.diff_scroll)
        .map(|diff_line| {
            let style = match diff_line.line_type {
                DiffLineType::Addition => Style::default().fg(Color::Green),
                DiffLineType::Deletion => Style::default().fg(Color::Red),
                DiffLineType::Hunk => Style::default().fg(Color::Cyan),
                DiffLineType::Header => Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
                DiffLineType::Context => Style::default().fg(Color::White),
            };

            Line::from(Span::styled(diff_line.content.clone(), style))
        })
        .collect()
}
