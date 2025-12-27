use crate::tui::app::{App, ConfirmAction, FocusedPane, InputAction, Mode};
use crate::tui::widgets::{render_details, render_diff, render_stack_tree};
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
            Constraint::Length(3), // Status bar
        ])
        .split(f.area());

    // Main content: left panel (stack + details) + right panel (diff)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(chunks[0]);

    // Left panel: stack tree (top) + details (bottom)
    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(main_chunks[0]);

    render_stack_tree(f, app, left_chunks[0]);
    render_details(f, app, left_chunks[1]);
    render_diff(f, app, main_chunks[1]);

    // Status bar
    render_status_bar(f, app, chunks[1]);

    // Modal overlays
    match &app.mode {
        Mode::Help => render_help_modal(f),
        Mode::Confirm(action) => render_confirm_modal(f, action),
        Mode::Input(action) => render_input_modal(f, action, &app.input_buffer, app.input_cursor),
        _ => {}
    }
}

/// Render the bottom status bar with keybindings
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let content: Line = if let Some(msg) = &app.status_message {
        Line::from(Span::styled(
            msg.clone(),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))
    } else {
        match app.mode {
            Mode::Normal => {
                let (focus_label, focus_color) = match app.focused_pane {
                    FocusedPane::Stack => ("◀ STACK", Color::Cyan),
                    FocusedPane::Diff => ("DIFF ▶", Color::Green),
                };
                Line::from(vec![
                    Span::styled(
                        format!(" {} ", focus_label),
                        Style::default()
                            .fg(Color::Black)
                            .bg(focus_color)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw("  "),
                    Span::styled("Tab", Style::default().fg(Color::Cyan)),
                    Span::raw(" switch  "),
                    Span::styled("↑↓", Style::default().fg(Color::Cyan)),
                    Span::raw(" navigate  "),
                    Span::styled("⏎", Style::default().fg(Color::Cyan)),
                    Span::raw(" checkout  "),
                    Span::styled("r", Style::default().fg(Color::Cyan)),
                    Span::raw(" restack  "),
                    Span::styled("s", Style::default().fg(Color::Cyan)),
                    Span::raw(" submit  "),
                    Span::styled("n", Style::default().fg(Color::Cyan)),
                    Span::raw(" new  "),
                    Span::styled("e", Style::default().fg(Color::Cyan)),
                    Span::raw(" rename  "),
                    Span::styled("/", Style::default().fg(Color::Cyan)),
                    Span::raw(" search  "),
                    Span::styled("?", Style::default().fg(Color::Cyan)),
                    Span::raw(" help  "),
                    Span::styled("q", Style::default().fg(Color::Cyan)),
                    Span::raw(" quit"),
                ])
            }
            Mode::Search => Line::from(vec![
                Span::styled("↑↓", Style::default().fg(Color::Cyan)),
                Span::raw(" navigate  "),
                Span::styled("⏎", Style::default().fg(Color::Cyan)),
                Span::raw(" select  "),
                Span::styled("Esc", Style::default().fg(Color::Cyan)),
                Span::raw(" cancel  Type to filter..."),
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
        }
    };

    let paragraph = Paragraph::new(content)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(paragraph, area);
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
            Span::styled("│", Style::default().fg(Color::Cyan).add_modifier(Modifier::SLOW_BLINK)),
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
