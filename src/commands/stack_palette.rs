use colored::Color as AnsiColor;
use colored::Colorize;
use console::Color as ConsoleColor;

const LANE_RGB: &[(u8, u8, u8)] = &[
    (56, 189, 248),  // sky
    (74, 222, 128),  // emerald
    (163, 230, 53),  // lime
    (250, 204, 21),  // yellow
    (251, 146, 60),  // orange
    (248, 113, 113), // coral
    (244, 114, 182), // pink
    (167, 139, 250), // violet
];

pub(crate) fn lane_rgb(column: usize) -> (u8, u8, u8) {
    LANE_RGB[column % LANE_RGB.len()]
}

pub(crate) fn lane_color(column: usize) -> AnsiColor {
    let (r, g, b) = lane_rgb(column);
    AnsiColor::TrueColor { r, g, b }
}

pub(crate) fn lane_console_color(column: usize) -> ConsoleColor {
    let (r, g, b) = lane_rgb(column);
    ConsoleColor::TrueColor(r, g, b)
}

/// Padded marker so U+2387 (drawn ~half em height in most monospace fonts) reads
/// as a badge instead of a speck beside the stack tree.
pub(crate) const LINKED_WORKTREE_MARKER_PLAIN: &str = " ⎇ ";

const LINKED_WORKTREE_MARKER_BG: (u8, u8, u8) = (28, 52, 68);

pub(crate) fn format_linked_worktree_marker() -> colored::ColoredString {
    LINKED_WORKTREE_MARKER_PLAIN
        .truecolor(120, 220, 255)
        .bold()
        .on_truecolor(
            LINKED_WORKTREE_MARKER_BG.0,
            LINKED_WORKTREE_MARKER_BG.1,
            LINKED_WORKTREE_MARKER_BG.2,
        )
}

pub(crate) fn linked_worktree_marker_console_style() -> console::Style {
    console::Style::new()
        .for_stderr()
        .fg(ConsoleColor::Cyan)
        .bright()
        .bold()
        .bg(ConsoleColor::TrueColor(
            LINKED_WORKTREE_MARKER_BG.0,
            LINKED_WORKTREE_MARKER_BG.1,
            LINKED_WORKTREE_MARKER_BG.2,
        ))
}
