use crate::tui::app::{App, FocusedPane, Mode};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

/// Render the stack tree widget (left panel)
pub fn render_stack_tree(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_pane == FocusedPane::Stack;
    let search_active = app.mode == Mode::Search;
    let branches = if search_active {
        app.filtered_indices
            .iter()
            .map(|&idx| &app.branches[idx])
            .collect::<Vec<_>>()
    } else {
        app.branches.iter().collect::<Vec<_>>()
    };

    // Find max column for proper alignment
    let max_column = branches.iter().map(|b| b.column).max().unwrap_or(0);

    let items: Vec<ListItem> = if branches.is_empty() {
        vec![ListItem::new(Line::from(vec![Span::styled(
            if search_active && !app.search_query.is_empty() {
                format!("No branches match '/{}'", app.search_query)
            } else {
                "No branches found".to_string()
            },
            Style::default().fg(Color::DarkGray),
        )]))]
    } else {
        branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let is_selected = i == app.selected_index;

                let mut tree = String::new();
                tree.push(if is_selected { '▶' } else { ' ' });

                for col in 0..=branch.column {
                    if col == branch.column {
                        tree.push_str(if branch.is_current { "◉" } else { "○" });
                    } else {
                        tree.push_str("│ ");
                    }
                }

                let tree_width = branch.column * 2 + 2;
                let target_width = (max_column + 1) * 2 + 2;
                for _ in tree_width..target_width {
                    tree.push(' ');
                }

                let mut status_spans: Vec<Span> = Vec::new();

                if branch.unpushed > 0 {
                    status_spans.push(Span::styled(
                        format!(" {}⬆", branch.unpushed),
                        Style::default().fg(Color::Yellow),
                    ));
                }

                if branch.unpulled > 0 {
                    status_spans.push(Span::styled(
                        format!(" {}⬇", branch.unpulled),
                        Style::default().fg(Color::Magenta),
                    ));
                }

                if branch.has_remote && branch.unpushed == 0 && branch.unpulled == 0 {
                    status_spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
                }

                if branch.needs_restack {
                    status_spans.push(Span::styled(" ⟳", Style::default().fg(Color::Red)));
                }

                if let Some(pr_num) = branch.pr_number {
                    status_spans.push(Span::styled(
                        format!(" #{}", pr_num),
                        Style::default().fg(Color::Cyan),
                    ));
                }

                if let Some(ci) = &branch.ci_state {
                    let (icon, color) = match ci.as_str() {
                        "success" => ("●", Color::Green),
                        "failure" | "error" => ("●", Color::Red),
                        "pending" => ("●", Color::Yellow),
                        _ => ("●", Color::DarkGray),
                    };
                    status_spans.push(Span::styled(
                        format!(" {}", icon),
                        Style::default().fg(color),
                    ));
                }

                if let Some(progress) = app.ci_row_progress(&branch.name) {
                    status_spans.push(Span::styled(
                        format!(" {}", progress),
                        Style::default().fg(Color::Yellow),
                    ));
                }

                let branch_style = if branch.is_current {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if branch.is_trunk {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default()
                };

                let tree_style = Style::default().fg(Color::DarkGray);

                let mut line_spans = vec![
                    Span::styled(tree, tree_style),
                    Span::styled(&branch.name, branch_style),
                ];
                line_spans.extend(status_spans);
                let line = Line::from(line_spans);

                let item_style = if is_selected {
                    Style::default().bg(Color::DarkGray)
                } else {
                    Style::default()
                };

                ListItem::new(line).style(item_style)
            })
            .collect()
    };

    let title = if search_active && !app.search_query.is_empty() {
        format!(" Stack /{} ({} matches) ", app.search_query, branches.len())
    } else if search_active {
        format!(" Stack (filter: all {}) ", branches.len())
    } else {
        format!(" Stack ({}) ", app.branches.len())
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

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, title_style))
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(Style::default());

    let mut state = ListState::default();
    state.select((!branches.is_empty()).then_some(app.selected_index));

    f.render_stateful_widget(list, area, &mut state);
}
