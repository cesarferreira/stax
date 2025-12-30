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
    let branches = if app.mode == Mode::Search && !app.filtered_indices.is_empty() {
        app.filtered_indices
            .iter()
            .map(|&idx| &app.branches[idx])
            .collect::<Vec<_>>()
    } else {
        app.branches.iter().collect::<Vec<_>>()
    };

    // Find max column for proper alignment
    let max_column = branches.iter().map(|b| b.column).max().unwrap_or(0);

    let items: Vec<ListItem> = branches
        .iter()
        .enumerate()
        .map(|(i, branch)| {
            let is_selected = i == app.selected_index;

            // Build tree graphics
            let mut tree = String::new();

            // Selection indicator
            if is_selected {
                tree.push('▶');
            } else {
                tree.push(' ');
            }

            // Tree structure
            for col in 0..=branch.column {
                if col == branch.column {
                    let circle = if branch.is_current { "◉" } else { "○" };
                    tree.push_str(circle);
                } else {
                    tree.push_str("│ ");
                }
            }

            // Pad for alignment
            let tree_width = branch.column * 2 + 2; // +2 for selection indicator and circle
            let target_width = (max_column + 1) * 2 + 2;
            for _ in tree_width..target_width {
                tree.push(' ');
            }

            // Build status spans with individual styling
            let mut status_spans: Vec<Span> = Vec::new();

            // Show unpushed commits (ahead of remote) - most important
            if branch.unpushed > 0 {
                status_spans.push(Span::styled(
                    format!(" {}⬆", branch.unpushed),
                    Style::default().fg(Color::Yellow),
                ));
            }

            // Show unpulled commits (behind remote)
            if branch.unpulled > 0 {
                status_spans.push(Span::styled(
                    format!(" {}⬇", branch.unpulled),
                    Style::default().fg(Color::Magenta),
                ));
            }

            // Show synced with remote indicator (cloud icon)
            if branch.has_remote && branch.unpushed == 0 && branch.unpulled == 0 {
                status_spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
            }

            // Needs restack indicator
            if branch.needs_restack {
                status_spans.push(Span::styled(" ⟳", Style::default().fg(Color::Red)));
            }

            // PR info
            if let Some(pr_num) = branch.pr_number {
                status_spans.push(Span::styled(
                    format!(" #{}", pr_num),
                    Style::default().fg(Color::Cyan),
                ));
            }

            // Build the line with styling
            let branch_style = if branch.is_current {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
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
        .collect();

    let title = if app.mode == Mode::Search {
        format!(" Stack (/{}) ", app.search_query)
    } else {
        " Stack ".to_string()
    };

    let (border_color, title_style) = if is_focused {
        (Color::Cyan, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
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
    state.select(Some(app.selected_index));

    f.render_stateful_widget(list, area, &mut state);
}
