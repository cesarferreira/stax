use super::app::{FlatItem, HunkSplitApp, HunkSplitMode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render(f: &mut Frame, app: &HunkSplitApp) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(f.area());

    let main_area = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(outer[0]);

    render_file_list(f, app, main_area[0]);
    render_diff_preview(f, app, main_area[1]);
    render_status_bar(f, app, outer[1]);

    match &app.mode {
        HunkSplitMode::Naming => render_naming_dialog(f, app),
        HunkSplitMode::ConfirmAbort => render_confirm_abort(f),
        HunkSplitMode::Help => render_help_dialog(f),
        _ => {}
    }
}

fn render_file_list(f: &mut Frame, app: &HunkSplitApp, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    for (i, flat_item) in app.flat_items.iter().enumerate() {
        let is_cursor = i == app.cursor;
        match flat_item {
            FlatItem::FileHeader { file_idx } => {
                let sel = app.file_selected_count(*file_idx);
                let total = app.file_hunk_count(*file_idx);
                let path = &app.files[*file_idx].path;

                let cursor_indicator = if is_cursor { "▶ " } else { "  " };
                let line = Line::from(vec![
                    Span::styled(
                        cursor_indicator,
                        Style::default().fg(if is_cursor {
                            Color::Yellow
                        } else {
                            Color::Reset
                        }),
                    ),
                    Span::styled(
                        path.to_string(),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" ({}/{})", sel, total),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]);
                let mut item = ListItem::new(line);
                if is_cursor {
                    item = item.style(Style::default().bg(Color::DarkGray));
                }
                items.push(item);
            }
            FlatItem::Hunk { file_idx, hunk_idx } => {
                let selected = app.selected[*file_idx][*hunk_idx];
                let checkbox = if selected { "\u{2611}" } else { "\u{2610}" };
                let file = &app.files[*file_idx];
                let hunk = &file.hunks[*hunk_idx];

                let (snippet, range) = if hunk.header.is_empty() {
                    let label = if file.is_new {
                        "new file".to_string()
                    } else if file.is_deleted {
                        "deleted file".to_string()
                    } else {
                        "empty change".to_string()
                    };
                    (label, String::new())
                } else {
                    let r = format!("@@ +{},{}", hunk.new_start, hunk.new_count);
                    let s = hunk
                        .lines
                        .iter()
                        .find(|l| l.starts_with('+') || l.starts_with('-'))
                        .map(|l| {
                            let trimmed = l[1..].trim();
                            if trimmed.len() > 40 {
                                format!("{}...", &trimmed[..37])
                            } else {
                                trimmed.to_string()
                            }
                        })
                        .unwrap_or_else(|| r.clone());
                    (s, r)
                };

                let cursor_indicator = if is_cursor { "  ▸ " } else { "    " };
                let line = Line::from(vec![
                    Span::styled(
                        cursor_indicator,
                        Style::default().fg(if is_cursor {
                            Color::Yellow
                        } else {
                            Color::Reset
                        }),
                    ),
                    Span::styled(
                        format!("{} ", checkbox),
                        Style::default().fg(if selected {
                            Color::Green
                        } else {
                            Color::DarkGray
                        }),
                    ),
                    Span::styled(snippet, Style::default().fg(Color::White)),
                    Span::styled(format!(" {}", range), Style::default().fg(Color::DarkGray)),
                ]);
                let mut item = ListItem::new(line);
                if is_cursor {
                    item = item.style(Style::default().bg(Color::DarkGray));
                }
                items.push(item);
            }
        }
    }

    let mode_label = match app.mode {
        HunkSplitMode::List => "List",
        HunkSplitMode::Sequential => "Sequential",
        _ => "List",
    };

    let title = format!(
        " Round {} \u{2502} {} mode \u{2502} {}/{} selected ",
        app.round,
        mode_label,
        app.selected_count(),
        app.total_hunk_count()
    );

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_diff_preview(f: &mut Frame, app: &HunkSplitApp, area: Rect) {
    let block = Block::default()
        .title(" Diff Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = match app.current_item() {
        Some(FlatItem::Hunk { file_idx, hunk_idx }) => {
            let file = &app.files[*file_idx];
            let hunk = &file.hunks[*hunk_idx];
            let mut result = Vec::new();
            if hunk.header.is_empty() {
                result.push(Line::from(Span::styled(
                    file.path.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
                let label = if file.is_new {
                    "new file"
                } else if file.is_deleted {
                    "deleted file"
                } else {
                    "empty change"
                };
                result.push(Line::from(Span::styled(
                    label,
                    Style::default().fg(if file.is_new {
                        Color::Green
                    } else {
                        Color::Red
                    }),
                )));
            } else {
                result.push(Line::from(Span::styled(
                    hunk.header.clone(),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )));
            }
            for line in &hunk.lines {
                let style = if line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                result.push(Line::from(Span::styled(line.clone(), style)));
            }
            result
        }
        Some(FlatItem::FileHeader { file_idx }) => {
            let file = &app.files[*file_idx];
            let mut result = vec![Line::from(Span::styled(
                file.path.clone(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))];
            if file.is_new {
                result.push(Line::from(Span::styled(
                    "new file",
                    Style::default().fg(Color::Green),
                )));
            }
            if file.is_deleted {
                result.push(Line::from(Span::styled(
                    "deleted file",
                    Style::default().fg(Color::Red),
                )));
            }
            result.push(Line::from(Span::styled(
                format!("{} hunk(s)", file.hunks.len()),
                Style::default().fg(Color::DarkGray),
            )));
            result
        }
        None => vec![Line::from(Span::styled(
            "No hunks",
            Style::default().fg(Color::DarkGray),
        ))],
    };

    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(paragraph, inner);
}

fn render_status_bar(f: &mut Frame, app: &HunkSplitApp, area: Rect) {
    let help_line = match app.mode {
        HunkSplitMode::List => {
            "j/k:nav  Space:toggle  a:file  Tab:sequential  Enter:commit  u:undo  ?:help  q:quit"
        }
        HunkSplitMode::Sequential => {
            "y:accept  n:skip  a:toggle file  Tab:list  Enter:commit  u:undo  ?:help  q:quit"
        }
        HunkSplitMode::Naming => "Enter:confirm  Esc:cancel",
        HunkSplitMode::ConfirmAbort => "y:quit  n:cancel",
        HunkSplitMode::Help => "any key: close",
    };

    let status_text = app.status_message.as_deref().unwrap_or("");

    let lines = vec![
        Line::from(Span::styled(
            status_text,
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            help_line,
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

fn render_naming_dialog(f: &mut Frame, app: &HunkSplitApp) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Enter branch name ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let input_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    let input_text =
        Paragraph::new(app.input_buffer.as_str()).style(Style::default().fg(Color::White));
    f.render_widget(input_text, input_area[0]);

    f.set_cursor_position((input_area[0].x + app.input_cursor as u16, input_area[0].y));

    let hint = Paragraph::new("Enter to confirm, Esc to cancel")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, input_area[1]);
}

fn render_confirm_abort(f: &mut Frame) {
    let area = centered_rect(40, 15, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Abort Split? ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Discard all progress and restore original branch?",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "y",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
            Span::raw(": yes, abort  "),
            Span::styled(
                "n",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(": no, continue"),
        ]),
    ];

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn render_help_dialog(f: &mut Frame) {
    let area = centered_rect(55, 65, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let help_text = vec![
        Line::from(Span::styled(
            "List Mode",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  j/k        Navigate up/down"),
        Line::from("  Space      Toggle hunk selection"),
        Line::from("  a          Toggle all hunks in file"),
        Line::from("  u          Undo last toggle"),
        Line::from("  Tab        Switch to sequential mode"),
        Line::from("  Enter      Commit selected hunks"),
        Line::from(""),
        Line::from(Span::styled(
            "Sequential Mode",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  y          Accept hunk and advance"),
        Line::from("  n          Skip hunk and advance"),
        Line::from("  a          Toggle file and skip past"),
        Line::from("  u          Undo last action"),
        Line::from("  Tab        Switch to list mode"),
        Line::from("  Enter      Commit selected hunks"),
        Line::from(""),
        Line::from(Span::styled(
            "General",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("  ?          Toggle help"),
        Line::from("  q/Esc      Quit (abort split)"),
        Line::from("  Ctrl-C     Force quit"),
        Line::from(""),
        Line::from(Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let text = Paragraph::new(help_text);
    f.render_widget(text, inner);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
