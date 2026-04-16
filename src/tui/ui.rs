use crate::tui::app::{App, ConfirmAction, FocusedPane, InputAction, Mode};
use crate::tui::widgets::{render_details, render_diff, render_reorder_preview, render_stack_tree};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Main UI render function
pub fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Main content
            Constraint::Length(4), // Status bar
        ])
        .split(f.area());

    // Main content: left panel (stack) + right panel (summary + diff/reorder preview)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(chunks[0]);

    render_stack_tree(f, app, main_chunks[0]);

    if matches!(app.mode, Mode::Reorder)
        || matches!(app.mode, Mode::Confirm(ConfirmAction::ApplyReorder))
    {
        render_reorder_preview(f, app, main_chunks[1]);
    } else {
        let right_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // branch summary
                Constraint::Min(3),     // patch view
            ])
            .split(main_chunks[1]);

        render_details(f, app, right_chunks[0]);
        render_diff(f, app, right_chunks[1]);
    }

    // Status bar
    render_status_bar(f, app, chunks[1]);

    // Modal overlays
    match &app.mode {
        Mode::Help => render_help_modal(f),
        Mode::Confirm(action) => render_confirm_modal(f, action),
        Mode::Input(action) => render_input_modal(f, action, &app.input_buffer, app.input_cursor),
        Mode::MovePicker => render_move_picker_modal(f, app),
        _ => {}
    }
}

/// Render the bottom status bar with keybindings
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(
            msg.clone(),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        match app.mode {
            Mode::Normal => {
                let (focus_label, focus_color, focus_hint) = match app.focused_pane {
                    FocusedPane::Stack => (" STACK ", Color::Cyan, "browse branches"),
                    FocusedPane::Diff => (" PATCH ", Color::Green, "scroll patch"),
                };
                let branch_count = app.branches.len();
                Line::from(vec![
                    Span::styled(
                        focus_label,
                        Style::default()
                            .fg(Color::Black)
                            .bg(focus_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(
                        format!("{} branches", branch_count),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(" • ", Style::default().fg(Color::DarkGray)),
                    Span::styled(focus_hint, Style::default().fg(Color::DarkGray)),
                ])
            }
            Mode::Search => Line::from(vec![
                Span::styled("/", Style::default().fg(Color::Cyan)),
                Span::raw(" filtering branches  "),
                Span::styled("Type", Style::default().fg(Color::Cyan)),
                Span::raw(" to narrow  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" close search"),
            ]),
            Mode::Help => Line::from("Press any key to close"),
            Mode::Confirm(_) => Line::from(vec![
                Span::styled("y", Style::default().fg(Color::Cyan)),
                Span::raw(" confirm  "),
                Span::styled("n/Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ]),
            Mode::Input(_) => Line::from(vec![
                Span::styled("⏎", Style::default().fg(Color::Cyan)),
                Span::raw(" confirm  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ]),
            Mode::Reorder => Line::from(vec![
                Span::styled(
                    " ◀ REORDER ▶ ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  "),
                Span::styled("Shift+↑/↓", Style::default().fg(Color::Magenta)),
                Span::raw(" move branch in stack  "),
                Span::styled("Enter", Style::default().fg(Color::Cyan)),
                Span::raw(" apply  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel"),
            ]),
            Mode::MovePicker => Line::from(vec![
                Span::styled(
                    " MOVE ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("  pick new parent for "),
                Span::styled(
                    format!("'{}'", app.move_picker_source),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
        }
    };

    let shortcuts_line = match app.mode {
        Mode::Normal => build_normal_shortcuts(app),
        Mode::Search => Line::from(vec![
            key_hint("↑↓", Color::Cyan),
            Span::raw(" navigate  "),
            key_hint("Enter", Color::Green),
            Span::raw(" checkout  "),
            key_hint("Esc", Color::Cyan),
            Span::raw(" cancel"),
        ]),
        Mode::Help => Line::from(vec![Span::styled(
            "? closes this dialog",
            Style::default().fg(Color::DarkGray),
        )]),
        Mode::Confirm(_) => Line::from(vec![
            key_hint("y", Color::Green),
            Span::raw(" confirm  "),
            key_hint("Esc", Color::Red),
            Span::raw(" cancel"),
        ]),
        Mode::Input(_) => Line::from(vec![
            key_hint("Enter", Color::Green),
            Span::raw(" accept  "),
            key_hint("Esc", Color::Red),
            Span::raw(" cancel"),
        ]),
        Mode::Reorder => Line::from(vec![
            key_hint("Shift+↑↓", Color::Magenta),
            Span::raw(" move  "),
            key_hint("Enter", Color::Green),
            Span::raw(" apply  "),
            key_hint("Esc", Color::Red),
            Span::raw(" cancel"),
        ]),
        Mode::MovePicker => Line::from(vec![
            key_hint("Type", Color::Cyan),
            Span::raw(" filter  "),
            key_hint("↑↓", Color::Cyan),
            Span::raw(" select  "),
            key_hint("Enter", Color::Green),
            Span::raw(" move  "),
            key_hint("Esc", Color::Red),
            Span::raw(" cancel"),
        ]),
    };

    let paragraph = Paragraph::new(vec![status_line, shortcuts_line])
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(paragraph, area);
}

fn build_normal_shortcuts(app: &App) -> Line<'static> {
    let mut spans = vec![
        key_hint("↑↓", Color::Cyan),
        Span::raw(" move  "),
        key_hint("Tab", Color::Cyan),
        Span::raw(" pane  "),
    ];

    if let Some(branch) = app.selected_branch() {
        let (label, action, color) = if !branch.is_current {
            ("Enter", "checkout", Color::Green)
        } else if branch.is_trunk {
            ("n", "new", Color::Green)
        } else if branch.needs_restack {
            ("r", "restack", Color::Yellow)
        } else if branch.pr_number.is_some() {
            ("p", "PR", Color::Cyan)
        } else {
            ("s", "submit", Color::Green)
        };

        spans.push(key_hint(label, color));
        spans.push(Span::raw(format!(" {}  ", action)));
    }

    spans.push(key_hint("/", Color::Cyan));
    spans.push(Span::raw(" search  "));
    spans.push(key_hint("m", Color::Magenta));
    spans.push(Span::raw(" move  "));
    spans.push(key_hint("?", Color::Yellow));
    spans.push(Span::raw(" help  "));
    spans.push(key_hint("q", Color::Cyan));
    spans.push(Span::raw(" quit"));

    Line::from(spans)
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

/// Render help modal
fn render_help_modal(f: &mut Frame) {
    let area = centered_rect(60, 70, f.area());

    let help_text = vec![
        Line::from(vec![Span::styled(
            "Stax TUI Help",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ↑/k      Move selection up"),
        Line::from("  ↓/j      Move selection down"),
        Line::from("  Enter    Checkout selected branch"),
        Line::from("  Tab      Switch focus to patch scrolling"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Actions",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  r        Restack selected branch"),
        Line::from("  R        Restack all branches"),
        Line::from("  s        Submit stack (push + create PRs)"),
        Line::from("  p        Open PR in browser"),
        Line::from("  n        Create new branch"),
        Line::from("  e        Rename current branch"),
        Line::from("  d        Delete selected branch"),
        Line::from("  o        Reorder stack (swap siblings)"),
        Line::from("  m        Move branch onto a new parent"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Reorder Mode (press 'o' to enter)",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Shift+↑/K  Move branch up in stack"),
        Line::from("  Shift+↓/J  Move branch down in stack"),
        Line::from("  Enter      Apply reparenting and restack"),
        Line::from("  Esc        Cancel reorder"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Move Mode (press 'm' to enter)",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Type       Filter candidate parents"),
        Line::from("  ↑/↓        Select candidate"),
        Line::from("  Enter      Reparent and run `upstack onto`"),
        Line::from("  Esc        Cancel"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Other",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  /        Search/filter branches"),
        Line::from("  ?        Show this help"),
        Line::from("  q/Esc    Quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close",
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let paragraph = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

/// Render confirmation modal
fn render_confirm_modal(f: &mut Frame, action: &ConfirmAction) {
    let area = centered_rect(50, 20, f.area());

    let message = match action {
        ConfirmAction::Delete(branch) => format!("Delete branch '{}'?", branch),
        ConfirmAction::Restack(branch) => format!("Restack '{}'?", branch),
        ConfirmAction::RestackAll => "Restack all branches?".to_string(),
        ConfirmAction::ApplyReorder => "Apply reorder and restack affected branches?".to_string(),
    };

    let content = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            message,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(Color::Green)),
            Span::raw(" confirm    "),
            Span::styled("n/Esc", Style::default().fg(Color::Red)),
            Span::raw(" cancel"),
        ]),
    ];

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Confirm ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

/// Render input modal
fn render_input_modal(f: &mut Frame, action: &InputAction, input: &str, cursor: usize) {
    let area = centered_rect(50, 25, f.area());

    let title = match action {
        InputAction::Rename => " Rename Branch ",
        InputAction::NewBranch => " New Branch ",
    };

    let prompt = match action {
        InputAction::Rename => "Enter new branch name:",
        InputAction::NewBranch => "Enter branch name:",
    };

    // Split input at cursor position
    let (before, after) = input.split_at(cursor.min(input.len()));

    let content = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            prompt,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::styled("> ", Style::default().fg(Color::Cyan)),
            Span::styled(before, Style::default().fg(Color::White)),
            Span::styled(
                "│",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::SLOW_BLINK),
            ),
            Span::styled(after, Style::default().fg(Color::White)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("←→ move  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Home/End  ", Style::default().fg(Color::DarkGray)),
            Span::styled("⏎ confirm  ", Style::default().fg(Color::DarkGray)),
            Span::styled("Esc cancel", Style::default().fg(Color::DarkGray)),
        ]),
    ];

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

/// Render the move-picker modal (parent selector for `gt move`).
///
/// Layout: a centered 60×60 box. First line is a bold prompt naming the
/// source branch; second is the live query; remaining lines list filtered
/// candidates with the selected one highlighted. Trailing line is the
/// shortcut hint. The list truncates to fit — users scroll with ↑/↓.
fn render_move_picker_modal(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 60, f.area());

    let filtered = app.move_picker_filtered_indices();
    let selected = app.move_picker_selected;

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(Line::from(vec![
        Span::styled("Move ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("'{}'", app.move_picker_source),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            " (and its descendants) onto:",
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Cyan)),
        Span::styled(
            app.move_picker_query.clone(),
            Style::default().fg(Color::White),
        ),
        Span::styled(
            "│",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::SLOW_BLINK),
        ),
    ]));
    lines.push(Line::from(""));

    if filtered.is_empty() {
        lines.push(Line::from(Span::styled(
            "  (no matches)",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        // Build a name→column lookup so candidates render with tree
        // connectors (same visual language as the main stack view).
        let column_of = |name: &str| -> usize {
            app.branches
                .iter()
                .find(|b| b.name == name)
                .map_or(0, |b| b.column)
        };

        // Max column across visible candidates, for alignment.
        let max_col = filtered
            .iter()
            .map(|i| column_of(&app.move_picker_candidates[*i]))
            .max()
            .unwrap_or(0);
        let tree_target_width = (max_col + 1) * 2 + 2;

        // Scroll window: keep the selected row roughly centered.
        const MAX_VISIBLE: usize = 20;
        let start = selected.saturating_sub(MAX_VISIBLE / 2);
        let end = (start + MAX_VISIBLE).min(filtered.len());
        let start = end.saturating_sub(MAX_VISIBLE);

        for (row, filter_idx) in filtered[start..end].iter().enumerate() {
            let absolute_row = start + row;
            let name = &app.move_picker_candidates[*filter_idx];
            let col = column_of(name);
            let is_selected = absolute_row == selected;

            // Tree prefix: "│ " for each ancestor level, then "○" at the
            // branch's own column — same pattern as render_stack_tree.
            let mut tree = String::new();
            tree.push(if is_selected { '▸' } else { ' ' });
            for c in 0..=col {
                if c == col {
                    tree.push('○');
                } else {
                    tree.push_str("│ ");
                }
            }
            let tree_width = col * 2 + 2;
            for _ in tree_width..tree_target_width {
                tree.push(' ');
            }

            let style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(vec![
                Span::styled(tree, style),
                Span::styled(name.clone(), style),
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Type", Style::default().fg(Color::DarkGray)),
        Span::styled(" filter  ", Style::default().fg(Color::DarkGray)),
        Span::styled("↑↓", Style::default().fg(Color::DarkGray)),
        Span::styled(" select  ", Style::default().fg(Color::DarkGray)),
        Span::styled("⏎", Style::default().fg(Color::DarkGray)),
        Span::styled(" move  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Esc", Style::default().fg(Color::DarkGray)),
        Span::styled(" cancel", Style::default().fg(Color::DarkGray)),
    ]));

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Move onto ")
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

/// Create a centered rectangle
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
