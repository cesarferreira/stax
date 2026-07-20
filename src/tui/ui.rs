use crate::tui::app::{App, ConfirmAction, FocusedPane, InputAction, Mode, PaneVisibility};
use crate::tui::widgets::{render_details, render_diff, render_reorder_preview, render_stack_tree};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DashboardLayout {
    stack: Option<Rect>,
    summary: Option<Rect>,
    patch: Option<Rect>,
    status: Rect,
}

fn dashboard_layout(area: Rect, visibility: PaneVisibility) -> DashboardLayout {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),    // Main content
            Constraint::Length(4), // Status bar
        ])
        .split(area);

    let main = chunks[0];
    let status = chunks[1];

    match (visibility.stack, visibility.summary, visibility.patch) {
        (true, true, true) => {
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(main);

            let left_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(8),     // stack
                    Constraint::Length(10), // branch summary
                ])
                .split(main_chunks[0]);

            DashboardLayout {
                stack: Some(left_chunks[0]),
                summary: Some(left_chunks[1]),
                patch: Some(main_chunks[1]),
                status,
            }
        }
        (true, true, false) => {
            let chunks = stack_summary_layout(main);
            DashboardLayout {
                stack: Some(chunks[0]),
                summary: Some(chunks[1]),
                patch: None,
                status,
            }
        }
        (true, false, true) => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(main);
            DashboardLayout {
                stack: Some(chunks[0]),
                summary: None,
                patch: Some(chunks[1]),
                status,
            }
        }
        (false, true, true) => {
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
                .split(main);
            DashboardLayout {
                stack: None,
                summary: Some(chunks[0]),
                patch: Some(chunks[1]),
                status,
            }
        }
        (true, false, false) => DashboardLayout {
            stack: Some(main),
            summary: None,
            patch: None,
            status,
        },
        (false, true, false) => DashboardLayout {
            stack: None,
            summary: Some(main),
            patch: None,
            status,
        },
        (false, false, true) => DashboardLayout {
            stack: None,
            summary: None,
            patch: Some(main),
            status,
        },
        (false, false, false) => DashboardLayout {
            stack: Some(main),
            summary: None,
            patch: None,
            status,
        },
    }
}

fn stack_summary_layout(area: Rect) -> [Rect; 2] {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),     // stack
            Constraint::Length(10), // branch summary
        ])
        .split(area);
    [chunks[0], chunks[1]]
}

/// Main UI render function
pub fn render(f: &mut Frame, app: &App) {
    let show_reorder_preview = matches!(app.mode, Mode::Reorder)
        || matches!(app.mode, Mode::Confirm(ConfirmAction::ApplyReorder));
    let layout = dashboard_layout(f.area(), app.pane_visibility);

    if let Some(area) = layout.stack {
        render_stack_tree(f, app, area);
    }
    if let Some(area) = layout.summary {
        render_details(f, app, area);
    }

    if let Some(area) = layout.patch {
        if show_reorder_preview {
            render_reorder_preview(f, app, area);
        } else {
            render_diff(f, app, area);
        }
    }

    // Status bar
    render_status_bar(f, app, layout.status);

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
                    FocusedPane::Summary => (" SUMMARY ", Color::Blue, "inspect branch"),
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
    spans.push(key_hint("1/2/3", Color::Blue));
    spans.push(Span::raw(" panes  "));
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
        Line::from("  ↑/k/Ctrl+p  Move selection up"),
        Line::from("  ↓/j/Ctrl+n  Move selection down"),
        Line::from("  Enter    Checkout selected branch"),
        Line::from("  Tab      Switch focus to patch scrolling"),
        Line::from("  1        Show/hide Stack pane"),
        Line::from("  2        Show/hide Summary pane"),
        Line::from("  3        Show/hide Patch pane"),
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
            "Text Entry (rename, new branch)",
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Ctrl+a/e   Jump to start/end of line"),
        Line::from("  Ctrl+f/b   Move cursor right/left"),
        Line::from("  Ctrl+d     Delete character under cursor"),
        Line::from("  Ctrl+k     Delete to end of line"),
        Line::from("  Ctrl+g     Cancel (same as Esc)"),
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
        // name→column lookup (HashMap avoids repeated linear scans).
        let column_map: HashMap<&str, usize> = app
            .branches
            .iter()
            .map(|b| (b.name.as_str(), b.column))
            .collect();
        let col_of = |name: &str| column_map.get(name).copied().unwrap_or(0);

        let max_col = filtered
            .iter()
            .map(|i| col_of(&app.move_picker_candidates[*i]))
            .max()
            .unwrap_or(0);

        // Scroll window: keep the selected row roughly centered.
        const MAX_VISIBLE: usize = 20;
        let start = selected.saturating_sub(MAX_VISIBLE / 2);
        let end = (start + MAX_VISIBLE).min(filtered.len());
        let start = end.saturating_sub(MAX_VISIBLE);

        for (row, filter_idx) in filtered[start..end].iter().enumerate() {
            let absolute_row = start + row;
            let name = &app.move_picker_candidates[*filter_idx];
            let is_selected = absolute_row == selected;

            let tree = build_tree_prefix(col_of(name), max_col, is_selected);

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

/// Build the tree-connector prefix for a branch at the given `column` depth.
///
/// Output shape: `▸│ │ ○   ` — a selection marker, one `│ ` per ancestor
/// column, `○` at the branch's own column, then spaces to pad to
/// `max_column` width so all rows align.
///
/// Used by the move-picker modal; the main stack view
/// (`render_stack_tree`) has the same logic inlined with an additional
/// `is_current` flag. Extracted here so the pattern is testable.
pub(crate) fn build_tree_prefix(column: usize, max_column: usize, is_selected: bool) -> String {
    let mut s = String::new();
    s.push(if is_selected { '▸' } else { ' ' });
    for c in 0..=column {
        if c == column {
            s.push('○');
        } else {
            s.push_str("│ ");
        }
    }
    let tree_width = column * 2 + 2;
    let target_width = (max_column + 1) * 2 + 2;
    for _ in tree_width..target_width {
        s.push(' ');
    }
    s
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

#[cfg(test)]
mod tests {
    use super::{build_tree_prefix, dashboard_layout};
    use crate::tui::app::PaneVisibility;
    use ratatui::layout::Rect;

    #[test]
    fn dashboard_layout_places_summary_under_stack() {
        let layout = dashboard_layout(Rect::new(0, 0, 100, 40), PaneVisibility::default());
        let stack = layout.stack.expect("stack pane");
        let summary = layout.summary.expect("summary pane");
        let patch = layout.patch.expect("patch pane");

        assert_eq!(stack.x, 0);
        assert_eq!(summary.x, 0);
        assert_eq!(summary.y, stack.y + stack.height);
        assert_eq!(summary.width, stack.width);
        assert!(patch.x > stack.x);
        assert_eq!(patch.y, 0);
        assert_eq!(patch.height, 36);
    }

    #[test]
    fn dashboard_layout_keeps_reorder_preview_on_right() {
        let layout = dashboard_layout(Rect::new(0, 0, 100, 40), PaneVisibility::default());
        let summary = layout.summary.expect("summary pane");
        let patch = layout.patch.expect("patch pane");

        assert_eq!(summary.x, layout.stack.expect("stack pane").x);
        assert!(patch.x > summary.x);
        assert_eq!(patch.y, 0);
        assert_eq!(patch.height, 36);
    }

    #[test]
    fn dashboard_layout_expands_stack_when_summary_and_patch_are_hidden() {
        let layout = dashboard_layout(
            Rect::new(0, 0, 100, 40),
            PaneVisibility {
                stack: true,
                summary: false,
                patch: false,
            },
        );

        assert_eq!(layout.stack, Some(Rect::new(0, 0, 100, 36)));
        assert_eq!(layout.summary, None);
        assert_eq!(layout.patch, None);
        assert_eq!(layout.status, Rect::new(0, 36, 100, 4));
    }

    #[test]
    fn dashboard_layout_expands_stack_and_summary_when_patch_is_hidden() {
        let layout = dashboard_layout(
            Rect::new(0, 0, 100, 40),
            PaneVisibility {
                stack: true,
                summary: true,
                patch: false,
            },
        );
        let stack = layout.stack.expect("stack pane");
        let summary = layout.summary.expect("summary pane");

        assert_eq!(stack.width, 100);
        assert_eq!(summary.width, 100);
        assert_eq!(summary.y, 26);
        assert_eq!(summary.height, 10);
        assert_eq!(layout.patch, None);
    }

    #[test]
    fn tree_prefix_root_column() {
        // Column 0, max 0: marker + circle + 2 padding chars.
        assert_eq!(build_tree_prefix(0, 0, false), " ○  ");
        assert_eq!(build_tree_prefix(0, 0, true), "▸○  ");
    }

    #[test]
    fn tree_prefix_nested_column() {
        // Column 2, max 2: marker + ancestor pipes + circle + padding.
        assert_eq!(build_tree_prefix(2, 2, false), " │ │ ○  ");
        // Column 1, max 2: shallower branch gets more trailing padding.
        assert_eq!(build_tree_prefix(1, 2, true), "▸│ ○    ");
    }

    #[test]
    fn tree_prefix_padding_aligns_to_max_column() {
        // All rows at different depths should produce the same visual
        // width so branch names start in the same column.
        let w0 = build_tree_prefix(0, 2, false).chars().count();
        let w1 = build_tree_prefix(1, 2, false).chars().count();
        let w2 = build_tree_prefix(2, 2, false).chars().count();
        assert_eq!(w0, w1);
        assert_eq!(w1, w2);
    }
}
