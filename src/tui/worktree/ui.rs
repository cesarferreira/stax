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
                .details
                .branch_label
                .split('/')
                .next_back()
                .unwrap_or(&record.details.branch_label);

            let mut spans = vec![
                Span::styled(
                    indicator,
                    if selected {
                        Style::default().fg(Color::Yellow)
                    } else {
                        Style::default()
                    },
                ),
                Span::styled(
                    format!("{:<18}", record.details.info.name),
                    if record.details.info.is_current {
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
                Span::raw(" "),
                Span::styled(branch.to_string(), Style::default().fg(Color::DarkGray)),
            ];

            match record.tmux_state {
                TmuxState::Attached(_) => spans.push(Span::styled(
                    "  tmux:attached",
                    Style::default().fg(Color::Green),
                )),
                TmuxState::Detached => spans.push(Span::styled(
                    "  tmux:ready",
                    Style::default().fg(Color::Blue),
                )),
                TmuxState::Missing => spans.push(Span::styled(
                    "  tmux:new",
                    Style::default().fg(Color::DarkGray),
                )),
                TmuxState::Unavailable => {
                    spans.push(Span::styled("  tmux:off", Style::default().fg(Color::Red)))
                }
            }

            let mut item = ListItem::new(Line::from(spans));
            if selected {
                item = item.style(Style::default().bg(Color::DarkGray));
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

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Name: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                &record.details.info.name,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Branch: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(record.details.branch_label.clone()),
        ]),
        Line::from(vec![
            Span::styled("Base: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                record
                    .details
                    .stack_parent
                    .clone()
                    .unwrap_or_else(|| "—".to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled("Path: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                record.details.info.path.display().to_string(),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Ahead/Behind: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "{} / {}",
                record
                    .details
                    .ahead
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                record
                    .details
                    .behind
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "—".to_string())
            )),
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

    if let Some(marker) = &record.details.marker {
        lines.push(Line::from(vec![
            Span::styled("Marker: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(marker.clone(), Style::default().fg(Color::Yellow)),
        ]));
    }

    lines.push(Line::from(vec![
        Span::styled("Labels: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(record.status_labels.join(", ")),
    ]));

    if record.details.info.is_locked {
        lines.push(Line::from(vec![
            Span::styled("Lock: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                record
                    .details
                    .info
                    .lock_reason
                    .clone()
                    .unwrap_or_else(|| "locked".to_string()),
            ),
        ]));
    }

    if record.details.info.is_prunable {
        lines.push(Line::from(vec![
            Span::styled("Prune: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(
                record
                    .details
                    .info
                    .prunable_reason
                    .clone()
                    .unwrap_or_else(|| "stale worktree entry".to_string()),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "Tmux-first workflow: Enter attaches/switches to the derived session, or creates it on demand.",
        Style::default().fg(Color::DarkGray),
    )]));

    let text = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(text, inner);
}

fn render_status_bar(f: &mut Frame, app: &WorktreeApp, area: Rect) {
    let status_line = if let Some(message) = &app.status_message {
        Line::from(Span::styled(
            message.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(Span::styled(
            "Tmux-first dashboard: browse lanes here, enter the session in tmux when ready.",
            Style::default().fg(Color::DarkGray),
        ))
    };

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
        .map(|record| record.details.info.name.clone())
        .unwrap_or_else(|| "this worktree".to_string());
    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("Remove ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(name, Style::default().fg(Color::Red)),
            Span::raw("?"),
        ]),
        Line::from(""),
        Line::from("Press y to confirm or Esc to cancel."),
    ];
    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(" Confirm Remove ")
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
        "detached" => Style::default().fg(Color::DarkGray),
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
