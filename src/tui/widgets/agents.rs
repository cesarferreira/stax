use crate::tui::app::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

/// Render the active agent worktrees panel (bottom of left column)
pub fn render_agent_worktrees(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .agent_worktrees
        .iter()
        .map(|agent| {
            let (indicator, name_style) = if agent.exists {
                ("◈ ", Style::default().fg(Color::Cyan))
            } else {
                ("◇ ", Style::default().fg(Color::DarkGray))
            };

            let branch_short = agent
                .branch
                .split('/')
                .last()
                .unwrap_or(&agent.branch)
                .to_string();

            Line::from(vec![
                Span::styled(indicator, name_style),
                Span::styled(agent.name.clone(), name_style.add_modifier(Modifier::BOLD)),
                Span::styled(
                    format!(" {}", branch_short),
                    Style::default().fg(Color::DarkGray),
                ),
            ])
            .into()
        })
        .collect();

    let title = if app.agent_worktrees.is_empty() {
        " Agents (none) "
    } else {
        " Agents "
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(Color::DarkGray)),
    );

    f.render_widget(list, area);
}
