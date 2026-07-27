use crate::commands::ready::{ReadyAction, ReadyReason, ReadyRowState};
use crate::tui::ready::app::ReadyTuiApp;
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

const INDICATOR_WIDTH: usize = 2;
const ACTION_WIDTH: usize = 8;
const PR_WIDTH: usize = 9;
const REVIEWS_WIDTH: usize = 17;
const CI_WIDTH: usize = 9;
const MIN_BRANCH_WIDTH: usize = 18;
const MIN_TITLE_WIDTH: usize = 16;

#[derive(Debug, Clone, Copy)]
struct TableLayout {
    branch_width: usize,
    title_width: usize,
}

impl TableLayout {
    fn for_rows(rows: &[ReadyRowState], inner_width: usize) -> Self {
        let fixed_width = INDICATOR_WIDTH + ACTION_WIDTH + PR_WIDTH + REVIEWS_WIDTH + CI_WIDTH;
        let available = inner_width.saturating_sub(fixed_width);
        let needed_branch_width = rows
            .iter()
            .map(|row| row.branch().chars().count())
            .max()
            .unwrap_or("BRANCH".len())
            .max("BRANCH".len())
            + 2;
        let branch_budget = available
            .saturating_sub(MIN_TITLE_WIDTH)
            .max(MIN_BRANCH_WIDTH.min(available));
        let branch_width = needed_branch_width.min(branch_budget);
        let title_width = available.saturating_sub(branch_width).max(1);

        Self {
            branch_width,
            title_width,
        }
    }

    fn branch_text_width(self) -> usize {
        self.branch_width.saturating_sub(2).max(1)
    }
}

pub fn render(f: &mut Frame, app: &ReadyTuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(f.area());

    let detail_lines = detail_lines(app);
    let details_height =
        details_pane_height(detail_lines.len()).min(chunks[0].height.saturating_sub(3));
    let main = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(details_height)])
        .split(chunks[0]);

    render_table(f, app, main[0]);
    render_details(f, detail_lines, main[1]);
    render_status(f, app, chunks[1]);

    if app.show_help {
        render_help(f);
    }
}

fn details_pane_height(content_height: usize) -> u16 {
    u16::try_from(content_height)
        .unwrap_or(u16::MAX)
        .saturating_add(2)
}

fn render_table(f: &mut Frame, app: &ReadyTuiApp, area: Rect) {
    let layout = TableLayout::for_rows(&app.rows, area.width.saturating_sub(2) as usize);
    let title = format!(
        " PR Readiness  {} · {} · {} PRs{} ",
        app.repo_label,
        app.scope_label,
        app.rows.len(),
        if app.loading_count() > 0 {
            format!(" · loading {}", app.loading_count())
        } else {
            String::new()
        }
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let header = ListItem::new(Line::from(vec![
        Span::styled("  ACTION  ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("PR       ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("{:<width$}", "BRANCH", width = layout.branch_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "REVIEWS          ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled("CI       ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled("TITLE", Style::default().add_modifier(Modifier::BOLD)),
    ]));

    let mut items = vec![header];
    items.extend(app.rows.iter().enumerate().map(|(index, row)| {
        let selected = index == app.selected_index;
        let mut item = ListItem::new(row_line(row, selected, layout));
        if selected {
            item = item.style(Style::default().add_modifier(Modifier::REVERSED));
        }
        item
    }));

    f.render_widget(List::new(items).block(block), area);
}

fn row_line(row: &ReadyRowState, selected: bool, layout: TableLayout) -> Line<'static> {
    let indicator = if selected { "► " } else { "  " };
    match row {
        ReadyRowState::Loading { branch } => Line::from(vec![
            Span::raw(indicator.to_string()),
            Span::styled(
                format!("{:<width$}", "○ wait", width = ACTION_WIDTH),
                action_style(ReadyAction::Wait),
            ),
            Span::styled(
                format!("{:<width$}", pr_text(branch.pr_number), width = PR_WIDTH),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(format!(
                "{:<width$}",
                trim_middle(&branch.name, layout.branch_text_width()),
                width = layout.branch_width
            )),
            Span::styled(
                format!("{:<width$}", "loading...", width = REVIEWS_WIDTH),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<width$}", "loading", width = CI_WIDTH),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                trim_end("loading...", layout.title_width),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        ReadyRowState::Loaded(row) => Line::from(vec![
            Span::raw(indicator.to_string()),
            Span::styled(
                format!("{:<width$}", row.action.display(), width = ACTION_WIDTH),
                action_style(row.action),
            ),
            Span::styled(
                format!(
                    "{:<width$}",
                    format!("#{}", row.pr_number),
                    width = PR_WIDTH
                ),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(format!(
                "{:<width$}",
                trim_middle(&row.branch, layout.branch_text_width()),
                width = layout.branch_width
            )),
            Span::styled(
                format!(
                    "{:<width$}",
                    trim_end(&row.review_summary, REVIEWS_WIDTH.saturating_sub(2)),
                    width = REVIEWS_WIDTH
                ),
                review_text_style(&row.review_summary, row.reason),
            ),
            Span::styled(
                format!(
                    "{:<width$}",
                    trim_end(&row.ci_summary, CI_WIDTH.saturating_sub(2)),
                    width = CI_WIDTH
                ),
                ci_text_style(&row.ci_status, &row.ci_summary),
            ),
            Span::raw(trim_end(&row.title, layout.title_width)),
        ]),
        ReadyRowState::Unavailable { branch, message } => Line::from(vec![
            Span::raw(indicator.to_string()),
            Span::styled(
                format!("{:<width$}", "○ wait", width = ACTION_WIDTH),
                action_style(ReadyAction::Wait),
            ),
            Span::styled(
                format!("{:<width$}", pr_text(branch.pr_number), width = PR_WIDTH),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(format!(
                "{:<width$}",
                trim_middle(&branch.name, layout.branch_text_width()),
                width = layout.branch_width
            )),
            Span::styled(
                format!("{:<width$}", "unknown", width = REVIEWS_WIDTH),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                format!("{:<width$}", "unknown", width = CI_WIDTH),
                Style::default().fg(Color::DarkGray),
            ),
            Span::styled(
                trim_end(message, layout.title_width),
                Style::default().fg(Color::Red),
            ),
        ]),
    }
}

fn render_details(f: &mut Frame, lines: Vec<Line<'static>>, area: Rect) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Magenta));
    let inner = block.inner(area);
    f.render_widget(block, area);

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn detail_lines(app: &ReadyTuiApp) -> Vec<Line<'static>> {
    match app.selected_row() {
        Some(ReadyRowState::Loaded(row)) => vec![
            Line::from(vec![
                Span::styled("Action: ", detail_label_style()),
                Span::styled(row.action.display(), action_style(row.action)),
            ]),
            Line::from(vec![
                Span::styled("Reason: ", detail_label_style()),
                Span::styled(format!("{:?}", row.reason), reason_detail_style(row.reason)),
            ]),
            Line::from(vec![
                Span::styled("Reviews: ", detail_label_style()),
                Span::styled(
                    row.review_summary.clone(),
                    review_text_style(&row.review_summary, row.reason),
                ),
            ]),
            Line::from(vec![
                Span::styled("CI: ", detail_label_style()),
                Span::styled(
                    row.ci_summary.clone(),
                    ci_text_style(&row.ci_status, &row.ci_summary),
                ),
            ]),
            Line::from(vec![
                Span::styled("Mergeable: ", detail_label_style()),
                Span::styled(
                    mergeable_detail_label(row.mergeable).to_string(),
                    mergeable_detail_style(row.mergeable),
                ),
            ]),
            Line::from(vec![
                Span::styled("State: ", detail_label_style()),
                Span::styled(
                    row.mergeable_state.clone(),
                    merge_state_detail_style(&row.mergeable_state),
                ),
            ]),
            Line::from(vec![
                Span::styled("PR: ", detail_label_style()),
                Span::styled(format!("#{}", row.pr_number), metadata_detail_style()),
            ]),
            Line::from(vec![
                Span::styled("Branch: ", detail_label_style()),
                Span::styled(row.branch.clone(), branch_detail_style()),
            ]),
            Line::from(vec![
                Span::styled("URL: ", detail_label_style()),
                Span::styled(row.pr_url.clone().unwrap_or_default(), url_detail_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled(row.title.clone(), title_detail_style())),
        ],
        Some(ReadyRowState::Loading { branch }) => vec![
            Line::from(vec![
                Span::styled("Branch: ", detail_label_style()),
                Span::styled(branch.name.clone(), branch_detail_style()),
            ]),
            Line::from(vec![
                Span::styled("PR: ", detail_label_style()),
                Span::styled(pr_text(branch.pr_number), metadata_detail_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Loading live PR readiness...",
                Style::default().fg(Color::Yellow),
            )),
        ],
        Some(ReadyRowState::Unavailable { branch, message }) => vec![
            Line::from(vec![
                Span::styled("Branch: ", detail_label_style()),
                Span::styled(branch.name.clone(), branch_detail_style()),
            ]),
            Line::from(vec![
                Span::styled("PR: ", detail_label_style()),
                Span::styled(pr_text(branch.pr_number), metadata_detail_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Error: ", detail_label_style()),
                Span::styled(message.clone(), Style::default().fg(Color::Red)),
            ]),
        ],
        None => vec![Line::from("No PRs in scope")],
    }
}

fn render_status(f: &mut Frame, app: &ReadyTuiApp, area: Rect) {
    let line = app
        .status_message
        .as_ref()
        .map(|message| Line::from(Span::styled(message.clone(), muted_shortcut_style())))
        .unwrap_or_else(shortcut_status_line);
    f.render_widget(Paragraph::new(line), area);
}

fn render_help(f: &mut Frame) {
    let area = centered_rect(62, 42, f.area());
    f.render_widget(Clear, area);
    let lines = vec![
        Line::from("PR Readiness Help"),
        Line::from(""),
        Line::from("  ↑/↓, k/j, Ctrl+p/n   Move selection"),
        Line::from("  Enter / o    Open selected PR"),
        Line::from("  r            Refresh live data"),
        Line::from("  q / Esc      Quit"),
        Line::from("  ?            Close help"),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
}

pub(crate) fn action_style(action: ReadyAction) -> Style {
    match action {
        ReadyAction::Merge => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        ReadyAction::Ping => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        ReadyAction::Fix => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ReadyAction::Wait => Style::default().fg(Color::Blue),
        ReadyAction::Draft => Style::default().fg(Color::DarkGray),
    }
}

fn reason_tone(reason: ReadyReason) -> Style {
    match reason {
        ReadyReason::Ready => Style::default().fg(Color::Green),
        ReadyReason::ReviewRequired | ReadyReason::CiPending | ReadyReason::MergeablePending => {
            Style::default().fg(Color::Yellow)
        }
        ReadyReason::Draft | ReadyReason::Unknown => Style::default().fg(Color::DarkGray),
        ReadyReason::CiFailed
        | ReadyReason::ChangesRequested
        | ReadyReason::NotMergeable
        | ReadyReason::Closed => Style::default().fg(Color::Red),
    }
}

fn review_text_style(summary: &str, reason: ReadyReason) -> Style {
    let normalized = summary.to_ascii_lowercase();
    if normalized.contains("approval") {
        return Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
    }
    if normalized.contains("changes requested") || matches!(reason, ReadyReason::ChangesRequested) {
        return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    }

    match reason {
        ReadyReason::Ready => Style::default().fg(Color::Green),
        ReadyReason::ReviewRequired => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
        ReadyReason::Draft => Style::default().fg(Color::DarkGray),
        ReadyReason::Unknown => Style::default().fg(Color::DarkGray),
        _ => reason_tone(reason),
    }
}

fn ci_text_style(status: &str, summary: &str) -> Style {
    let normalized = format!(
        "{} {}",
        status.to_ascii_lowercase(),
        summary.to_ascii_lowercase()
    );
    if normalized.contains("success") || normalized.contains("passed") {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else if normalized.contains("fail")
        || normalized.contains("error")
        || normalized.contains("cancel")
    {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else if normalized.contains("pending")
        || normalized.contains("running")
        || normalized.contains("waiting")
    {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn detail_label_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn reason_detail_style(reason: ReadyReason) -> Style {
    reason_tone(reason).add_modifier(Modifier::BOLD)
}

fn mergeable_detail_style(mergeable: Option<bool>) -> Style {
    match mergeable {
        Some(true) => Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD),
        Some(false) => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        None => Style::default().fg(Color::Yellow),
    }
}

fn mergeable_detail_label(mergeable: Option<bool>) -> &'static str {
    match mergeable {
        Some(true) => "yes",
        Some(false) => "no",
        None => "checking…",
    }
}

fn merge_state_detail_style(state: &str) -> Style {
    let normalized = state.to_ascii_lowercase();
    if normalized == "clean" || normalized == "has_hooks" {
        Style::default().fg(Color::Green)
    } else if normalized.contains("dirty")
        || normalized.contains("blocked")
        || normalized.contains("unknown")
        || normalized.contains("unstable")
    {
        Style::default().fg(Color::Red)
    } else {
        Style::default().fg(Color::Yellow)
    }
}

fn metadata_detail_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::BOLD)
}

fn branch_detail_style() -> Style {
    Style::default().fg(Color::Cyan)
}

fn url_detail_style() -> Style {
    Style::default().fg(Color::Blue)
}

fn title_detail_style() -> Style {
    Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD)
}

fn shortcut_status_line() -> Line<'static> {
    Line::from(vec![
        shortcut_key("↑/↓"),
        shortcut_label(" move  "),
        shortcut_key("Enter"),
        shortcut_label(" open PR  "),
        shortcut_key("r"),
        shortcut_label(" refresh  "),
        shortcut_key("o"),
        shortcut_label(" open  "),
        shortcut_key("?"),
        shortcut_label(" help  "),
        shortcut_key("q"),
        shortcut_label(" quit"),
    ])
}

fn shortcut_key(text: &'static str) -> Span<'static> {
    Span::styled(text, shortcut_key_style())
}

fn shortcut_label(text: &'static str) -> Span<'static> {
    Span::styled(text, muted_shortcut_style())
}

fn shortcut_key_style() -> Style {
    Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD)
}

fn muted_shortcut_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn pr_text(number: Option<u64>) -> String {
    number
        .map(|number| format!("#{number}"))
        .unwrap_or_else(|| "—".to_string())
}

fn trim_end(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else if width <= 3 {
        ".".repeat(width)
    } else {
        format!("{}...", text.chars().take(width - 3).collect::<String>())
    }
}

fn trim_middle(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let keep = width - 3;
    let front = keep / 2 + keep % 2;
    let back = keep / 2;
    let suffix = text
        .chars()
        .rev()
        .take(back)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!(
        "{}...{}",
        text.chars().take(front).collect::<String>(),
        suffix
    )
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
    use crate::commands::ready::{PrReadinessRow, ReadyBranch};
    use crate::tui::ready::app::ReadyTuiUpdate;
    use ratatui::{Terminal, backend::TestBackend};

    fn loaded_row() -> PrReadinessRow {
        PrReadinessRow {
            branch: "feature/a".to_string(),
            pr_number: 10,
            title: "Ready PR".to_string(),
            updated_at: Some("2026-07-21T10:00:00Z".to_string()),
            action: ReadyAction::Merge,
            reason: ReadyReason::Ready,
            review_decision: Some("APPROVED".to_string()),
            approvals: 1,
            changes_requested: false,
            ci_status: "success".to_string(),
            ci_summary: "passed".to_string(),
            is_draft: false,
            mergeable: Some(true),
            mergeable_state: "clean".to_string(),
            review_summary: "1 approval".to_string(),
            pr_url: Some("https://example.com/pull/10".to_string()),
            pr_state: "open".to_string(),
        }
    }

    #[test]
    fn ready_tui_action_style_uses_expected_colors() {
        assert_eq!(action_style(ReadyAction::Merge).fg, Some(Color::Green));
        assert_eq!(action_style(ReadyAction::Ping).fg, Some(Color::Yellow));
        assert_eq!(action_style(ReadyAction::Fix).fg, Some(Color::Red));
        assert_eq!(action_style(ReadyAction::Draft).fg, Some(Color::DarkGray));
    }

    #[test]
    fn ready_tui_status_styles_color_reviews_and_ci() {
        assert_eq!(
            review_text_style("1 approval", ReadyReason::Ready).fg,
            Some(Color::Green)
        );
        assert_eq!(
            review_text_style("missing review", ReadyReason::ReviewRequired).fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            review_text_style("draft", ReadyReason::Draft).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(ci_text_style("success", "passed").fg, Some(Color::Green));
        assert_eq!(ci_text_style("failure", "failed").fg, Some(Color::Red));
        assert_eq!(ci_text_style("pending", "running").fg, Some(Color::Yellow));
    }

    #[test]
    fn ready_tui_detail_styles_use_semantic_colors() {
        assert_eq!(detail_label_style().fg, Some(Color::Cyan));
        assert_eq!(
            reason_detail_style(ReadyReason::Closed).fg,
            Some(Color::Red)
        );
        assert_eq!(mergeable_detail_style(Some(true)).fg, Some(Color::Green));
        assert_eq!(mergeable_detail_style(Some(false)).fg, Some(Color::Red));
        assert_eq!(mergeable_detail_style(None).fg, Some(Color::Yellow));
        assert_eq!(mergeable_detail_label(Some(true)), "yes");
        assert_eq!(mergeable_detail_label(Some(false)), "no");
        assert_eq!(mergeable_detail_label(None), "checking…");
        assert_eq!(metadata_detail_style().fg, Some(Color::Magenta));
        assert_eq!(url_detail_style().fg, Some(Color::Blue));
        assert_eq!(title_detail_style().fg, Some(Color::White));
    }

    #[test]
    fn ready_tui_shortcut_line_colors_keys_and_mutes_labels() {
        let line = shortcut_status_line();

        assert_eq!(line.spans[0].style.fg, Some(Color::Cyan));
        assert_eq!(line.spans[1].style.fg, Some(Color::DarkGray));
        assert!(
            line.spans
                .iter()
                .any(|span| span.content.as_ref() == "Enter" && span.style.fg == Some(Color::Cyan))
        );
    }

    #[test]
    fn ready_tui_branch_column_uses_full_name_when_width_allows() {
        let rows = vec![
            ReadyRowState::Loading {
                branch: ReadyBranch {
                    name: "cesar/OBX-2758-internal-tbt-poc-design".to_string(),
                    pr_number: Some(115_665),
                },
            },
            ReadyRowState::Loading {
                branch: ReadyBranch {
                    name: "codex/robot-android-bazel-docker".to_string(),
                    pr_number: Some(107_328),
                },
            },
        ];

        let layout = TableLayout::for_rows(&rows, 120);
        let line = row_line(&rows[0], false, layout);
        let rendered = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(rendered.contains("cesar/OBX-2758-internal-tbt-poc-design"));
    }

    #[test]
    fn ready_tui_trims_middle_with_suffix() {
        let trimmed = trim_middle("codex/pr-readiness-table", 14);
        assert!(trimmed.starts_with("codex/"));
        assert!(trimmed.ends_with("table"));
    }

    #[test]
    fn ready_tui_help_overlay_renders_key_bindings() {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = ReadyTuiApp::new_for_test(
            "owner/repo",
            "current stack",
            vec![ReadyBranch {
                name: "feature/a".to_string(),
                pr_number: Some(10),
            }],
        );
        app.show_help = true;

        terminal.draw(|f| render(f, &app)).expect("draw");
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("PR Readiness Help"));
        assert!(rendered.contains("Open selected PR"));
        assert!(rendered.contains("Refresh live data"));
    }

    #[test]
    fn ready_tui_renders_loading_details_below_table_at_content_height() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let app = ReadyTuiApp::new_for_test(
            "owner/repo",
            "current stack",
            vec![ReadyBranch {
                name: "feature/a".to_string(),
                pr_number: Some(10),
            }],
        );

        terminal.draw(|f| render(f, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let line = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };

        assert!(line(0).contains("PR Readiness"));
        assert!(!line(0).contains("Details"));
        assert!(line(22).contains("Details"));
        assert!(line(23).contains("Branch: feature/a"));
    }

    #[test]
    fn ready_tui_expands_bottom_details_to_fit_a_loaded_row() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let mut app = ReadyTuiApp::new_for_test(
            "owner/repo",
            "current stack",
            vec![ReadyBranch {
                name: "feature/a".to_string(),
                pr_number: Some(10),
            }],
        );
        app.apply_update(ReadyTuiUpdate::Loaded {
            index: 0,
            row: loaded_row(),
        });

        terminal.draw(|f| render(f, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let line = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };

        assert!(line(15).contains("Details"));
        assert!(line(16).contains("Action: ✓ merge"));
        assert!(line(26).contains("Ready PR"));
    }

    #[test]
    fn ready_tui_keeps_empty_details_pane_to_one_content_row() {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let app = ReadyTuiApp::new_for_test("owner/repo", "current stack", Vec::new());

        terminal.draw(|f| render(f, &app)).expect("draw");
        let buffer = terminal.backend().buffer();
        let line = |y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };

        assert!(line(25).contains("Details"));
        assert!(line(26).contains("No PRs in scope"));
    }
}
