use super::app::{App, DiffKind, GroupStatus, Overlay, PatchState, Screen, Tab};
use crate::commands::github_list::format_relative_time;
use crate::github::pr_browse::{
    CheckStatus, Detail, EventKind, Mergeability, State, Verdict, checks_status, describe_checks,
    method_key, method_label, parse_time,
};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};

const SELECTED_ROW_BACKGROUND: Color = Color::Indexed(236);
/// Longest a comment runs in the timeline before `e` is needed to read the rest.
const COLLAPSED_LINES: usize = 12;

pub fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    match app.screen {
        Screen::List => render_list_screen(f, app, area),
        Screen::View => render_view_screen(f, app, area),
    }
    match app.overlay.clone() {
        Overlay::None => {}
        Overlay::Help => render_help(f, app.screen),
        Overlay::MergeMethod(methods) => render_merge_picker(f, &methods),
        Overlay::Confirm(action) => render_confirm(f, app, &action),
        Overlay::Comment(text) => render_comment(f, &text),
    }
}

fn dim() -> Style {
    Style::default().fg(Color::DarkGray)
}

fn bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

/// A steady colour per name, so one person reads the same everywhere.
fn author_style(name: &str) -> Style {
    const PALETTE: [Color; 6] = [
        Color::Yellow,
        Color::LightMagenta,
        Color::LightCyan,
        Color::LightGreen,
        Color::LightRed,
        Color::LightBlue,
    ];
    let hash = name
        .bytes()
        .fold(7usize, |h, b| h.wrapping_mul(31).wrapping_add(b as usize));
    Style::default()
        .fg(PALETTE[hash % PALETTE.len()])
        .add_modifier(Modifier::BOLD)
}

/// `key description · key description`, the keys picked out in colour.
fn keys_line(pairs: &[(&str, &str)], width: usize) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    let mut used = 1;
    for (i, (key, what)) in pairs.iter().enumerate() {
        let sep = if i == 0 { 0 } else { 3 };
        let cost = sep + cell_width(key) + 1 + cell_width(what);
        if used + cost > width {
            break;
        }
        if i > 0 {
            spans.push(Span::styled(" · ", dim()));
        }
        spans.push(Span::styled(
            (*key).to_owned(),
            Style::default().fg(Color::Cyan),
        ));
        spans.push(Span::styled(format!(" {what}"), dim()));
        used += cost;
    }
    Line::from(spans)
}

fn cell_width(text: &str) -> usize {
    Span::raw(text).width()
}

/// Cuts `text` to `width` terminal cells, ending in `…` when it is cut.
fn truncate(text: &str, width: usize) -> String {
    if cell_width(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = cell_width(c.encode_utf8(&mut [0; 4]));
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Wraps `text` at `width` cells, keeping its line breaks and long words whole
/// unless one alone is wider than the line.
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let width = width.max(8);
    let mut out = Vec::new();
    for raw in text.replace('\t', "    ").lines() {
        let raw = raw.trim_end();
        if cell_width(raw) <= width {
            out.push(raw.to_owned());
            continue;
        }
        let indent: String = raw.chars().take_while(|c| *c == ' ').collect();
        let mut line = indent.clone();
        for word in raw.split_whitespace() {
            let sep = usize::from(line.trim().len() > 0);
            if cell_width(&line) + sep + cell_width(word) > width && !line.trim().is_empty() {
                out.push(std::mem::replace(&mut line, indent.clone()));
            }
            if !line.trim().is_empty() {
                line.push(' ');
            }
            if cell_width(word) > width {
                // Break a word that can never fit.
                for c in word.chars() {
                    if cell_width(&line) + cell_width(c.encode_utf8(&mut [0; 4])) > width {
                        out.push(std::mem::replace(&mut line, indent.clone()));
                    }
                    line.push(c);
                }
            } else {
                line.push_str(word);
            }
        }
        out.push(line);
    }
    out
}

fn relative(at: &str) -> String {
    parse_time(at).map(format_relative_time).unwrap_or_default()
}

fn state_style(state: State) -> Style {
    let color = match state {
        State::Open => Color::Green,
        State::Draft => Color::Gray,
        State::Merged => Color::Magenta,
        State::Closed => Color::Red,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

fn verdict_span(verdict: Verdict) -> Span<'static> {
    let (glyph, color) = match verdict {
        Verdict::Approved => ("✓", Color::Green),
        Verdict::ChangesRequested => ("✗", Color::Red),
        Verdict::Requested => ("○", Color::Yellow),
        Verdict::Commented => ("●", Color::Cyan),
        Verdict::Dismissed => ("–", Color::DarkGray),
    };
    Span::styled(glyph, Style::default().fg(color))
}

// ---------------------------------------------------------------------------
// List
// ---------------------------------------------------------------------------

fn render_list_screen(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    let filter = if app.filter.is_empty() {
        Span::styled(
            "type to filter by title, repository, number or author",
            dim(),
        )
    } else {
        Span::raw(app.filter.clone())
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                " Pull requests",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(vec![Span::styled(" / ", dim()), filter]),
        ]),
        chunks[0],
    );

    let body = chunks[1];
    app.list_height = (body.height as usize).max(1);
    let width = body.width as usize;
    let groups = app.visible_groups();
    let mut lines: Vec<Line> = Vec::new();
    let mut selected_row = 0;
    let mut index = 0;
    for (group, state) in groups.iter().zip(&app.groups) {
        let note = match &state.status {
            GroupStatus::Loading => Some(Span::styled(" loading…", dim())),
            GroupStatus::Failed(error) => Some(Span::styled(
                format!(
                    " {}",
                    truncate(error.lines().next().unwrap_or(""), width.saturating_sub(24))
                ),
                Style::default().fg(Color::Red),
            )),
            GroupStatus::Done => None,
        };
        if group.entries.is_empty() && note.is_none() {
            continue;
        }
        let mut heading = vec![Span::styled(format!(" {}", group.title), bold())];
        if !group.entries.is_empty() {
            heading.push(Span::styled(format!(" ({})", group.entries.len()), dim()));
        }
        heading.extend(note);
        lines.push(Line::from(heading));
        for entry in &group.entries {
            let selected = index == app.cursor;
            if selected {
                selected_row = lines.len();
            }
            let bg = if selected {
                Style::default().bg(SELECTED_ROW_BACKGROUND)
            } else {
                Style::default()
            };
            let number = format!("#{}", entry.pr.number);
            let draft = if entry.draft { " [draft]" } else { "" };
            let title_room = width.saturating_sub(cell_width(&number) + cell_width(draft) + 5);
            let mut first = vec![
                Span::styled(
                    if selected { " ▌ " } else { "   " },
                    Style::default().fg(Color::Cyan),
                ),
                Span::styled(number, Style::default().fg(Color::Cyan)),
                Span::raw(" "),
                Span::styled(truncate(&entry.title, title_room), bold()),
            ];
            if entry.draft {
                first.push(Span::styled(draft, Style::default().fg(Color::Yellow)));
            }
            let when = relative(&entry.updated);
            let second = vec![
                Span::raw("      "),
                Span::styled(entry.pr.repo.clone(), Style::default().fg(Color::LightBlue)),
                Span::styled(" · ", dim()),
                Span::styled(entry.author.clone(), author_style(&entry.author)),
                Span::styled(format!(" · {when}"), dim()),
            ];
            lines.push(Line::from(first).style(bg));
            lines.push(Line::from(second).style(bg));
            index += 1;
        }
    }
    if index == 0 && lines.is_empty() {
        lines.push(Line::from(Span::styled(" Nothing found.", dim())));
    } else if index == 0 {
        lines.push(Line::from(Span::styled(
            if app.filter.is_empty() {
                " No open pull requests."
            } else {
                " Nothing matches the filter."
            },
            dim(),
        )));
    }

    // Keep the selected row (and its second line) on screen.
    let height = body.height as usize;
    let top = (selected_row + 2).saturating_sub(height);
    let top = top.min(selected_row);
    f.render_widget(Paragraph::new(lines).scroll((top as u16, 0)), body);

    let footer = match &app.status {
        Some(status) => status_line(status),
        None => keys_line(
            &[
                ("↑/↓", "move"),
                ("enter", "open"),
                ("ctrl+y", "copy number"),
                ("ctrl+r", "reload"),
                ("esc", "quit"),
            ],
            chunks[2].width as usize,
        ),
    };
    f.render_widget(Paragraph::new(footer), chunks[2]);
}

// ---------------------------------------------------------------------------
// View
// ---------------------------------------------------------------------------

fn render_view_screen(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_view_header(f, app, chunks[0]);

    let body = chunks[1];
    app.view_height = body.height as usize;
    let width = body.width.saturating_sub(2) as usize;

    let content: Vec<Line> = match (&app.detail, &app.detail_error) {
        (Some(detail), _) => match app.tab {
            Tab::Summary => summary_lines(detail, width),
            Tab::Timeline => timeline_lines(detail, width, app.expanded),
            Tab::Code => code_lines(app, width),
        },
        (None, Some(error)) => {
            let mut lines = vec![Line::from(Span::styled(
                "Couldn't read this pull request.",
                Style::default().fg(Color::Red),
            ))];
            lines.extend(wrap_text(error, width).into_iter().map(Line::from));
            lines.push(Line::default());
            lines.push(Line::from(Span::styled("Press R to try again.", dim())));
            lines
        }
        (None, None) => vec![Line::from(Span::styled("Loading…", dim()))],
    };

    app.content_len = content.len();
    let max = app.max_scroll();
    let scroll = app.scroll_mut();
    *scroll = (*scroll).min(max);
    let scroll = *scroll;
    let hscroll = if app.tab == Tab::Code { app.hscroll } else { 0 };
    let inner = Rect {
        x: body.x + 1,
        width: body.width.saturating_sub(1),
        ..body
    };
    f.render_widget(
        Paragraph::new(content).scroll((
            scroll.min(u16::MAX as usize) as u16,
            hscroll.min(u16::MAX as usize) as u16,
        )),
        inner,
    );

    render_view_footer(f, app, chunks[2]);
}

fn render_view_header(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;
    let Some(pr) = &app.current else { return };
    let (title, meta) = match &app.detail {
        Some(d) => (
            d.title.clone(),
            vec![
                Span::styled(format!(" ● {}", d.state.label()), state_style(d.state)),
                Span::styled(format!("  {} · by ", pr.label()), dim()),
                Span::styled(d.author.clone(), author_style(&d.author)),
                Span::styled(format!(" · updated {} · ", relative(&d.updated)), dim()),
                Span::styled(
                    format!("+{}", d.additions),
                    Style::default().fg(Color::Green),
                ),
                Span::raw(" "),
                Span::styled(format!("−{}", d.deletions), Style::default().fg(Color::Red)),
            ],
        ),
        None => (
            String::new(),
            vec![Span::styled(format!(" {}", pr.label()), dim())],
        ),
    };

    let mut tabs = vec![Span::raw(" ")];
    for tab in Tab::ALL {
        let label = match tab {
            Tab::Summary => "1 Summary".to_owned(),
            Tab::Timeline => match &app.detail {
                Some(d) => format!("2 Timeline ({})", d.timeline.len()),
                None => "2 Timeline".to_owned(),
            },
            Tab::Code => "3 Code".to_owned(),
        };
        let style = if tab == app.tab {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            dim()
        };
        tabs.push(Span::styled(label, style));
        tabs.push(Span::raw("   "));
    }

    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(" {}", truncate(&title, width.saturating_sub(2))),
                bold(),
            )),
            Line::from(meta),
            Line::from(tabs),
        ]),
        area,
    );
}

/// A message in the footer: failures red, work in progress yellow, the rest green.
fn status_line(text: &str) -> Line<'static> {
    let color = if text.starts_with("Couldn't") {
        Color::Red
    } else if text.ends_with('…') {
        Color::Yellow
    } else {
        Color::Green
    };
    Line::from(Span::styled(format!(" {text}"), Style::default().fg(color)))
}

fn render_view_footer(f: &mut Frame, app: &App, area: Rect) {
    let line = if let Some(busy) = &app.busy {
        status_line(busy)
    } else if let Some(status) = &app.status {
        status_line(status)
    } else {
        let mut pairs = vec![("←/→", "tabs"), ("j/k", "scroll")];
        if app.tab == Tab::Code {
            pairs.extend([("[/]", "file"), ("h/l", "sideways")]);
        }
        pairs.extend([
            ("m", "merge"),
            ("x", "close"),
            ("r", "ready"),
            ("a", "approve"),
            ("c", "comment"),
            ("o", "browser"),
            ("?", "keys"),
            ("q", if app.direct { "quit" } else { "back" }),
        ]);
        keys_line(&pairs, area.width as usize)
    };
    f.render_widget(Paragraph::new(line), area);
}

fn label_value(label: &str, mut value: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!("{label:<10}"), dim())];
    spans.append(&mut value);
    Line::from(spans)
}

fn summary_lines(detail: &Detail, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    match detail.state {
        State::Merged => lines.push(Line::from(Span::styled(
            "Merged",
            Style::default().fg(Color::Magenta),
        ))),
        State::Closed => lines.push(Line::from(Span::styled(
            "Closed without merging (x reopens it)",
            Style::default().fg(Color::Red),
        ))),
        State::Draft => lines.push(Line::from(Span::styled(
            "Draft: press r to mark it ready for review",
            dim(),
        ))),
        State::Open => {
            let color = match detail.mergeability {
                Mergeability::Ready => Color::Green,
                Mergeability::Unknown => Color::DarkGray,
                Mergeability::Blocked | Mergeability::Behind | Mergeability::Unstable => {
                    Color::Yellow
                }
                Mergeability::Conflicts => Color::Red,
            };
            lines.push(Line::from(Span::styled(
                detail.mergeability.label(),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )));
        }
    }
    lines.push(Line::default());

    lines.push(label_value(
        "Branch",
        vec![
            Span::styled(detail.head.clone(), Style::default().fg(Color::Cyan)),
            Span::styled(" → ", dim()),
            Span::styled(detail.base.clone(), Style::default().fg(Color::LightBlue)),
        ],
    ));
    if !detail.reviewers.is_empty() {
        let mut spans = Vec::new();
        for (i, reviewer) in detail.reviewers.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled("  ", dim()));
            }
            spans.push(verdict_span(reviewer.verdict));
            spans.push(Span::raw(format!(" {}", reviewer.login)));
            spans.push(Span::styled(
                format!(" ({})", reviewer.verdict.label()),
                dim(),
            ));
        }
        lines.push(label_value("Reviewers", spans));
    }
    if !detail.labels.is_empty() {
        lines.push(label_value(
            "Labels",
            vec![Span::styled(
                detail.labels.join(", "),
                Style::default().fg(Color::Yellow),
            )],
        ));
    }

    let status_color = match checks_status(&detail.checks) {
        Some(CheckStatus::Failed) => Color::Red,
        Some(CheckStatus::Running) => Color::Yellow,
        Some(_) => Color::Green,
        None => Color::DarkGray,
    };
    lines.push(label_value(
        "Checks",
        vec![Span::styled(
            describe_checks(&detail.checks),
            Style::default().fg(status_color),
        )],
    ));
    for check in detail
        .checks
        .iter()
        .filter(|c| matches!(c.status, CheckStatus::Failed | CheckStatus::Running))
        .take(15)
    {
        let (glyph, color) = if check.status == CheckStatus::Failed {
            ("✗", Color::Red)
        } else {
            ("◌", Color::Yellow)
        };
        lines.push(Line::from(vec![
            Span::raw("          "),
            Span::styled(glyph, Style::default().fg(color)),
            Span::raw(format!(
                " {}",
                truncate(&check.name, width.saturating_sub(14))
            )),
        ]));
    }

    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Description", bold())));
    lines.push(Line::default());
    if detail.body.trim().is_empty() {
        lines.push(Line::from(Span::styled("No description provided.", dim())));
    } else {
        lines.extend(super::markdown::render(&detail.body, width));
    }
    lines
}

fn timeline_lines(detail: &Detail, width: usize, expanded: bool) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for event in &detail.timeline {
        let actor = if event.actor.is_empty() {
            String::new()
        } else {
            format!("{} ", event.actor)
        };
        let when = relative(&event.at);
        let (glyph, color, what, body, path) = match &event.kind {
            EventKind::Opened => (
                "●",
                Color::Green,
                "opened this pull request".to_owned(),
                None,
                None,
            ),
            EventKind::Commit { sha, headline } => (
                "◆",
                Color::DarkGray,
                format!("committed {sha}  {headline}"),
                None,
                None,
            ),
            EventKind::Comment { body } => {
                ("◉", Color::Cyan, "commented".to_owned(), Some(body), None)
            }
            EventKind::Review { verdict, body } => (
                "◉",
                match verdict {
                    Verdict::Approved => Color::Green,
                    Verdict::ChangesRequested => Color::Red,
                    _ => Color::Cyan,
                },
                verdict.label().to_owned(),
                Some(body),
                None,
            ),
            EventKind::Inline { path, body } => (
                "◉",
                Color::Cyan,
                "commented on".to_owned(),
                Some(body),
                Some(path),
            ),
            EventKind::Merged => (
                "◆",
                Color::Magenta,
                "merged this pull request".to_owned(),
                None,
                None,
            ),
            EventKind::Closed => (
                "◆",
                Color::Red,
                "closed this pull request".to_owned(),
                None,
                None,
            ),
        };
        let mut head = vec![
            Span::styled(glyph, Style::default().fg(color)),
            Span::styled(format!(" {actor}"), author_style(&event.actor)),
            Span::raw(truncate(
                &what,
                width.saturating_sub(cell_width(&actor) + 14),
            )),
        ];
        if let Some(path) = path {
            head.push(Span::styled(
                format!(" {path}"),
                Style::default().fg(Color::Yellow),
            ));
        }
        if !when.is_empty() {
            head.push(Span::styled(format!("  {when}"), dim()));
        }
        lines.push(Line::from(head));

        if let Some(body) = body {
            let rendered = super::markdown::render(body, width.saturating_sub(4));
            let total = rendered.len();
            let shown = if expanded {
                total
            } else {
                total.min(COLLAPSED_LINES)
            };
            for line in rendered.into_iter().take(shown) {
                let mut spans = vec![Span::styled("  │ ", Style::default().fg(color))];
                spans.extend(line.spans);
                lines.push(Line::from(spans));
            }
            if shown < total {
                lines.push(Line::from(Span::styled(
                    format!("  │ … {} more lines (e expands)", total - shown),
                    dim(),
                )));
            }
        }
        lines.push(Line::default());
    }
    lines
}

fn code_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    match &app.patch {
        PatchState::None | PatchState::Loading => {
            vec![Line::from(Span::styled("Loading the diff…", dim()))]
        }
        PatchState::Failed(error) => {
            let mut lines = vec![Line::from(Span::styled(
                "Couldn't fetch the diff.",
                Style::default().fg(Color::Red),
            ))];
            lines.extend(wrap_text(error, width).into_iter().map(Line::from));
            lines
        }
        PatchState::Ready(patch) if patch.rows.is_empty() => {
            vec![Line::from(Span::styled("No changes.", dim()))]
        }
        PatchState::Ready(patch) => patch
            .rows
            .iter()
            .map(|row| {
                let gutter = |n: Option<u32>| match n {
                    Some(n) => format!("{n:>5}"),
                    None => "     ".to_owned(),
                };
                match row.kind {
                    DiffKind::File => Line::from(Span::styled(
                        format!("▌ {}", row.text),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
                    DiffKind::Meta => Line::from(Span::styled(row.text.clone(), dim())),
                    DiffKind::Hunk => Line::from(Span::styled(
                        row.text.clone(),
                        Style::default().fg(Color::Magenta),
                    )),
                    kind => {
                        let (sign, style, tint) = match kind {
                            DiffKind::Add => (
                                '+',
                                Style::default().fg(Color::LightGreen),
                                Style::default().bg(Color::Indexed(22)),
                            ),
                            DiffKind::Del => (
                                '-',
                                Style::default().fg(Color::LightRed),
                                Style::default().bg(Color::Indexed(52)),
                            ),
                            _ => (' ', Style::default(), Style::default()),
                        };
                        Line::from(vec![
                            Span::styled(
                                format!("{} {} ", gutter(row.old), gutter(row.new)),
                                dim(),
                            ),
                            Span::styled(
                                format!("{sign}{}", row.text.replace('\t', "    ")),
                                style,
                            ),
                        ])
                        .style(tint)
                    }
                }
            })
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn popup(f: &mut Frame, title: &str, lines: Vec<Line<'static>>, width: u16) {
    let area = centered(f.area(), width, lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(format!(" {title} "), bold())),
        ),
        area,
    );
}

fn render_help(f: &mut Frame, screen: Screen) {
    let rows: &[(&str, &str)] = match screen {
        Screen::List => &[
            ("type", "filter by title, repository, number or author"),
            ("↑/↓ ctrl+p/n", "move"),
            ("pgup/pgdn", "move by a page"),
            ("enter", "open the pull request"),
            ("ctrl+y", "copy the number"),
            ("ctrl+r", "reload"),
            ("esc", "clear the filter, then quit"),
        ],
        Screen::View => &[
            ("←/→ tab 1 2 3", "Summary, Timeline, Code"),
            ("j/k ↑/↓", "scroll"),
            ("ctrl+d/u space/b", "half page / page"),
            ("g/G", "top / bottom"),
            ("[ ] h/l", "Code: previous/next file, scroll sideways"),
            ("e", "Timeline: expand long comments"),
            ("m", "merge (pick squash, merge or rebase, then confirm)"),
            ("x", "close, or reopen a closed one (asks first)"),
            ("r", "mark a draft ready, or convert back to draft"),
            ("a", "approve"),
            ("c", "comment (enter posts, alt+enter adds a line)"),
            ("o", "open on GitHub"),
            ("y Y n", "copy the link / checkout command / number"),
            ("R", "reload"),
            ("q esc", "back"),
        ],
    };
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(key, what)| {
            Line::from(vec![
                Span::styled(format!(" {key:<18}"), Style::default().fg(Color::Cyan)),
                Span::raw((*what).to_owned()),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(" any key closes this", dim())));
    popup(f, "Keys", lines, 78);
}

fn render_merge_picker(f: &mut Frame, methods: &[crate::forge::MergeMethod]) {
    let mut lines: Vec<Line> = methods
        .iter()
        .map(|method| {
            Line::from(vec![
                Span::styled(
                    format!(" {} ", method_key(*method)),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(method_label(*method)),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(" esc cancels", dim())));
    popup(f, "Merge", lines, 40);
}

fn render_confirm(f: &mut Frame, app: &App, action: &crate::github::pr_browse::Action) {
    use crate::github::pr_browse::Action;
    let label = app
        .current
        .as_ref()
        .map(|pr| format!("#{}", pr.number))
        .unwrap_or_default();
    let question = match action {
        Action::Merge(method) => format!("{} {label}?", method_label(*method)),
        Action::Close => format!("Close {label} without merging?"),
        Action::Reopen => format!("Reopen {label}?"),
        _ => format!("Do this to {label}?"),
    };
    let mut lines = vec![
        Line::from(Span::styled(format!(" {question}"), bold())),
        Line::default(),
    ];
    if let (Action::Merge(_), Some(d)) = (action, &app.detail)
        && d.mergeability != Mergeability::Ready
    {
        lines.push(Line::from(Span::styled(
            format!(" Note: {}", d.mergeability.label()),
            Style::default().fg(Color::Yellow),
        )));
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(
        " y confirms · anything else cancels",
        dim(),
    )));
    popup(f, "Confirm", lines, 60);
}

fn render_comment(f: &mut Frame, text: &str) {
    let area = f.area();
    let width = area.width.saturating_sub(8).clamp(20, 90);
    let inner = width.saturating_sub(2) as usize;
    let mut lines: Vec<Line> = Vec::new();
    for (i, raw) in text.split('\n').enumerate() {
        let wrapped = wrap_text(raw, inner.saturating_sub(1));
        let wrapped = if wrapped.is_empty() {
            vec![String::new()]
        } else {
            wrapped
        };
        let last_raw = i + 1 == text.split('\n').count();
        let count = wrapped.len();
        for (j, line) in wrapped.into_iter().enumerate() {
            let mut spans = vec![Span::raw(line)];
            if last_raw && j + 1 == count {
                spans.push(Span::styled("▏", Style::default().fg(Color::Cyan)));
            }
            lines.push(Line::from(spans));
        }
    }
    let max_lines = (area.height.saturating_sub(6) as usize).max(3);
    if lines.len() > max_lines {
        lines.drain(..lines.len() - max_lines);
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        "enter posts · alt+enter adds a line · esc cancels",
        dim(),
    )));
    popup(f, "Comment", lines, width);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_on_words_and_keeps_blank_lines() {
        assert_eq!(
            wrap_text("one two three four\n\nfive", 9),
            ["one two", "three", "four", "", "five"]
        );
    }

    #[test]
    fn wraps_a_word_wider_than_the_line() {
        let lines = wrap_text("abcdefghijklmnopqrstuvwxyz", 10);
        assert!(lines.iter().all(|l| cell_width(l) <= 10), "{lines:?}");
        assert_eq!(lines.concat(), "abcdefghijklmnopqrstuvwxyz");
    }

    #[test]
    fn wrapping_counts_wide_characters_as_two_cells() {
        let lines = wrap_text("日本語日本語 日本語", 8);
        assert!(lines.iter().all(|l| cell_width(l) <= 8), "{lines:?}");
    }

    #[test]
    fn truncate_ends_in_an_ellipsis_within_the_width() {
        assert_eq!(truncate("short", 10), "short");
        let cut = truncate("a long title that will not fit", 10);
        assert!(cell_width(&cut) <= 10, "{cut}");
        assert!(cut.ends_with('…'));
    }
}
