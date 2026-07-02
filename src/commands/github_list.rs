use chrono::{DateTime, Utc};
use colored::Colorize;
use console::{measure_text_width, truncate_str};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CellTone {
    Default,
    Id,
    StateOpen,
    StateDraft,
    Branch,
    Secondary,
    Label,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TruncationMode {
    None,
    End,
    Middle,
}

pub(crate) struct TableColumn<'a> {
    pub header: &'a str,
    pub width: usize,
}

pub(crate) struct TableCell {
    pub text: String,
    pub tone: CellTone,
    pub truncation: TruncationMode,
}

pub(crate) fn terminal_width() -> usize {
    console::Term::stdout().size().1 as usize
}

pub(crate) fn format_relative_time(timestamp: DateTime<Utc>) -> String {
    let delta = Utc::now().signed_duration_since(timestamp);

    if delta.num_minutes() < 1 {
        "now".to_string()
    } else if delta.num_hours() < 1 {
        format!("{}m ago", delta.num_minutes())
    } else if delta.num_days() < 1 {
        format!("{}h ago", delta.num_hours())
    } else if delta.num_days() < 7 {
        format!("{}d ago", delta.num_days())
    } else if delta.num_days() < 30 {
        format!("{}w ago", delta.num_days() / 7)
    } else if delta.num_days() < 365 {
        format!("{}mo ago", delta.num_days() / 30)
    } else {
        format!("{}y ago", delta.num_days() / 365)
    }
}

pub(crate) fn print_table(
    repo_label: &str,
    summary: &str,
    empty_message: &str,
    columns: &[TableColumn<'_>],
    rows: &[Vec<TableCell>],
) {
    println!("{}  {}", repo_label.cyan().bold(), summary.dimmed());

    if rows.is_empty() {
        println!("{}", empty_message.dimmed());
        return;
    }

    let header = columns
        .iter()
        .map(|column| pad_plain(column.header, column.width).bold().to_string())
        .collect::<Vec<_>>()
        .join("  ");
    println!("{}", header);

    let divider_width = columns.iter().map(|column| column.width).sum::<usize>()
        + (columns.len().saturating_sub(1) * 2);
    println!("{}", "─".repeat(divider_width).dimmed());

    for row in rows {
        let rendered = row
            .iter()
            .zip(columns.iter())
            .map(|(cell, column)| render_cell(cell, column.width))
            .collect::<Vec<_>>()
            .join("  ");
        println!("{}", rendered);
    }
}

pub(crate) fn split_flexible_width(
    total_width: usize,
    leading_min: usize,
    trailing_pref: usize,
    trailing_min: usize,
    trailing_max: usize,
) -> (usize, usize) {
    if total_width == 0 {
        return (0, 0);
    }

    if total_width <= leading_min {
        return (total_width, 0);
    }

    let max_trailing = total_width.saturating_sub(leading_min);
    let trailing = trailing_pref
        .clamp(trailing_min.min(total_width), trailing_max.min(total_width))
        .min(max_trailing);
    let leading = total_width.saturating_sub(trailing);
    (leading, trailing)
}

fn render_cell(cell: &TableCell, width: usize) -> String {
    let fitted = truncate_to_width(&cell.text, width, cell.truncation);
    let padding = width.saturating_sub(measure_text_width(&fitted));
    format!("{}{}", apply_tone(&fitted, cell.tone), " ".repeat(padding))
}

fn apply_tone(text: &str, tone: CellTone) -> String {
    match tone {
        CellTone::Default => text.to_string(),
        CellTone::Id => text.bright_magenta().bold().to_string(),
        CellTone::StateOpen => text.green().bold().to_string(),
        CellTone::StateDraft => text.yellow().bold().to_string(),
        CellTone::Branch => text.cyan().to_string(),
        CellTone::Secondary => text.dimmed().to_string(),
        CellTone::Label => text.blue().to_string(),
    }
}

fn pad_plain(text: &str, width: usize) -> String {
    let padding = width.saturating_sub(measure_text_width(text));
    format!("{}{}", text, " ".repeat(padding))
}

fn truncate_to_width(text: &str, width: usize, mode: TruncationMode) -> String {
    if width == 0 {
        return String::new();
    }

    match mode {
        TruncationMode::None => text.to_string(),
        TruncationMode::End => truncate_str(text, width, "...").into_owned(),
        TruncationMode::Middle => middle_truncate(text, width),
    }
}

fn middle_truncate(text: &str, width: usize) -> String {
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
    let prefix: String = chars.iter().take(front).collect();
    let suffix: String = chars
        .iter()
        .rev()
        .take(back)
        .copied()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{}...{}", prefix, suffix)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn format_relative_time_prefers_compact_units() {
        let timestamp = Utc::now() - Duration::hours(3);
        assert_eq!(format_relative_time(timestamp), "3h ago");
    }

    #[test]
    fn middle_truncate_preserves_suffix() {
        let truncated = middle_truncate("rawnam/02-12/feat-upstream-sync-and-pr-support", 24);
        assert!(truncated.starts_with("rawnam/"));
        assert!(truncated.ends_with("support"));
        assert!(measure_text_width(&truncated) <= 24);
    }
}
