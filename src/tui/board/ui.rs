use crate::ci::CheckRunInfo;
use crate::commands::github_list::format_relative_time;
use crate::forge::PrComment;
use crate::github::board::{BoardIssueSummary, BoardPrSummary};
use crate::tui::board::app::{BoardApp, BoardMode, BoardTab, BoardTarget};
use console::{measure_text_width, truncate_str};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

// Matches the branch checkout picker's selected-row background, so a
// selected row reads as a subtle opaque highlight (src/tui/ready/ui.rs:23).
const SELECTED_ROW_BACKGROUND: Color = Color::Indexed(236);
const NARROW_WIDTH: u16 = 100;

pub fn render(f: &mut Frame, app: &BoardApp) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_tab_header(f, app, chunks[0]);
    render_body(f, app, chunks[1]);
    render_footer(f, app, chunks[2]);

    match app.mode {
        BoardMode::Diff => render_diff_overlay(f, app),
        BoardMode::Comments => render_comments_overlay(f, app),
        BoardMode::Help => render_help_overlay(f),
        BoardMode::LabelPicker => render_label_picker_overlay(f, app),
        BoardMode::ConfirmMerge => render_confirm_merge_overlay(f, app),
        BoardMode::List | BoardMode::Detail | BoardMode::Filter => {}
    }

    if app.mode == BoardMode::Filter {
        render_filter_overlay(f, app, chunks[2]);
    }
}

fn render_tab_header(f: &mut Frame, app: &BoardApp, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            format!(" {} ", app.repo_label),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            "PULL REQUESTS",
            tab_style(app.tab == BoardTab::PullRequests),
        ),
        Span::raw(" │ "),
        Span::styled("ISSUES", tab_style(app.tab == BoardTab::Issues)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn render_body(f: &mut Frame, app: &BoardApp, area: Rect) {
    if area.width < NARROW_WIDTH || !app.show_detail {
        render_list(f, app, area);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);
    render_list(f, app, chunks[0]);
    render_detail(f, app, chunks[1]);
}

/// `3` when nothing is filtered out, `3/12` when a filter (search or "mine
/// only") is hiding some of the fetched items — so the header count always
/// matches what's actually rendered below it.
fn count_label(visible: usize, total: usize) -> String {
    if visible == total {
        visible.to_string()
    } else {
        format!("{visible}/{total}")
    }
}

fn render_list(f: &mut Frame, app: &BoardApp, area: Rect) {
    let inner_width = area.width.saturating_sub(2) as usize;

    let (title, items): (String, Vec<ListItem<'static>>) = match app.tab {
        BoardTab::PullRequests => {
            let indices = app.visible_pr_indices();
            let title = format!(
                " Pull Requests ({}){}{} ",
                count_label(indices.len(), app.prs.len()),
                if app.loading_list { " · loading" } else { "" },
                if app.mine_only { " · mine" } else { "" }
            );
            let items = indices
                .iter()
                .enumerate()
                .map(|(row, &index)| pr_row(&app.prs[index], row == app.pr_selected, inner_width))
                .collect();
            (title, items)
        }
        BoardTab::Issues => {
            let indices = app.visible_issue_indices();
            let title = format!(
                " Issues ({}){}{} ",
                count_label(indices.len(), app.issues.len()),
                if app.loading_list { " · loading" } else { "" },
                if app.mine_only { " · mine" } else { "" }
            );
            let items = indices
                .iter()
                .enumerate()
                .map(|(row, &index)| {
                    issue_row(&app.issues[index], row == app.issue_selected, inner_width)
                })
                .collect();
            (title, items)
        }
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    if items.is_empty() {
        let message = if app.loading_list {
            "Loading…"
        } else {
            "Nothing here."
        };
        f.render_widget(
            Paragraph::new(Span::styled(message, Style::default().fg(Color::DarkGray)))
                .block(block),
            area,
        );
        return;
    }

    f.render_widget(List::new(items).block(block), area);
}

fn pr_row(pr: &BoardPrSummary, selected: bool, width: usize) -> ListItem<'static> {
    let number = format!("#{}", pr.number);
    let age = format_relative_time(pr.updated_at);
    let draft = if pr.is_draft { " [draft]" } else { "" };
    let fixed = measure_text_width(&number)
        + measure_text_width(&age)
        + measure_text_width(draft)
        + measure_text_width(&pr.author)
        + 8;
    let title_width = width.saturating_sub(fixed).max(8);
    let title = truncate_str(&pr.title, title_width, "…").into_owned();

    let line = Line::from(vec![
        Span::styled(number, Style::default().fg(Color::Magenta)),
        Span::raw("  "),
        Span::raw(title),
        Span::styled(draft, Style::default().fg(Color::Yellow)),
        Span::raw("  "),
        Span::styled(pr.author.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(age, Style::default().fg(Color::DarkGray)),
    ]);

    row_item(line, selected)
}

fn issue_row(issue: &BoardIssueSummary, selected: bool, width: usize) -> ListItem<'static> {
    let number = format!("#{}", issue.number);
    let age = format_relative_time(issue.updated_at);
    let fixed = measure_text_width(&number)
        + measure_text_width(&age)
        + measure_text_width(&issue.author)
        + 6;
    let title_width = width.saturating_sub(fixed).max(8);
    let title = truncate_str(&issue.title, title_width, "…").into_owned();

    let line = Line::from(vec![
        Span::styled(number, Style::default().fg(Color::Magenta)),
        Span::raw("  "),
        Span::raw(title),
        Span::raw("  "),
        Span::styled(issue.author.clone(), Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(age, Style::default().fg(Color::DarkGray)),
    ]);

    row_item(line, selected)
}

fn row_item(line: Line<'static>, selected: bool) -> ListItem<'static> {
    let item = ListItem::new(line);
    if selected {
        item.style(Style::default().bg(SELECTED_ROW_BACKGROUND))
    } else {
        item
    }
}

fn render_detail(f: &mut Frame, app: &BoardApp, area: Rect) {
    let block = Block::default()
        .title(" Detail ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let lines = match app.tab {
        BoardTab::PullRequests => pr_detail_lines(app),
        BoardTab::Issues => issue_detail_lines(app),
    };

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.detail_scroll, 0)),
        inner,
    );
}

/// While a detail request is in flight this reports the loading placeholder;
/// once it fails, the pane shows the actual error instead of hanging on
/// "Loading…" forever (the underlying request already gave up).
fn detail_error_or_loading_line(
    app: &BoardApp,
    target: BoardTarget,
    loading_message: String,
) -> Line<'static> {
    if app.loading_detail == Some(target) {
        return Line::from(loading_message);
    }
    if let Some(message) = app.detail_error.get(&target) {
        return Line::from(Span::styled(
            format!("{message} (press Enter or r to retry)"),
            Style::default().fg(Color::Red),
        ));
    }
    Line::from(loading_message)
}

fn pr_detail_lines(app: &BoardApp) -> Vec<Line<'static>> {
    let Some(pr) = app.selected_pr() else {
        return vec![Line::from("No pull requests in scope.")];
    };
    let Some(detail) = app.pr_details.get(&pr.number) else {
        return vec![detail_error_or_loading_line(
            app,
            BoardTarget::Pr(pr.number),
            format!("Loading PR #{}…", pr.number),
        )];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("#{} {}", detail.number, detail.title),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled(detail.head_branch.clone(), Style::default().fg(Color::Cyan)),
            Span::raw(" → "),
            Span::styled(detail.base_branch.clone(), Style::default().fg(Color::Cyan)),
        ]),
        Line::from(vec![
            Span::raw(format!(
                "{} files  ",
                app.pr_files
                    .get(&pr.number)
                    .map(Vec::len)
                    .unwrap_or(detail.changed_files as usize)
            )),
            Span::styled(
                format!("+{}", detail.additions),
                Style::default().fg(Color::Green),
            ),
            Span::raw(" "),
            Span::styled(
                format!("-{}", detail.deletions),
                Style::default().fg(Color::Red),
            ),
        ]),
        Line::from(""),
    ];

    match app.pr_checks.get(&pr.number) {
        Some(checks) if !checks.is_empty() => {
            for check in checks {
                lines.push(check_line(check));
            }
            lines.push(Line::from(""));
        }
        Some(_) => {
            lines.push(Line::from(Span::styled(
                "No CI checks",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(""));
        }
        None => {}
    }

    if !detail.labels.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Labels: ", Style::default().fg(Color::DarkGray)),
            Span::raw(detail.labels.join(", ")),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Comments: ", Style::default().fg(Color::DarkGray)),
        Span::raw(detail.comment_count.to_string()),
    ]));
    lines.push(Line::from(""));

    if detail.body.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "No description provided.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.extend(detail.body.lines().map(|line| Line::from(line.to_string())));
    }

    lines
}

fn issue_detail_lines(app: &BoardApp) -> Vec<Line<'static>> {
    let Some(issue) = app.selected_issue() else {
        return vec![Line::from("No issues in scope.")];
    };
    let Some(detail) = app.issue_details.get(&issue.number) else {
        return vec![detail_error_or_loading_line(
            app,
            BoardTarget::Issue(issue.number),
            format!("Loading issue #{}…", issue.number),
        )];
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("#{} {}", detail.number, detail.title),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    if !detail.labels.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("Labels: ", Style::default().fg(Color::DarkGray)),
            Span::raw(detail.labels.join(", ")),
        ]));
    }
    lines.push(Line::from(vec![
        Span::styled("Comments: ", Style::default().fg(Color::DarkGray)),
        Span::raw(detail.comment_count.to_string()),
    ]));
    lines.push(Line::from(""));

    if detail.body.trim().is_empty() {
        lines.push(Line::from(Span::styled(
            "No description provided.",
            Style::default().fg(Color::DarkGray),
        )));
    } else {
        lines.extend(detail.body.lines().map(|line| Line::from(line.to_string())));
    }

    lines
}

/// Green check for success-like conclusions, red for failure-like ones,
/// yellow for anything still in flight.
fn check_line(check: &CheckRunInfo) -> Line<'static> {
    let (symbol, color) = if check.status != "completed" {
        ("•", Color::Yellow)
    } else {
        match check.conclusion.as_deref() {
            Some("success") | Some("neutral") | Some("skipped") => ("✓", Color::Green),
            Some("failure") | Some("timed_out") | Some("cancelled") | Some("action_required") => {
                ("✗", Color::Red)
            }
            _ => ("•", Color::Yellow),
        }
    };
    Line::from(vec![
        Span::styled(format!("{symbol} "), Style::default().fg(color)),
        Span::raw(check.name.clone()),
    ])
}

fn render_footer(f: &mut Frame, app: &BoardApp, area: Rect) {
    if let Some(message) = &app.status_message {
        f.render_widget(
            Paragraph::new(Span::styled(
                message.clone(),
                Style::default().fg(Color::DarkGray),
            )),
            area,
        );
        return;
    }

    if app.loading_list {
        f.render_widget(
            Paragraph::new(Span::styled("Loading…", Style::default().fg(Color::Yellow))),
            area,
        );
        return;
    }

    let mut spans = Vec::new();
    for (index, (key, label)) in app.footer_hints().into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_filter_overlay(f: &mut Frame, app: &BoardApp, area: Rect) {
    let line = Line::from(vec![
        Span::styled(
            "/",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(app.filter.clone()),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_diff_overlay(f: &mut Frame, app: &BoardApp) {
    let area = f.area();
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Diff (Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let selected_number = app.selected_pr().map(|pr| pr.number);
    let lines = match selected_number.and_then(|number| app.diff_lines.get(&number)) {
        Some(lines) => lines.clone(),
        None => vec![Line::from(if app.loading_diff.is_some() {
            "Loading diff…"
        } else {
            "No diff loaded."
        })],
    };

    f.render_widget(Paragraph::new(lines).scroll((app.diff_scroll, 0)), inner);
}

fn render_comments_overlay(f: &mut Frame, app: &BoardApp) {
    let area = f.area();
    f.render_widget(Clear, area);
    let block = Block::default()
        .title(" Comments (Esc to close) ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let selected_target = app.selected_target();
    let lines = match selected_target.and_then(|target| app.comments.get(&target)) {
        Some(comments) if !comments.is_empty() => comments_lines(comments),
        Some(_) => vec![Line::from(Span::styled(
            "No comments yet.",
            Style::default().fg(Color::DarkGray),
        ))],
        None => vec![Line::from(if app.loading_comments.is_some() {
            "Loading comments…"
        } else {
            "No comments loaded."
        })],
    };

    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((app.comments_scroll, 0)),
        inner,
    );
}

fn comments_lines(comments: &[PrComment]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for comment in comments {
        lines.push(Line::from(vec![
            Span::styled(
                comment.user().to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  {}",
                comment.created_at().format("%Y-%m-%d %H:%M")
            )),
        ]));
        for line in comment.body().lines() {
            lines.push(Line::from(line.to_string()));
        }
        lines.push(Line::from(""));
    }
    lines
}

fn render_help_overlay(f: &mut Frame) {
    let area = centered_rect(64, 60, f.area());
    f.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            "Board Dashboard Help",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Tab / 1 / 2   Switch PRs / Issues tab"),
        Line::from("  j/k, ↑/↓      Move selection"),
        Line::from("  g / G         Jump to top / bottom"),
        Line::from("  Ctrl-d/Ctrl-u Page down / up"),
        Line::from("  Enter         Open detail"),
        Line::from("  d             Diff (PRs)"),
        Line::from("  c             Comments"),
        Line::from("  l             Label picker (Space toggles)"),
        Line::from("  t             Toggle draft (PRs)"),
        Line::from("  m             Merge (PRs, opens confirm)"),
        Line::from("  o             Open in browser"),
        Line::from("  r             Refresh"),
        Line::from("  /             Filter"),
        Line::from("  q / Esc       Back one mode, quit from list"),
        Line::from(""),
        Line::from(Span::styled(
            "Merges here are API-only (no local rebase/cleanup) — run `stax sync` after.",
            Style::default().fg(Color::DarkGray),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_label_picker_overlay(f: &mut Frame, app: &BoardApp) {
    let area = centered_rect(50, 60, f.area());
    f.render_widget(Clear, area);

    let items: Vec<ListItem<'static>> = if app.repo_labels.is_empty() {
        vec![ListItem::new(Span::styled(
            "Loading labels…",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        app.repo_labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let active = app.active_labels.contains(label);
                let marker = if active { "[x] " } else { "[ ] " };
                let line = Line::from(format!("{marker}{label}"));
                let item = ListItem::new(line);
                if index == app.label_cursor {
                    item.style(Style::default().bg(SELECTED_ROW_BACKGROUND))
                } else {
                    item
                }
            })
            .collect()
    };

    f.render_widget(
        List::new(items).block(
            Block::default()
                .title(" Labels (Space toggles, Esc closes) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        ),
        area,
    );
}

fn render_confirm_merge_overlay(f: &mut Frame, app: &BoardApp) {
    let area = centered_rect(50, 20, f.area());
    f.render_widget(Clear, area);

    let number = app.selected_pr().map(|pr| pr.number).unwrap_or_default();
    let lines = vec![
        Line::from(format!("Merge PR #{number} (squash)? [y/N]")),
        Line::from(""),
        Line::from(Span::styled(
            "This calls the GitHub API merge only — no local rebase, PR-base",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "retargeting, or branch cleanup. Run `stax sync` afterwards.",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    f.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Confirm merge ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
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
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::board::BoardTabSelection;
    use chrono::Utc;
    use ratatui::{Terminal, backend::TestBackend};

    fn pr(number: u64) -> BoardPrSummary {
        BoardPrSummary {
            number,
            title: "Add board dashboard".to_string(),
            author: "octocat".to_string(),
            head_branch: "feature/board".to_string(),
            base_branch: "main".to_string(),
            is_draft: false,
            labels: vec!["cli".to_string()],
            created_at: Utc::now(),
            updated_at: Utc::now(),
            url: "https://github.com/o/r/pull/1".to_string(),
        }
    }

    #[test]
    fn renders_pr_list_at_normal_width() {
        let backend = TestBackend::new(140, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = BoardApp::new(
            "owner/repo".to_string(),
            BoardTabSelection::PullRequests,
            false,
        );
        app.apply_update(super::super::app::BoardUpdate::Prs(vec![pr(1)]));

        terminal.draw(|f| render(f, &app)).expect("draw");
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("PULL REQUESTS"));
        assert!(rendered.contains("Add board dashboard"));
    }

    #[test]
    fn narrow_width_hides_detail_pane() {
        let backend = TestBackend::new(80, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = BoardApp::new(
            "owner/repo".to_string(),
            BoardTabSelection::PullRequests,
            false,
        );
        app.apply_update(super::super::app::BoardUpdate::Prs(vec![pr(1)]));

        terminal.draw(|f| render(f, &app)).expect("draw");
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(!rendered.contains("Detail"));
    }

    #[test]
    fn toggle_hides_detail_pane_at_normal_width() {
        let backend = TestBackend::new(140, 30);
        let mut terminal = Terminal::new(backend).expect("terminal");
        let mut app = BoardApp::new(
            "owner/repo".to_string(),
            BoardTabSelection::PullRequests,
            false,
        );
        app.apply_update(super::super::app::BoardUpdate::Prs(vec![pr(1)]));
        app.toggle_show_detail();

        terminal.draw(|f| render(f, &app)).expect("draw");
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(!app.show_detail);
        assert!(!rendered.contains("Detail"));
    }
}
