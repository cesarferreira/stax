use crate::tui::app::{App, ConfirmAction, Mode};
use crate::tui::widgets::{render_details, render_stack_tree};
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

    // Main content: stack tree (left) + details (right)
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);

    render_stack_tree(f, app, main_chunks[0]);
    render_details(f, app, main_chunks[1]);

    // Status bar
    render_status_bar(f, app, chunks[1]);

    // Modal overlays
    match &app.mode {
        Mode::Help => render_help_modal(f),
        Mode::Confirm(action) => render_confirm_modal(f, action),
        _ => {}
    }
}

/// Render the bottom status bar with keybindings
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let (content, style) = if let Some(msg) = &app.status_message {
        (
            msg.clone(),
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )
    } else {
        let bindings = match app.mode {
            Mode::Normal => {
                "↑↓/jk navigate  ⏎ checkout  r restack  s submit  p PR  n new  e rename  d delete  / search  ? help  q quit"
            }
            Mode::Search => "↑↓ navigate  ⏎ select  Esc cancel  Type to filter...",
            Mode::Help => "Press any key to close",
            Mode::Confirm(_) => "y confirm  n/Esc cancel",
        };
        (bindings.to_string(), Style::default().fg(Color::White))
    };

    let paragraph = Paragraph::new(content)
        .style(style)
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
