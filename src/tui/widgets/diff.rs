use crate::tui::app::{App, DiffLineType, FocusedPane};
use ratatui::{
    layout::Rect,
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
            format!(" Diff: {} ← {} ", b.name, parent)
        } else {
            format!(" {} ", b.name)
        }
    } else {
        " Diff ".to_string()
    };

    let (border_color, title_style) = if is_focused {
        (Color::Cyan, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
    } else {
        (Color::DarkGray, Style::default().fg(Color::DarkGray))
    };

    // Build content with stat header + diff lines
    let mut content: Vec<Line> = Vec::new();

    // Add diff stat summary at top
    if !app.diff_stat.is_empty() {
        let total_add: usize = app.diff_stat.iter().map(|s| s.additions).sum();
        let total_del: usize = app.diff_stat.iter().map(|s| s.deletions).sum();

        content.push(Line::from(vec![
            Span::styled(
                format!("{} files changed, ", app.diff_stat.len()),
                Style::default().fg(Color::White),
            ),
            Span::styled(
                format!("{} insertions(+)", total_add),
                Style::default().fg(Color::Green),
            ),
            Span::raw(", "),
            Span::styled(
                format!("{} deletions(-)", total_del),
                Style::default().fg(Color::Red),
            ),
        ]));
        content.push(Line::from(""));

        // Add file stats with +/- bars
        let max_file_len = app.diff_stat.iter().map(|s| s.file.len()).max().unwrap_or(20).min(40);

        for stat in &app.diff_stat {
            let file = if stat.file.len() > max_file_len {
                format!("...{}", &stat.file[stat.file.len() - max_file_len + 3..])
            } else {
                stat.file.clone()
            };

            let total_changes = stat.additions + stat.deletions;
            let bar_width = 30.min(total_changes);
            let add_bars = if total_changes > 0 {
                (stat.additions * bar_width) / total_changes
            } else {
                0
            };
            let del_bars = bar_width.saturating_sub(add_bars);

            content.push(Line::from(vec![
                Span::styled(
                    format!("{:width$}", file, width = max_file_len),
                    Style::default().fg(Color::White),
                ),
                Span::raw(" | "),
                Span::styled(
                    format!("{:>4}", total_changes),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" "),
                Span::styled(
                    "+".repeat(add_bars),
                    Style::default().fg(Color::Green),
                ),
                Span::styled(
                    "-".repeat(del_bars),
                    Style::default().fg(Color::Red),
                ),
            ]));
        }

        content.push(Line::from(""));
        content.push(Line::from(vec![Span::styled(
            "─".repeat(60),
            Style::default().fg(Color::DarkGray),
        )]));
        content.push(Line::from(""));
    }

    // Add diff content with scroll
    if app.selected_diff.is_empty() {
        if branch.map(|b| b.is_trunk).unwrap_or(true) {
            content.push(Line::from(Span::styled(
                "No diff for trunk",
                Style::default().fg(Color::DarkGray),
            )));
        } else if app.diff_stat.is_empty() {
            content.push(Line::from(Span::styled(
                "No changes",
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else {
        // Apply scroll offset
        let diff_lines: Vec<Line> = app
            .selected_diff
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
            .collect();

        content.extend(diff_lines);
    }

    // Add scroll indicator if needed
    let title_with_scroll = if !app.selected_diff.is_empty() && app.diff_scroll > 0 {
        format!("{} [line {}]", title, app.diff_scroll + 1)
    } else {
        title
    };

    let paragraph = Paragraph::new(content).block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(title_with_scroll, title_style))
            .border_style(Style::default().fg(border_color)),
    );

    f.render_widget(paragraph, area);
}
