use crate::commands::ready::{ReadyReason, ReadyRowState};
use crate::tui::ready::app::ReadyTuiApp;
use console::{measure_text_width, truncate_str};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

const INDICATOR_WIDTH: usize = 2;
const COL_GAP: &str = "  ";
const MIN_BRANCH_WIDTH: usize = 18;
const MIN_TITLE_WIDTH: usize = 16;
const MIN_PR_WIDTH: usize = 4;
const MIN_REVIEW_WIDTH: usize = 6;
const MIN_CI_WIDTH: usize = 4;

#[derive(Debug, Clone, Copy)]
struct TableLayout {
    pr_width: usize,
    branch_width: usize,
    review_width: usize,
    ci_width: usize,
    title_width: usize,
}

impl TableLayout {
    fn for_rows(rows: &[ReadyRowState], inner_width: usize) -> Self {
        let pr_width = rows
            .iter()
            .map(|row| measure_text_width(&pr_text(row_pr_number(row))))
            .max()
            .unwrap_or(MIN_PR_WIDTH)
            .max(measure_text_width("PR"))
            .max(MIN_PR_WIDTH);
        let review_width = rows
            .iter()
            .map(|row| measure_text_width(review_cell(row)))
            .max()
            .unwrap_or(MIN_REVIEW_WIDTH)
            .max(measure_text_width("REVIEW"))
            .max(MIN_REVIEW_WIDTH);
        let ci_width = rows
            .iter()
            .map(|row| measure_text_width(ci_cell(row)))
            .max()
            .unwrap_or(MIN_CI_WIDTH)
            .max(measure_text_width("CI"))
            .max(MIN_CI_WIDTH);
        let branch_pref = rows
            .iter()
            .map(|row| measure_text_width(row.branch()))
            .max()
            .unwrap_or(MIN_BRANCH_WIDTH)
            .max(measure_text_width("BRANCH"))
            .clamp(MIN_BRANCH_WIDTH, 52);

        let gap_count = COL_GAP.len() * 5;
        let fixed = INDICATOR_WIDTH + pr_width + review_width + ci_width + gap_count;
        let available = inner_width.saturating_sub(fixed);
        let title_min = MIN_TITLE_WIDTH.min(available.max(1));
        let branch_width = branch_pref.min(available.saturating_sub(title_min).max(1));
        let title_width = available.saturating_sub(branch_width).max(1);

        Self {
            pr_width,
            branch_width,
            review_width,
            ci_width,
            title_width,
        }
    }
}

pub fn render(f: &mut Frame, app: &ReadyTuiApp) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(2)])
        .split(f.area());

    render_table(f, app, chunks[0]);
    render_status(f, app, chunks[1]);

    if app.show_help {
        render_help(f);
    }
}

fn render_table(f: &mut Frame, app: &ReadyTuiApp, area: Rect) {
    let layout = TableLayout::for_rows(&app.rows, area.width.saturating_sub(2) as usize);
    let status_suffix = if app.loading {
        if app.loading_count() == app.rows.len() {
            format!(" · loading {}", app.loading_count())
        } else {
            " · refreshing".to_string()
        }
    } else {
        String::new()
    };
    let title = format!(
        " PR Readiness  {} · {} · {} PRs{} ",
        app.repo_label,
        app.scope_label,
        app.rows.len(),
        status_suffix,
    );
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    let header = ListItem::new(Line::from(vec![
        Span::raw(pad_plain("", INDICATOR_WIDTH)),
        Span::raw(COL_GAP),
        Span::styled(
            pad_plain("PR", layout.pr_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(COL_GAP),
        Span::styled(
            pad_plain("BRANCH", layout.branch_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(COL_GAP),
        Span::styled(
            pad_plain("REVIEW", layout.review_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(COL_GAP),
        Span::styled(
            pad_plain("CI", layout.ci_width),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(COL_GAP),
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
    let indicator = if selected { "►" } else { " " };
    match row {
        ReadyRowState::Loading { branch } => Line::from(vec![
            Span::raw(pad_plain(indicator, INDICATOR_WIDTH)),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain(&pr_text(branch.pr_number), layout.pr_width),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(COL_GAP),
            Span::raw(pad_plain(
                &trim_middle(branch.name.as_str(), layout.branch_width),
                layout.branch_width,
            )),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain("…", layout.review_width),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain("…", layout.ci_width),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(COL_GAP),
            Span::styled(
                trim_end("loading…", layout.title_width),
                Style::default().fg(Color::DarkGray),
            ),
        ]),
        ReadyRowState::Loaded(row) => Line::from(vec![
            Span::raw(pad_plain(indicator, INDICATOR_WIDTH)),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain(&format!("#{}", row.pr_number), layout.pr_width),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(COL_GAP),
            Span::raw(pad_plain(
                &trim_middle(&row.branch, layout.branch_width),
                layout.branch_width,
            )),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain(
                    &trim_end(&row.review_summary, layout.review_width),
                    layout.review_width,
                ),
                review_text_style(&row.review_summary, row.reason),
            ),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain(&trim_end(&row.ci_summary, layout.ci_width), layout.ci_width),
                ci_text_style(&row.ci_status, &row.ci_summary),
            ),
            Span::raw(COL_GAP),
            Span::raw(trim_end(&row.title, layout.title_width)),
        ]),
        ReadyRowState::Unavailable { branch, message } => Line::from(vec![
            Span::raw(pad_plain(indicator, INDICATOR_WIDTH)),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain(&pr_text(branch.pr_number), layout.pr_width),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(COL_GAP),
            Span::raw(pad_plain(
                &trim_middle(&branch.name, layout.branch_width),
                layout.branch_width,
            )),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain("—", layout.review_width),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(COL_GAP),
            Span::styled(
                pad_plain("—", layout.ci_width),
                Style::default().fg(Color::DarkGray),
            ),
            Span::raw(COL_GAP),
            Span::styled(
                trim_end(message, layout.title_width),
                Style::default().fg(Color::Red),
            ),
        ]),
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
        Line::from("  ↑/↓ or k/j   Move selection"),
        Line::from("  Enter / o    Open selected PR"),
        Line::from("  r            Refresh live data now"),
        Line::from("  q / Esc      Quit (stays open after CI passes)"),
        Line::from("  ?            Close help"),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title(" Help ").borders(Borders::ALL))
            .wrap(Wrap { trim: true }),
        area,
    );
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
    if normalized == "approved" || normalized.contains("approval") {
        return Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
    }
    if normalized == "changes" || normalized.contains("changes requested") {
        return Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
    }
    if normalized == "review" {
        return Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
    }
    if normalized == "draft" || matches!(reason, ReadyReason::Draft) {
        return Style::default().fg(Color::DarkGray);
    }

    match reason {
        ReadyReason::Ready => Style::default().fg(Color::Green),
        ReadyReason::ReviewRequired => Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD),
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
    if normalized.contains('/')
        && !normalized.contains("fail")
        && !normalized.contains("running")
        && !normalized.contains("pending")
    {
        return Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD);
    }
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

fn row_pr_number(row: &ReadyRowState) -> Option<u64> {
    match row {
        ReadyRowState::Loading { branch } => branch.pr_number,
        ReadyRowState::Loaded(row) => Some(row.pr_number),
        ReadyRowState::Unavailable { branch, .. } => branch.pr_number,
    }
}

fn review_cell(row: &ReadyRowState) -> &str {
    match row {
        ReadyRowState::Loading { .. } => "…",
        ReadyRowState::Loaded(row) => row.review_summary.as_str(),
        ReadyRowState::Unavailable { .. } => "—",
    }
}

fn ci_cell(row: &ReadyRowState) -> &str {
    match row {
        ReadyRowState::Loading { .. } => "…",
        ReadyRowState::Loaded(row) => row.ci_summary.as_str(),
        ReadyRowState::Unavailable { .. } => "—",
    }
}

fn pad_plain(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(measure_text_width(text));
    format!("{text}{}", " ".repeat(padding))
}

fn trim_end(text: &str, width: usize) -> String {
    if measure_text_width(text) <= width {
        return text.to_string();
    }
    truncate_str(text, width, "...").into_owned()
}

fn trim_middle(text: &str, width: usize) -> String {
    if measure_text_width(text) <= width {
        return text.to_string();
    }
    if width <= 3 {
        return ".".repeat(width);
    }
    let chars: Vec<char> = text.chars().collect();
    let keep = width.saturating_sub(3);
    let front = keep / 2 + keep % 2;
    let back = keep / 2;
    let suffix = chars
        .iter()
        .rev()
        .take(back)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    let candidate = format!(
        "{}...{}",
        chars.iter().take(front).collect::<String>(),
        suffix
    );
    if measure_text_width(&candidate) <= width {
        candidate
    } else {
        truncate_str(text, width, "...").into_owned()
    }
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
    use crate::commands::ready::{PrReadinessRow, ReadyAction, ReadyBranch};
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
            review_summary: "approved".to_string(),
            pr_url: Some("https://example.com/pull/10".to_string()),
            pr_state: "open".to_string(),
        }
    }

    #[test]
    fn ready_tui_status_styles_color_reviews_and_ci() {
        assert_eq!(
            review_text_style("approved", ReadyReason::Ready).fg,
            Some(Color::Green)
        );
        assert_eq!(
            review_text_style("review", ReadyReason::ReviewRequired).fg,
            Some(Color::Yellow)
        );
        assert_eq!(
            review_text_style("draft", ReadyReason::Draft).fg,
            Some(Color::DarkGray)
        );
        assert_eq!(ci_text_style("success", "12/12").fg, Some(Color::Green));
        assert_eq!(ci_text_style("failure", "failed").fg, Some(Color::Red));
        assert_eq!(ci_text_style("pending", "running").fg, Some(Color::Yellow));
    }

    #[test]
    fn ready_tui_renders_table_without_details_pane() {
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
        let rendered = format!("{:?}", terminal.backend().buffer());

        assert!(rendered.contains("PR Readiness"));
        assert!(!rendered.contains("Details"));
        assert!(!rendered.contains("ACTION"));
        assert!(rendered.contains("approved"));
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
}
