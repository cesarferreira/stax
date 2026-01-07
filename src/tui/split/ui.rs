use super::app::{SplitApp, SplitMode};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

/// Render the split TUI
pub fn render(f: &mut Frame, app: &SplitApp) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(f.area());

    render_commits(f, app, chunks[0]);
    render_preview(f, app, chunks[1]);

    // Render overlays based on mode
    match &app.mode {
        SplitMode::Naming => render_naming_dialog(f, app),
        SplitMode::Confirm => render_confirm_dialog(f, app),
        SplitMode::Help => render_help_dialog(f),
        SplitMode::Normal => {}
    }
}

fn render_commits(f: &mut Frame, app: &SplitApp, area: Rect) {
    let mut items: Vec<ListItem> = Vec::new();

    for (i, commit) in app.commits.iter().enumerate() {
        let is_selected = i == app.selected_index;
        let has_split = app.split_points.iter().any(|sp| sp.after_commit_index == i);

        // Build the commit line
        let mut spans = vec![];

        // Selection indicator
        if is_selected {
            spans.push(Span::styled("► ", Style::default().fg(Color::Yellow)));
        } else {
            spans.push(Span::raw("  "));
        }

        // Commit hash
        spans.push(Span::styled(
            format!("{} ", commit.short_sha),
            Style::default().fg(Color::Cyan),
        ));

        // Commit message
        let msg_style = if is_selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(&commit.message, msg_style));

        let mut item = ListItem::new(Line::from(spans));
        if is_selected {
            item = item.style(Style::default().bg(Color::DarkGray));
        }
        items.push(item);

        // Add split marker line after this commit if there's a split point
        if has_split {
            if let Some(sp) = app.split_points.iter().find(|sp| sp.after_commit_index == i) {
                let split_line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled("──── ", Style::default().fg(Color::Green)),
                    Span::styled(
                        format!("split: {} ", sp.branch_name),
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("────", Style::default().fg(Color::Green)),
                ]);
                items.push(ListItem::new(split_line));
            }
        }
    }

    let title = format!(
        " Commits on '{}' ({} total) ",
        app.current_branch,
        app.commits.len()
    );

    let help_text = if app.status_message.is_some() {
        app.status_message.as_ref().unwrap().clone()
    } else {
        "s: split | d: remove | Enter: apply | ?: help | q: quit".to_string()
    };

    let block = Block::default()
        .title(title)
        .title_bottom(Line::from(help_text).centered())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_preview(f: &mut Frame, app: &SplitApp, area: Rect) {
    let preview = app.build_preview();

    let mut items: Vec<ListItem> = Vec::new();

    // Show parent at top
    items.push(ListItem::new(Line::from(vec![
        Span::styled(&app.parent_branch, Style::default().fg(Color::DarkGray)),
    ])));

    // Show each resulting branch
    for (i, branch) in preview.iter().enumerate() {
        let indent = "  ".repeat(i + 1);
        let connector = if i == preview.len() - 1 { "└─" } else { "├─" };

        let is_current = branch.name == app.current_branch;
        let name_style = if is_current {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Green)
        };

        let line = Line::from(vec![
            Span::raw(format!("{}{} ", indent, connector)),
            Span::styled(&branch.name, name_style),
            Span::styled(
                format!(" ({} commit{})", branch.commit_count, if branch.commit_count == 1 { "" } else { "s" }),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        items.push(ListItem::new(line));
    }

    if preview.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(
                "  No split points defined",
                Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
            ),
        ])));
    }

    let block = Block::default()
        .title(" Preview ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn render_naming_dialog(f: &mut Frame, app: &SplitApp) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Enter branch name ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Input field
    let input_area = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(0)])
        .split(inner);

    let input_text = Paragraph::new(app.input_buffer.as_str())
        .style(Style::default().fg(Color::White));
    f.render_widget(input_text, input_area[0]);

    // Show cursor
    f.set_cursor_position((
        input_area[0].x + app.input_cursor as u16,
        input_area[0].y,
    ));

    let hint = Paragraph::new("Enter to confirm, Esc to cancel")
        .style(Style::default().fg(Color::DarkGray));
    f.render_widget(hint, input_area[1]);
}

fn render_confirm_dialog(f: &mut Frame, app: &SplitApp) {
    let preview = app.build_preview();
    let height = (preview.len() + 6).min(15) as u16;
    let area = centered_rect(60, height, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Confirm Split ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut lines = vec![
        Line::from(Span::styled(
            format!("Create {} new branch(es):", app.split_points.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    for sp in &app.split_points {
        lines.push(Line::from(vec![
            Span::raw("  • "),
            Span::styled(&sp.branch_name, Style::default().fg(Color::Green)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Proceed? ", Style::default()),
        Span::styled("(y/n)", Style::default().fg(Color::Yellow)),
    ]));

    let text = Paragraph::new(lines);
    f.render_widget(text, inner);
}

fn render_help_dialog(f: &mut Frame) {
    let area = centered_rect(50, 60, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .title(" Help ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));

    let inner = block.inner(area);
    f.render_widget(block, area);

    let help_text = vec![
        Line::from(Span::styled("Navigation", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  j/↓    Move down"),
        Line::from("  k/↑    Move up"),
        Line::from(""),
        Line::from(Span::styled("Actions", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  s      Mark split point at cursor"),
        Line::from("  d      Remove split point at cursor"),
        Line::from("  S-J/K  Move split point down/up"),
        Line::from("  Enter  Execute split"),
        Line::from(""),
        Line::from(Span::styled("Other", Style::default().add_modifier(Modifier::BOLD))),
        Line::from("  ?      Toggle help"),
        Line::from("  q/Esc  Quit"),
        Line::from(""),
        Line::from(Span::styled("Press any key to close", Style::default().fg(Color::DarkGray))),
    ];

    let text = Paragraph::new(help_text);
    f.render_widget(text, inner);
}

/// Create a centered rectangle
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
