use crate::tui::app::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

/// Render the linked worktrees panel (bottom of left column)
pub fn render_worktrees(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .worktrees
        .iter()
        .map(|worktree| {
            let (indicator, name_style) = if worktree.is_current {
                ("◆ ", Style::default().fg(Color::Yellow))
            } else if worktree.exists {
                ("◈ ", Style::default().fg(Color::Cyan))
            } else {
                ("◇ ", Style::default().fg(Color::DarkGray))
            };

            let branch_short = worktree
                .branch
                .split('/')
                .last()
                .unwrap_or(&worktree.branch)
                .to_string();

            Line::from(vec![
                Span::styled(indicator, name_style),
                Span::styled(
                    worktree.name.clone(),
                    name_style.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", branch_short),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
            .into()
        })
        .collect();

    let title = if app.worktrees.is_empty() {
        " Worktrees (none) "
    } else {
        " Worktrees "
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(list, area);
}
