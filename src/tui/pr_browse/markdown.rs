//! Renders GitHub-flavoured Markdown (the way a pull request description or
//! comment is written) into styled, wrapped terminal lines: headings, emphasis,
//! inline code, links, lists, task lists, block quotes, fenced code, rules and
//! tables drawn with box characters.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

const CODE_BG: Color = Color::Indexed(236);
const MIN_COLUMN: usize = 4;

/// A run of text with one style. `"\n"` alone is a forced line break.
#[derive(Clone)]
struct Seg {
    text: String,
    style: Style,
}

fn seg(text: impl Into<String>, style: Style) -> Seg {
    Seg {
        text: text.into(),
        style,
    }
}

fn width_of(text: &str) -> usize {
    Span::raw(text).width()
}

fn spans_width(spans: &[Span]) -> usize {
    spans.iter().map(Span::width).sum()
}

/// Renders `src` for a column `width` cells wide.
pub fn render(src: &str, width: usize) -> Vec<Line<'static>> {
    let mut renderer = Renderer::new(width.max(24));
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(src, options) {
        renderer.event(event);
    }
    renderer.finish()
}

#[derive(Default)]
struct TableState {
    aligns: Vec<Alignment>,
    head: Vec<Vec<Seg>>,
    rows: Vec<Vec<Vec<Seg>>>,
    row: Vec<Vec<Seg>>,
}

struct Renderer {
    width: usize,
    out: Vec<Line<'static>>,
    inline: Vec<Seg>,
    styles: Vec<Style>,
    /// One entry per open list: the next number when ordered.
    lists: Vec<Option<u64>>,
    pending_bullet: Option<String>,
    quote: usize,
    code: Option<(String, String)>,
    table: Option<TableState>,
    links: Vec<String>,
    images: Vec<usize>,
}

impl Renderer {
    fn new(width: usize) -> Self {
        Renderer {
            width,
            out: Vec::new(),
            inline: Vec::new(),
            styles: Vec::new(),
            lists: Vec::new(),
            pending_bullet: None,
            quote: 0,
            code: None,
            table: None,
            links: Vec::new(),
            images: Vec::new(),
        }
    }

    fn current(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, patch: Style) {
        let next = self.current().patch(patch);
        self.styles.push(next);
    }

    fn pop_style(&mut self) {
        self.styles.pop();
    }

    /// A blank line before a block, unless one is already there or this is the start.
    fn gap(&mut self) {
        if self.out.last().is_some_and(|line| !line.spans.is_empty()) {
            self.out.push(Line::default());
        }
    }

    fn quote_prefix(&self) -> Vec<Span<'static>> {
        (0..self.quote)
            .map(|_| Span::styled("▎ ", Style::default().fg(Color::Green)))
            .collect()
    }

    /// Prefixes for the first and the following lines of the current block:
    /// quote bars, list indentation and, once per item, its bullet.
    fn prefixes(&mut self) -> (Vec<Span<'static>>, Vec<Span<'static>>) {
        let mut first = self.quote_prefix();
        let mut rest = self.quote_prefix();
        if !self.lists.is_empty() {
            let indent = "  ".repeat(self.lists.len() - 1);
            match self.pending_bullet.take() {
                Some(bullet) => {
                    let pad = " ".repeat(width_of(&bullet));
                    first.push(Span::raw(indent.clone()));
                    first.push(Span::styled(bullet, Style::default().fg(Color::Cyan)));
                    rest.push(Span::raw(format!("{indent}{pad}")));
                }
                None => {
                    first.push(Span::raw(format!("{indent}  ")));
                    rest.push(Span::raw(format!("{indent}  ")));
                }
            }
        }
        (first, rest)
    }

    fn flush_inline(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let segs = std::mem::take(&mut self.inline);
        let (first, rest) = self.prefixes();
        let lines = wrap(&segs, self.width, first, rest);
        self.out.extend(lines);
    }

    fn text(&mut self, text: &str) {
        if let Some((_, code)) = &mut self.code {
            code.push_str(text);
        } else {
            let style = self.current();
            self.inline.push(seg(text, style));
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                let style = Style::default().fg(Color::Yellow).bg(CODE_BG);
                self.inline.push(seg(format!(" {code} "), style));
            }
            Event::SoftBreak => self.inline.push(seg(" ", self.current())),
            Event::HardBreak => self.inline.push(seg("\n", Style::default())),
            Event::Html(html) => self.html(&html, false),
            Event::InlineHtml(html) => self.html(&html, true),
            Event::Rule => {
                self.flush_inline();
                self.gap();
                let mut line = self.quote_prefix();
                let len = self.width.saturating_sub(spans_width(&line)).min(60);
                line.push(Span::styled(
                    "─".repeat(len),
                    Style::default().fg(Color::DarkGray),
                ));
                self.out.push(Line::from(line));
            }
            Event::TaskListMarker(done) => {
                let (glyph, color) = if done {
                    ("☑ ", Color::Green)
                } else {
                    ("☐ ", Color::Yellow)
                };
                self.inline.push(seg(glyph, Style::default().fg(color)));
            }
            _ => {}
        }
    }

    fn html(&mut self, html: &str, inline: bool) {
        let trimmed = html.trim_start();
        if inline {
            if trimmed.starts_with("<br") {
                self.inline.push(seg("\n", Style::default()));
            } else if trimmed.starts_with("<img") {
                let alt = attribute(trimmed, "alt").unwrap_or_default();
                self.inline.push(seg(image_label(&alt), image_style()));
            }
            return;
        }
        // A block of raw HTML: keep what a reader would see, without the tags.
        let text = strip_tags(html);
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.flush_inline();
        self.gap();
        let style = self.current();
        for line in text.lines() {
            self.inline.push(seg(line.trim(), style));
            self.inline.push(seg("\n", Style::default()));
        }
        self.inline.pop();
        self.flush_inline();
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph => {
                if self.pending_bullet.is_none() {
                    self.gap();
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_inline();
                self.gap();
                let (color, extra) = match level {
                    HeadingLevel::H1 => (Color::Magenta, Modifier::UNDERLINED),
                    HeadingLevel::H2 => (Color::Cyan, Modifier::empty()),
                    HeadingLevel::H3 => (Color::Yellow, Modifier::empty()),
                    _ => (Color::Green, Modifier::empty()),
                };
                self.push_style(
                    Style::default()
                        .fg(color)
                        .add_modifier(Modifier::BOLD | extra),
                );
            }
            Tag::BlockQuote(_) => {
                self.flush_inline();
                if self.quote == 0 && self.lists.is_empty() {
                    self.gap();
                }
                self.quote += 1;
                self.push_style(
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::ITALIC),
                );
            }
            Tag::CodeBlock(kind) => {
                self.flush_inline();
                if self.pending_bullet.is_none() {
                    self.gap();
                }
                let lang = match kind {
                    CodeBlockKind::Fenced(lang) => lang.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush_inline();
                if self.lists.is_empty() {
                    self.gap();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush_inline();
                let bullet = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let bullet = format!("{n}. ");
                        *n += 1;
                        bullet
                    }
                    _ => "• ".to_owned(),
                };
                self.pending_bullet = Some(bullet);
            }
            Tag::Emphasis => self.push_style(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT),
            ),
            Tag::Link { dest_url, .. } => {
                self.links.push(dest_url.to_string());
                self.push_style(
                    Style::default()
                        .fg(Color::LightBlue)
                        .add_modifier(Modifier::UNDERLINED),
                );
            }
            Tag::Image { .. } => {
                self.images.push(self.inline.len());
                self.inline.push(seg("[image: ", image_style()));
                self.push_style(image_style());
            }
            Tag::Table(aligns) => {
                self.flush_inline();
                self.gap();
                self.table = Some(TableState {
                    aligns,
                    ..TableState::default()
                });
            }
            Tag::TableHead | Tag::TableRow => {
                if let Some(table) = &mut self.table {
                    table.row.clear();
                }
            }
            Tag::TableCell => self.inline.clear(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush_inline(),
            TagEnd::Heading(_) => {
                self.flush_inline();
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush_inline();
                self.quote = self.quote.saturating_sub(1);
                self.pop_style();
            }
            TagEnd::CodeBlock => self.finish_code(),
            TagEnd::List(_) => {
                self.flush_inline();
                self.lists.pop();
            }
            TagEnd::Item => {
                self.flush_inline();
                self.pending_bullet = None;
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.links.pop() {
                    let shown: String = self
                        .inline
                        .iter()
                        .rev()
                        .take_while(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .collect();
                    if !url.starts_with('#') && shown.trim() != url && !url.is_empty() {
                        self.inline.push(seg(
                            format!(" ‹{url}›"),
                            Style::default().fg(Color::DarkGray),
                        ));
                    }
                }
            }
            TagEnd::Image => {
                self.pop_style();
                if let Some(start) = self.images.pop() {
                    if self.inline.len() == start + 1 {
                        self.inline[start].text = "[image]".to_owned();
                    } else {
                        self.inline.push(seg("]", image_style()));
                    }
                }
            }
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(table) = &mut self.table {
                    table.row.push(cell);
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = &mut self.table {
                    table.head = std::mem::take(&mut table.row);
                }
            }
            TagEnd::TableRow => {
                if let Some(table) = &mut self.table {
                    let row = std::mem::take(&mut table.row);
                    table.rows.push(row);
                }
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    let lines = render_table(&table, self.width);
                    self.out.extend(lines);
                }
            }
            _ => {}
        }
    }

    fn finish_code(&mut self) {
        let Some((lang, code)) = self.code.take() else {
            return;
        };
        let (mut prefix, _) = self.prefixes();
        if !self.lists.is_empty() && prefix.is_empty() {
            prefix.push(Span::raw("  "));
        }
        let room = self.width.saturating_sub(spans_width(&prefix)).max(8);
        let block = Style::default().fg(Color::Gray).bg(CODE_BG);
        if !lang.is_empty() {
            let mut line = prefix.clone();
            line.push(Span::styled(
                format!(" {lang} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            ));
            self.out.push(Line::from(line));
        }
        for raw in code.trim_end_matches('\n').lines() {
            let text = format!(" {} ", raw.replace('\t', "    "));
            let text = cut(&text, room);
            let pad = room.saturating_sub(width_of(&text));
            let mut line = prefix.clone();
            line.push(Span::styled(format!("{text}{}", " ".repeat(pad)), block));
            self.out.push(Line::from(line));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush_inline();
        while self.out.last().is_some_and(|line| line.spans.is_empty()) {
            self.out.pop();
        }
        self.out
    }
}

fn image_style() -> Style {
    Style::default()
        .fg(Color::Magenta)
        .add_modifier(Modifier::ITALIC)
}

fn image_label(alt: &str) -> String {
    if alt.trim().is_empty() {
        "[image]".to_owned()
    } else {
        format!("[image: {}]", alt.trim())
    }
}

/// `name="value"` from an HTML tag, in either quote style.
fn attribute(tag: &str, name: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let key = format!("{name}={quote}");
        if let Some(start) = tag.find(&key) {
            let rest = &tag[start + key.len()..];
            return rest.find(quote).map(|end| rest[..end].to_owned());
        }
    }
    None
}

/// Text of an HTML fragment: tags and comments removed.
fn strip_tags(html: &str) -> String {
    let mut out = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = if tail.starts_with("<!--") {
            tail.find("-->").map(|e| e + 3)
        } else {
            tail.find('>').map(|e| e + 1)
        };
        match end {
            Some(end) => {
                if tail.starts_with("<br") {
                    out.push('\n');
                }
                rest = &tail[end..];
            }
            None => {
                // An unterminated tag or comment swallows the rest.
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// Cuts `text` to `width` cells.
fn cut(text: &str, width: usize) -> String {
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = width_of(c.encode_utf8(&mut [0; 4]));
        if used + w > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out
}

struct Wrapper {
    lines: Vec<Line<'static>>,
    current: Vec<Span<'static>>,
    used: usize,
    has_content: bool,
    width: usize,
    rest: Vec<Span<'static>>,
}

impl Wrapper {
    fn newline(&mut self) {
        let next = self.rest.clone();
        let line = std::mem::replace(&mut self.current, next);
        self.lines.push(Line::from(line));
        self.used = spans_width(&self.current);
        self.has_content = false;
    }

    fn place(&mut self, word: &str, style: Style, space: bool) {
        let w = width_of(word);
        let sep = usize::from(space && self.has_content);
        if self.has_content && self.used + sep + w > self.width {
            self.newline();
        }
        let sep = usize::from(space && self.has_content);
        if sep == 1 {
            self.current.push(Span::raw(" "));
            self.used += 1;
        }
        if self.used + w > self.width && w > 1 {
            // A word wider than the line is broken where the line ends.
            let mut chunk = String::new();
            for c in word.chars() {
                let cw = width_of(c.encode_utf8(&mut [0; 4]));
                if self.used + width_of(&chunk) + cw > self.width && !chunk.is_empty() {
                    self.used += width_of(&chunk);
                    self.current
                        .push(Span::styled(std::mem::take(&mut chunk), style));
                    self.has_content = true;
                    self.newline();
                }
                chunk.push(c);
            }
            if !chunk.is_empty() {
                self.used += width_of(&chunk);
                self.current.push(Span::styled(chunk, style));
                self.has_content = true;
            }
            return;
        }
        self.current.push(Span::styled(word.to_owned(), style));
        self.used += w;
        self.has_content = true;
    }
}

/// Wraps styled segments to `width` cells. `first` starts the first line and
/// `rest` every later one.
fn wrap(
    segs: &[Seg],
    width: usize,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
) -> Vec<Line<'static>> {
    let used = spans_width(&first);
    let mut wrapper = Wrapper {
        lines: Vec::new(),
        current: first,
        used,
        has_content: false,
        width,
        rest,
    };
    let mut space = false;
    for seg in segs {
        if seg.text == "\n" {
            wrapper.newline();
            space = false;
            continue;
        }
        let mut word = String::new();
        for c in seg.text.chars() {
            if c.is_whitespace() {
                if !word.is_empty() {
                    wrapper.place(&word, seg.style, space);
                    word.clear();
                }
                space = true;
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            wrapper.place(&word, seg.style, space);
            space = false;
        }
    }
    if wrapper.has_content || wrapper.lines.is_empty() {
        wrapper.lines.push(Line::from(wrapper.current));
    }
    wrapper.lines
}

fn plain_width(cell: &[Seg]) -> usize {
    cell.iter()
        .map(|s| if s.text == "\n" { 0 } else { width_of(&s.text) })
        .sum()
}

fn render_table(table: &TableState, width: usize) -> Vec<Line<'static>> {
    let columns = table
        .rows
        .iter()
        .chain(std::iter::once(&table.head))
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    if columns == 0 {
        return Vec::new();
    }

    let mut natural = vec![1; columns];
    for row in table.rows.iter().chain(std::iter::once(&table.head)) {
        for (i, cell) in row.iter().enumerate() {
            natural[i] = natural[i].max(plain_width(cell));
        }
    }
    // Each column costs two cells of padding and one border, plus the left border.
    let room = width.saturating_sub(columns * 3 + 1);
    let mut widths = natural;
    while widths.iter().sum::<usize>() > room {
        let (widest, value) = widths
            .iter()
            .copied()
            .enumerate()
            .max_by_key(|(_, w)| *w)
            .unwrap_or((0, 0));
        if value <= MIN_COLUMN {
            break;
        }
        widths[widest] -= 1;
    }

    let border = Style::default().fg(Color::DarkGray);
    let rule = |left: &str, mid: &str, right: &str| {
        let mut text = left.to_owned();
        for (i, w) in widths.iter().enumerate() {
            text.push_str(&"─".repeat(w + 2));
            text.push_str(if i + 1 == widths.len() { right } else { mid });
        }
        Line::from(Span::styled(text, border))
    };

    let mut out = vec![rule("┌", "┬", "┐")];
    let header_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let emit = |row: &[Vec<Seg>], bold: bool, out: &mut Vec<Line<'static>>| {
        let cells: Vec<Vec<Line<'static>>> = (0..columns)
            .map(|i| {
                let cell = row.get(i).cloned().unwrap_or_default();
                let cell: Vec<Seg> = if bold {
                    cell.into_iter()
                        .map(|s| Seg {
                            style: s.style.patch(header_style),
                            ..s
                        })
                        .collect()
                } else {
                    cell
                };
                wrap(&cell, widths[i], Vec::new(), Vec::new())
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for line_index in 0..height {
            let mut spans = vec![Span::styled("│", border)];
            for (i, cell) in cells.iter().enumerate() {
                let content = cell.get(line_index).cloned().unwrap_or_default();
                let used = spans_width(&content.spans);
                let pad = widths[i].saturating_sub(used);
                let (left, right) = match table.aligns.get(i) {
                    Some(Alignment::Right) => (pad, 0),
                    Some(Alignment::Center) => (pad / 2, pad - pad / 2),
                    _ => (0, pad),
                };
                spans.push(Span::raw(" ".repeat(left + 1)));
                spans.extend(content.spans);
                spans.push(Span::raw(" ".repeat(right + 1)));
                spans.push(Span::styled("│", border));
            }
            out.push(Line::from(spans));
        }
    };
    emit(&table.head, true, &mut out);
    out.push(rule("├", "┼", "┤"));
    for row in &table.rows {
        emit(row, false, &mut out);
    }
    out.push(rule("└", "┴", "┘"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn tables_are_drawn_with_borders_and_aligned_columns() {
        let md = "| Name | Qty |\n|:-----|----:|\n| apple | 3 |\n| fig | 12 |\n";
        let lines = plain(&render(md, 40));
        assert!(lines.iter().all(|l| width_of(l) <= 40), "{lines:#?}");
        assert_eq!(
            lines,
            [
                "┌───────┬─────┐",
                "│ Name  │ Qty │",
                "├───────┼─────┤",
                "│ apple │   3 │",
                "│ fig   │  12 │",
                "└───────┴─────┘",
            ]
        );
    }

    #[test]
    fn wide_tables_shrink_to_the_width_and_wrap_cells() {
        let md = "| a | description |\n|---|---|\n| x | one two three four five six seven eight nine ten |\n";
        let lines = plain(&render(md, 30));
        assert!(lines.iter().all(|l| width_of(l) <= 30), "{lines:#?}");
        assert!(
            lines.len() > 6,
            "long cell should wrap onto several rows: {lines:#?}"
        );
        let joined = lines.join(" ");
        assert!(
            joined.contains("seven") && joined.contains("ten"),
            "{joined}"
        );
    }

    #[test]
    fn headings_emphasis_and_code_lose_their_markers_but_keep_style() {
        let lines = render("## Title\n\nsome **bold** and `code` here", 40);
        let text = plain(&lines);
        assert_eq!(text, ["Title", "", "some bold and code here"]);
        let bold = lines[2]
            .spans
            .iter()
            .find(|s| s.content == "bold")
            .expect("bold span");
        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
        assert!(
            lines[0].spans[0]
                .style
                .add_modifier
                .contains(Modifier::BOLD)
        );
    }

    #[test]
    fn lists_nest_number_and_show_task_state() {
        let md = "- one\n  - nested\n- [x] done\n- [ ] todo\n\n1. first\n2. second\n";
        let text = plain(&render(md, 40));
        assert_eq!(
            text,
            [
                "• one",
                "  • nested",
                "• ☑ done",
                "• ☐ todo",
                "",
                "1. first",
                "2. second",
            ]
        );
    }

    #[test]
    fn quotes_code_blocks_and_rules() {
        let md = "> quoted\n\n```rust\nlet x = 1;\n```\n\n---\n";
        let text = plain(&render(md, 30));
        assert!(text.iter().all(|l| width_of(l) <= 30), "{text:#?}");
        assert_eq!(text[0], "▎ quoted");
        assert_eq!(text[2].trim(), "rust");
        assert_eq!(text[3].trim(), "let x = 1;");
        assert_eq!(width_of(&text[3]), 30, "code block is padded to the width");
        assert!(text[5].starts_with("──"));
    }

    #[test]
    fn long_lines_wrap_and_indent_continuations_under_the_bullet() {
        let text = plain(&render("- alpha beta gamma delta epsilon", 24));
        assert_eq!(text, ["• alpha beta gamma delta", "  epsilon"]);
    }

    #[test]
    fn html_comments_vanish_and_images_become_placeholders() {
        let md = "<!-- template hint -->\nHello ![shot](http://x/y.png) and ![](z.png)\n\n<details>\n<summary>More</summary>\n\nBody\n\n</details>\n";
        let text = plain(&render(md, 60));
        assert_eq!(text[0], "Hello [image: shot] and [image]");
        let joined = text.join("\n");
        assert!(
            joined.contains("More") && joined.contains("Body"),
            "{joined}"
        );
        assert!(!joined.contains('<'), "{joined}");
    }

    #[test]
    fn links_show_their_address_unless_it_is_the_text() {
        let text = plain(&render("[docs](https://x.dev) and <https://y.dev>", 80));
        assert_eq!(text, ["docs ‹https://x.dev› and https://y.dev"]);
    }

    #[test]
    fn a_word_wider_than_the_line_is_broken() {
        let text = plain(&render(&"a".repeat(60), 30));
        assert!(text.iter().all(|l| width_of(l) <= 30), "{text:?}");
        assert_eq!(text.concat().len(), 60);
    }
}
