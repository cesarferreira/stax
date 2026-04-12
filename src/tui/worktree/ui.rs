use super::app::{worktree_badges, DashboardMode, TmuxState, WorktreeApp};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render(f: &mut Frame, app: &WorktreeApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(4)])
        .split(f.area());

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(42), Constraint::Percentage(58)])
        .split(chunks[0]);

    render_worktree_list(f, app, main[0]);
    render_details(f, app, main[1]);
    render_status_bar(f, app, chunks[1]);

    match app.mode {
        DashboardMode::Help => render_help_modal(f),
        DashboardMode::CreateInput => render_create_modal(f, app),
        DashboardMode::ConfirmDelete => render_delete_modal(f, app),
        DashboardMode::ConfirmForceDelete => render_delete_modal(f, app),
        DashboardMode::Normal => {}
    }
}

fn render_worktree_list(f: &mut Frame, app: &WorktreeApp, area: Rect) {
    let items = app
        .records
        .iter()
        .enumerate()
        .map(|(index, record)| {
            let selected = index == app.selected_index;
            let indicator = if selected { "► " } else { "  " };
            let branch = record
                .branch_label
                .split('/')
                .next_back()
                .unwrap_or(&record.branch_label);

            let mut spans = vec![
                Span::styled(
                    indicator,
                    if selected {
                        selected_row_style().fg(Color::Yellow)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("{:<18}", record.info.name),
                    if selected {
                        selected_name_style(record.info.is_current)
                    } else if record.info.is_current {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
                Span::raw(" "),
                Span::styled(branch.to_string(), muted_row_text_style(selected)),
            ];

            match record.tmux_state {
                TmuxState::Loading => spans.push(Span::styled(
                    "  tmux:...",
                    compact_tmux_style(&record.tmux_state, selected),
                )),
                TmuxState::Attached(_) => spans.push(Span::styled(
                    "  tmux:attached",
                    compact_tmux_style(&record.tmux_state, selected),
                )),
                TmuxState::Detached => spans.push(Span::styled(
                    "  tmux:ready",
                    compact_tmux_style(&record.tmux_state, selected),
                )),
                TmuxState::Missing => spans.push(Span::styled(
                    "  tmux:new",
                    compact_tmux_style(&record.tmux_state, selected),
                )),
                TmuxState::Unavailable => spans.push(Span::styled(
                    "  tmux:off",
                    compact_tmux_style(&record.tmux_state, selected),
                )),
            }

            let mut item = ListItem::new(Line::from(spans));
            if selected {
                item = item.style(selected_row_style());
            }
            item
        })
        .collect::<Vec<_>>();

    let block = Block::default()
        .title(" Worktrees ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    f.render_widget(List::new(items).block(block), area);
}

fn render_details(f: &mut Frame, app: &WorktreeApp, area: Rect) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(record) = app.selected() else {
        f.render_widget(Paragraph::new("No worktrees found"), inner);
        return;
    };

    let base_text = match record.details.as_ref() {
        Some(details) => details
            .stack_parent
            .clone()
            .unwrap_or_else(|| "—".to_string()),
        None if record.load_error.is_some() => "failed to load".to_string(),
        None => "loading...".to_string(),
    };
    let ahead_behind_text = match record.details.as_ref() {
        Some(details) => format!(
            "{} / {}",
            details
                .ahead
                .map(|value| value.to_string())
                .unwrap_or_else(|| "—".to_string()),
            details
                .behind
                .map(|value| value.to_string())
                .unwrap_or_else(|| "—".to_string())
        ),
        None if record.load_error.is_some() => "failed to load".to_string(),
        None => "loading...".to_string(),
    };

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                &record.info.name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Branch: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(record.branch_label.clone()),
        ]),
        Line::from(vec![
            Span::styled("Base: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(base_text),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                record.info.path.display().to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Ahead/Behind: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(ahead_behind_text),
        ]),
        Line::from(vec![
            Span::styled("Tmux: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                tmux_label(&record.tmux_state),
                tmux_style(&record.tmux_state),
            ),
        ]),
        Line::from(vec![
            Span::styled("Session: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(record.tmux_session.clone()),
        ]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Status",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
    ];

    let badge_line = worktree_badges(record)
        .into_iter()
        .map(|badge| Span::styled(format!("[{}] ", badge), badge_style(&badge)))
        .collect::<Vec<_>>();
    lines.push(Line::from(badge_line));

    if let Some(details) = record.details.as_ref() {
        if let Some(marker) = &details.marker {
            lines.push(Line::from(vec![
                Span::styled("Marker: ", Style::default().add_modifier(Modifier::BOLD)),
                Span::styled(marker.clone(), Style::default().fg(Color::Yellow)),
            ]));
        }
    }

    lines.push(Line::from(vec![
        Span::styled("Labels: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(record.status_labels.join(", ")),
    ]));

    if let Some(error) = &record.load_error {
        lines.push(Line::from(vec![
            Span::styled("Load: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(error.clone(), Style::default().fg(Color::Red)),
        ]));
    }

    if record.info.is_locked {
        lines.push(Line::from(vec![
            Span::styled("Lock: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                record
                    .info
                    .lock_reason
                    .clone()
                    .unwrap_or_else(|| "locked".to_string()),
            ),
        ]));
    }

    if record.info.is_prunable {
        lines.push(Line::from(vec![
            Span::styled("Prune: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                record
                    .info
                    .prunable_reason
                    .clone()
                    .unwrap_or_else(|| "stale worktree entry".to_string()),
            ),
        ]));
    }

    lines.push(Line::from(""));
    let footer = if app.is_loading() {
        "Details load incrementally after the first paint; tmux and stack status fill in as they arrive."
    } else {
        "Tmux-first workflow: Enter attaches/switches to the derived session, or creates it on demand."
    };
    lines.push(Line::from(vec![Span::styled(
        footer,
        Style::default().fg(Color::DarkGray),
    )]));

    let text = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(text, inner);
}

fn render_status_bar(f: &mut Frame, app: &WorktreeApp, area: Rect) {
    let status_text = if let Some(removal_msg) = &app.removal_status {
        removal_msg.clone()
    } else if let Some(msg) = &app.status_message {
        msg.clone()
    } else if let Some(loading_msg) = app.loading_summary() {
        loading_msg
    } else {
        "Tmux-first dashboard: browse lanes here, enter the session in tmux when ready.".to_string()
    };

    let status_line = Line::from(Span::styled(
        status_text,
        if app.removal_status.is_some() {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if app.status_message.is_some() {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        },
    ));

    let shortcuts_line = Line::from(vec![
        key_hint("↑↓", Color::Cyan),
        Span::raw(" navigate  "),
        key_hint("Enter", Color::Green),
        Span::raw(" open tmux  "),
        key_hint("c", Color::Cyan),
        Span::raw(" create  "),
        key_hint("d", Color::Red),
        Span::raw(" remove  "),
        key_hint("R", Color::Magenta),
        Span::raw(" restack  "),
        key_hint("?", Color::Yellow),
        Span::raw(" help  "),
        key_hint("q", Color::Cyan),
        Span::raw(" quit"),
    ]);

    f.render_widget(
        Paragraph::new(vec![status_line, shortcuts_line])
            .block(Block::default().borders(Borders::ALL)),
        area,
    );
}

fn render_help_modal(f: &mut Frame) {
    let area = centered_rect(58, 60, f.area());
    f.render_widget(Clear, area);

    let lines = vec![
        Line::from(Span::styled(
            "Worktree Dashboard",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ↑/k, ↓/j   Move selection"),
        Line::from("  Enter      Attach/switch to tmux session for selected worktree"),
        Line::from("  c          Create a new lane, then open it in tmux"),
        Line::from("  d          Remove selected worktree (with confirmation)"),
        Line::from("  R          Restack all stax-managed worktrees"),
        Line::from("  q/Esc      Quit dashboard"),
        Line::from(""),
        Line::from("Leave the create prompt blank to generate a random lane name."),
        Line::from("If tmux is unavailable, the dashboard stays view-only."),
        Line::from(""),
        Line::from("Press any key to close."),
    ];

    let widget = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}

fn render_create_modal(f: &mut Frame, app: &WorktreeApp) {
    let area = centered_rect(52, 22, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Create Lane ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    f.render_widget(Paragraph::new(app.input_buffer.as_str()), chunks[0]);
    f.render_widget(
        Paragraph::new("Enter a lane name or leave blank for a random slug"),
        chunks[1],
    );
    f.set_cursor_position((chunks[0].x + app.input_cursor as u16, chunks[0].y));
}

fn render_delete_modal(f: &mut Frame, app: &WorktreeApp) {
    let area = centered_rect(52, 20, f.area());
    f.render_widget(Clear, area);

    let name = app
        .selected()
        .map(|record| record.info.name.clone())
        .unwrap_or_else(|| "this worktree".to_string());

    let (title, lines) = match app.mode {
        DashboardMode::ConfirmDelete => {
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("Remove ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(&name, Style::default().fg(Color::Red)),
                    Span::raw("?"),
                ]),
                Line::from(""),
                Line::from("Press y to confirm or Esc to cancel."),
            ];
            (" Confirm Remove ", lines)
        }
        DashboardMode::ConfirmForceDelete => {
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        "Warning: ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(&name, Style::default().fg(Color::Red)),
                    Span::raw(" has uncommitted changes."),
                ]),
                Line::from(""),
                Line::from("Force remove anyway?"),
                Line::from(""),
                Line::from("Press y to force remove or Esc to cancel."),
            ];
            (" Force Remove ", lines)
        }
        _ => return, // shouldn't happen
    };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    );
    f.render_widget(widget, area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height) / 2),
            Constraint::Percentage(height),
            Constraint::Percentage((100 - height) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width) / 2),
            Constraint::Percentage(width),
            Constraint::Percentage((100 - width) / 2),
        ])
        .split(vertical[1])[1]
}

fn tmux_label(state: &TmuxState) -> String {
    match state {
        TmuxState::Loading => "probing tmux...".to_string(),
        TmuxState::Unavailable => "unavailable".to_string(),
        TmuxState::Missing => "no session yet".to_string(),
        TmuxState::Detached => "ready to attach".to_string(),
        TmuxState::Attached(count) => format!(
            "attached ({} client{})",
            count,
            if *count == 1 { "" } else { "s" }
        ),
    }
}

fn tmux_style(state: &TmuxState) -> Style {
    match state {
        TmuxState::Loading => Style::default().fg(Color::DarkGray),
        TmuxState::Unavailable => Style::default().fg(Color::Red),
        TmuxState::Missing => Style::default().fg(Color::DarkGray),
        TmuxState::Detached => Style::default().fg(Color::Blue),
        TmuxState::Attached(_) => Style::default().fg(Color::Green),
    }
}

fn selected_row_style() -> Style {
    Style::default().fg(Color::White).bg(Color::DarkGray)
}

fn selected_name_style(is_current: bool) -> Style {
    let style = selected_row_style().fg(if is_current {
        Color::Yellow
    } else {
        Color::Cyan
    });
    if is_current {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn muted_row_text_style(selected: bool) -> Style {
    if selected {
        selected_row_style().fg(Color::White)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn compact_tmux_style(state: &TmuxState, selected: bool) -> Style {
    if selected {
        return match state {
            TmuxState::Loading | TmuxState::Missing => selected_row_style().fg(Color::White),
            TmuxState::Unavailable => selected_row_style().fg(Color::Red),
            TmuxState::Detached => selected_row_style().fg(Color::Cyan),
            TmuxState::Attached(_) => selected_row_style().fg(Color::Green),
        };
    }

    match state {
        TmuxState::Loading => Style::default().fg(Color::DarkGray),
        TmuxState::Unavailable => Style::default().fg(Color::Red),
        TmuxState::Missing => Style::default().fg(Color::DarkGray),
        TmuxState::Detached => Style::default().fg(Color::Blue),
        TmuxState::Attached(_) => Style::default().fg(Color::Green),
    }
}

fn badge_style(label: &str) -> Style {
    match label {
        "current" => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        "main" => Style::default().fg(Color::Cyan),
        "managed" => Style::default().fg(Color::Blue),
        "unmanaged" => Style::default().fg(Color::DarkGray),
        "dirty" | "prunable" => Style::default().fg(Color::Yellow),
        "rebase" | "merge" | "conflicts" => Style::default().fg(Color::Red),
        "locked" => Style::default().fg(Color::Magenta),
        "detached" | "loading" => Style::default().fg(Color::DarkGray),
        "error" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::White),
    }
}

fn key_hint(label: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {} ", label),
        Style::default()
            .fg(Color::Black)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        compact_tmux_style, muted_row_text_style, selected_name_style, selected_row_style,
    };
    use crate::tui::worktree::app::TmuxState;
    use ratatui::style::{Color, Modifier, Style};

    #[test]
    fn selected_row_uses_light_foreground_on_highlight() {
        assert_eq!(
            selected_row_style(),
            Style::default().fg(Color::White).bg(Color::DarkGray)
        );
        assert_eq!(
            muted_row_text_style(true),
            selected_row_style().fg(Color::White)
        );
    }

    #[test]
    fn selected_name_style_keeps_current_entry_emphasis() {
        assert_eq!(
            selected_name_style(true),
            selected_row_style()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        );
        assert_eq!(
            selected_name_style(false),
            selected_row_style().fg(Color::Cyan)
        );
    }

    #[test]
    fn selected_tmux_styles_avoid_dark_gray_on_highlight() {
        assert_eq!(
            compact_tmux_style(&TmuxState::Missing, true),
            selected_row_style().fg(Color::White)
        );
        assert_eq!(
            compact_tmux_style(&TmuxState::Detached, true),
            selected_row_style().fg(Color::Cyan)
        );
    }
}
