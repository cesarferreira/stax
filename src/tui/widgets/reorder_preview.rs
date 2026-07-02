use crate::tui::app::{App, ReorderState};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

/// Render the reorder preview panel (replaces diff panel in reorder mode)
pub fn render_reorder_preview(f: &mut Frame, app: &App, area: Rect) {
    let content = if let Some(state) = &app.reorder_state {
        build_preview_content(state, app)
    } else {
        vec![Line::from("No reorder in progress")]
    };

    let paragraph = Paragraph::new(content)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " Restack Preview ",
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(paragraph, area);
}

fn build_preview_content(state: &ReorderState, app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    // Show what's being moved
    let moving_branch = state
        .pending_chain
        .get(state.moving_index)
        .map(|e| e.name.clone())
        .unwrap_or_default();

    lines.push(Line::from(vec![
        Span::styled("Moving: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            moving_branch.clone(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // Show position change
    let original_pos = state
        .original_chain
        .iter()
        .position(|e| e.name == moving_branch)
        .map(|p| p + 1)
        .unwrap_or(0);
    let new_pos = state.moving_index + 1;

    if original_pos != new_pos {
        lines.push(Line::from(vec![
            Span::styled("Position: ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{} → {}", original_pos, new_pos),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(" (in stack)", Style::default().fg(Color::DarkGray)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Position: ", Style::default().fg(Color::DarkGray)),
            Span::styled("unchanged", Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(Line::from(""));

    // Show stack chain comparison
    lines.push(Line::from(vec![Span::styled(
        "Stack Chain:",
        Style::default().add_modifier(Modifier::BOLD),
    )]));

    lines.push(Line::from(""));

    // Original chain
    lines.push(Line::from(vec![Span::styled(
        "Before:",
        Style::default().fg(Color::DarkGray),
    )]));

    // Show trunk at start
    let trunk = &app.stack.trunk;
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(trunk.clone(), Style::default().fg(Color::Blue)),
        Span::styled(" (trunk)", Style::default().fg(Color::DarkGray)),
    ]));

    for entry in &state.original_chain {
        let style = if entry.name == moving_branch {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(vec![
            Span::styled("  └─ ", Style::default().fg(Color::DarkGray)),
            Span::styled(entry.name.clone(), style),
        ]));
    }

    lines.push(Line::from(""));

    // New chain
    lines.push(Line::from(vec![Span::styled(
        "After:",
        Style::default().fg(Color::DarkGray),
    )]));

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(trunk.clone(), Style::default().fg(Color::Blue)),
        Span::styled(" (trunk)", Style::default().fg(Color::DarkGray)),
    ]));

    for (i, entry) in state.pending_chain.iter().enumerate() {
        let is_moving = i == state.moving_index;
        let style = if is_moving {
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let prefix = if is_moving { "  ► " } else { "  └─ " };
        let prefix_style = if is_moving {
            Color::Green
        } else {
            Color::DarkGray
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(prefix_style)),
            Span::styled(entry.name.clone(), style),
        ]));
    }

    lines.push(Line::from(""));

    // Show reparent operations
    let reparent_ops = app.get_reparent_operations();
    if !reparent_ops.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "Reparent operations:",
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        for (branch, new_parent) in &reparent_ops {
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(Color::Yellow)),
                Span::styled(branch.clone(), Style::default().fg(Color::Cyan)),
                Span::styled(" → parent: ", Style::default().fg(Color::DarkGray)),
                Span::styled(new_parent.clone(), Style::default().fg(Color::Blue)),
            ]));
        }
        lines.push(Line::from(""));
    }

    lines.push(Line::from(""));

    // Show commits to rebase
    if !state.preview.commits_to_rebase.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "Commits to rebase:",
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        for (branch, commits) in &state.preview.commits_to_rebase {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(branch.clone(), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!(
                        " ({} commit{})",
                        commits.len(),
                        if commits.len() == 1 { "" } else { "s" }
                    ),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));

            for commit in commits.iter().take(5) {
                let msg = if commit.len() > 45 {
                    format!("{}...", &commit[..42])
                } else {
                    commit.clone()
                };
                lines.push(Line::from(vec![
                    Span::styled("    • ", Style::default().fg(Color::DarkGray)),
                    Span::raw(msg),
                ]));
            }

            if commits.len() > 5 {
                lines.push(Line::from(vec![Span::styled(
                    format!("      +{} more commits", commits.len() - 5),
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }

        lines.push(Line::from(""));
    }

    // Show potential conflicts
    if !state.preview.potential_conflicts.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "⚠ Potential conflicts:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )]));
        lines.push(Line::from(""));

        for conflict in &state.preview.potential_conflicts {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(conflict.file.clone(), Style::default().fg(Color::Yellow)),
            ]));

            if !conflict.branches_involved.is_empty() {
                let branches = conflict.branches_involved.join(", ");
                lines.push(Line::from(vec![Span::styled(
                    format!("    modified in: {}", branches),
                    Style::default().fg(Color::DarkGray),
                )]));
            }
        }
    } else if !state.preview.commits_to_rebase.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "✓ No conflicts detected",
            Style::default().fg(Color::Green),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Instructions
    let has_changes = state.original_chain != state.pending_chain;
    if has_changes {
        lines.push(Line::from(vec![Span::styled(
            "Press Enter to apply changes and restack",
            Style::default().fg(Color::Cyan),
        )]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "Use Shift+↑/↓ to move the branch in the stack",
            Style::default().fg(Color::DarkGray),
        )]));
    }

    lines
}
