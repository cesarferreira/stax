use crate::commands::stack_palette;
use crate::tui::app::{App, BranchDisplay, FocusedPane, Mode};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};
use std::borrow::Borrow;

// Matches the branch checkout picker active row background (`48;5;236`).
const SELECTED_ROW_BACKGROUND: Color = Color::Indexed(236);

struct RenderedStackTreeLine {
    spans: Vec<Span<'static>>,
}

impl RenderedStackTreeLine {
    fn into_line(self) -> Line<'static> {
        Line::from(self.spans)
    }
}

fn stack_tree_lane_color(column: usize) -> Color {
    let (r, g, b) = stack_palette::lane_rgb(column);
    Color::Rgb(r, g, b)
}

fn stack_tree_item_style(is_selected: bool) -> Style {
    if is_selected {
        Style::default().bg(SELECTED_ROW_BACKGROUND)
    } else {
        Style::default()
    }
}

fn push_styled_span(spans: &mut Vec<Span<'static>>, text: impl Into<String>, style: Style) {
    let text = text.into();
    spans.push(Span::styled(text, style));
}

fn push_plain_span(spans: &mut Vec<Span<'static>>, text: impl Into<String>) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    spans.push(Span::raw(text));
}

fn push_lane_span(spans: &mut Vec<Span<'static>>, column: usize, text: &str) {
    push_styled_span(
        spans,
        text,
        Style::default().fg(stack_tree_lane_color(column)),
    );
}

fn tree_target_width(max_column: usize) -> usize {
    (max_column + 1) * 2
}

fn append_padding(spans: &mut Vec<Span<'static>>, visual_width: usize, target_width: usize) {
    let padding = target_width.saturating_sub(visual_width) + 1;
    push_plain_span(spans, " ".repeat(padding));
}

fn direct_trunk_child_max_column<B: Borrow<BranchDisplay>>(
    branches: &[B],
    trunk_name: &str,
) -> usize {
    branches
        .iter()
        .map(Borrow::borrow)
        .filter(|branch| branch.parent.as_deref() == Some(trunk_name))
        .map(|branch| branch.column)
        .max()
        .unwrap_or(0)
}

fn append_branch_tree<B: Borrow<BranchDisplay>>(
    branches: &[B],
    row_index: usize,
    max_column: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let branch = branches[row_index].borrow();
    let prev_column = (row_index > 0).then(|| branches[row_index - 1].borrow().column);
    let needs_corner = prev_column.is_some_and(|column| column > branch.column);
    let mut visual_width = 0;

    for column in 0..=branch.column {
        if column == branch.column {
            push_lane_span(spans, column, if branch.is_current { "◉" } else { "○" });
            visual_width += 1;

            if needs_corner {
                push_lane_span(spans, column, "─┘");
                visual_width += 2;
            }
        } else {
            push_lane_span(spans, column, "│");
            push_plain_span(spans, " ");
            visual_width += 2;
        }
    }

    append_padding(spans, visual_width, tree_target_width(max_column));
}

fn append_trunk_tree<B: Borrow<BranchDisplay>>(
    branches: &[B],
    trunk: &BranchDisplay,
    max_column: usize,
    spans: &mut Vec<Span<'static>>,
) {
    let trunk_child_max_column = direct_trunk_child_max_column(branches, &trunk.name);
    let mut visual_width = 0;

    push_lane_span(spans, 0, if trunk.is_current { "◉" } else { "○" });
    visual_width += 1;

    if trunk_child_max_column >= 1 {
        for column in 1..=trunk_child_max_column {
            if column < trunk_child_max_column {
                push_lane_span(spans, column, "─┴");
            } else {
                push_lane_span(spans, column, "─┘");
            }
            visual_width += 2;
        }
    }

    append_padding(spans, visual_width, tree_target_width(max_column));
}

fn branch_name_style(branch: &BranchDisplay) -> Style {
    if branch.is_current {
        Style::default()
            .fg(stack_tree_lane_color(branch.column))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(stack_tree_lane_color(branch.column))
    }
}

fn stack_tree_line<B: Borrow<BranchDisplay>>(
    branches: &[B],
    row_index: usize,
    selected_index: usize,
    max_column: usize,
) -> RenderedStackTreeLine {
    let branch = branches[row_index].borrow();
    let is_selected = row_index == selected_index;
    let mut spans = Vec::new();

    if is_selected {
        push_styled_span(
            &mut spans,
            "› ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        );
    } else {
        push_plain_span(&mut spans, "  ");
    }

    if branch.is_trunk {
        append_trunk_tree(branches, branch, max_column, &mut spans);
    } else {
        append_branch_tree(branches, row_index, max_column, &mut spans);
    }

    push_styled_span(&mut spans, branch.name.clone(), branch_name_style(branch));

    RenderedStackTreeLine { spans }
}

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

                let mut rendered_line =
                    stack_tree_line(&branches, i, app.selected_index, max_column);
                rendered_line.spans.extend(status_spans);

                ListItem::new(rendered_line.into_line()).style(stack_tree_item_style(is_selected))
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
        .highlight_style(stack_tree_item_style(true));

    let mut state = ListState::default();
    state.select((!branches.is_empty()).then_some(app.selected_index));

    f.render_stateful_widget(list, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::BranchDisplay;
    use ratatui::{buffer::Buffer, widgets::StatefulWidget};

    fn branch(name: &str, parent: Option<&str>, column: usize, current: bool) -> BranchDisplay {
        BranchDisplay {
            name: name.to_string(),
            parent: parent.map(str::to_string),
            column,
            is_current: current,
            is_trunk: false,
            ahead: 0,
            behind: 0,
            needs_restack: false,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number: None,
            pr_state: None,
            ci_state: None,
            commits: Vec::new(),
        }
    }

    fn trunk(name: &str) -> BranchDisplay {
        BranchDisplay {
            name: name.to_string(),
            parent: None,
            column: 0,
            is_current: false,
            is_trunk: true,
            ahead: 0,
            behind: 0,
            needs_restack: false,
            has_remote: false,
            unpushed: 0,
            unpulled: 0,
            pr_number: None,
            pr_state: None,
            ci_state: None,
            commits: Vec::new(),
        }
    }

    fn plain(line: &RenderedStackTreeLine) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    #[test]
    fn stack_tree_graph_matches_ls_corner_and_trunk_shape() {
        let branches = vec![
            branch("feature/a", Some("main"), 0, false),
            branch("feature/b-child", Some("feature/b"), 1, false),
            branch("feature/b", Some("main"), 1, false),
            trunk("main"),
        ];

        let max_column = branches.iter().map(|branch| branch.column).max().unwrap();
        let first = stack_tree_line(&branches, 0, 1, max_column);
        let child = stack_tree_line(&branches, 1, 1, max_column);
        let parent = stack_tree_line(&branches, 2, 1, max_column);
        let trunk = stack_tree_line(&branches, 3, 1, max_column);

        assert_eq!(plain(&first), "  ○    feature/a");
        assert_eq!(plain(&child), "› │ ○  feature/b-child");
        assert_eq!(plain(&parent), "  │ ○  feature/b");
        assert_eq!(plain(&trunk), "  ○─┘  main");
    }

    #[test]
    fn stack_tree_uses_ls_lane_colors_for_graph_and_branch_names() {
        let branches = vec![branch("feature/b", Some("main"), 1, false)];
        let line = stack_tree_line(&branches, 0, 0, 1);

        assert_eq!(line.spans[1].style.fg, Some(Color::Rgb(56, 189, 248)));
        assert_eq!(line.spans[3].style.fg, Some(Color::Rgb(74, 222, 128)));
        assert_eq!(line.spans[5].style.fg, Some(Color::Rgb(74, 222, 128)));
    }

    #[test]
    fn stack_tree_selected_row_uses_checkout_picker_background() {
        assert_eq!(stack_tree_item_style(true).bg, Some(Color::Indexed(236)));
        assert_eq!(stack_tree_item_style(false).bg, None);
    }

    #[test]
    fn stack_tree_list_renders_checkout_picker_background_on_selected_row() {
        let branches = vec![branch("feature/b", Some("main"), 0, true)];
        let item = ListItem::new(stack_tree_line(&branches, 0, 0, 0).into_line())
            .style(stack_tree_item_style(true));
        let list = List::new(vec![item]).highlight_style(stack_tree_item_style(true));
        let mut state = ListState::default();
        state.select(Some(0));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 20, 1));

        StatefulWidget::render(list, buffer.area, &mut buffer, &mut state);

        for x in 0..20 {
            assert_eq!(buffer[(x, 0)].bg, SELECTED_ROW_BACKGROUND);
        }
    }
}
