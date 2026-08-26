use colored::Color as AnsiColor;
use colored::Colorize;
use console::{Color as ConsoleColor, measure_text_width};

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

/// Nerd Font Material Design `file-tree` icon (`nf-md-file_tree`).
const NERD_FILE_TREE_GLYPH: char = '\u{f0645}';

const FALLBACK_WORKTREE_GLYPH: &str = "wt";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorktreeGlyphMode {
    Auto,
    Tree,
    Wt,
}

pub(crate) struct LinkedWorktreeMarker {
    pub plain: String,
    display_width: usize,
}

impl LinkedWorktreeMarker {
    pub(crate) fn slot_width(&self) -> usize {
        self.display_width + 1
    }
}

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

pub(crate) fn parse_worktree_glyph_mode(raw: &str) -> WorktreeGlyphMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "tree" | "nerd" => WorktreeGlyphMode::Tree,
        "wt" | "ascii" | "text" => WorktreeGlyphMode::Wt,
        _ => WorktreeGlyphMode::Auto,
    }
}

pub(crate) fn linked_worktree_marker(mode: WorktreeGlyphMode) -> LinkedWorktreeMarker {
    let plain = if nerd_icons_enabled(mode) {
        NERD_FILE_TREE_GLYPH.to_string()
    } else {
        FALLBACK_WORKTREE_GLYPH.to_string()
    };
    LinkedWorktreeMarker {
        display_width: measure_text_width(&plain),
        plain,
    }
}

pub(crate) fn format_linked_worktree_marker(mode: WorktreeGlyphMode) -> colored::ColoredString {
    linked_worktree_marker(mode).plain.bright_cyan().bold()
}

pub(crate) fn linked_worktree_marker_console_style() -> console::Style {
    console::Style::new()
        .for_stderr()
        .fg(ConsoleColor::Cyan)
        .bright()
        .bold()
}

pub(crate) fn uses_nerd_worktree_glyph(mode: WorktreeGlyphMode) -> bool {
    nerd_icons_enabled(mode)
}

fn nerd_icons_enabled(mode: WorktreeGlyphMode) -> bool {
    match mode {
        WorktreeGlyphMode::Tree => true,
        WorktreeGlyphMode::Wt => false,
        WorktreeGlyphMode::Auto => auto_nerd_icons_enabled(),
    }
}

fn auto_nerd_icons_enabled() -> bool {
    match std::env::var("STAX_NERD_ICONS").ok().as_deref() {
        Some("0" | "false" | "no" | "off") => return false,
        Some("1" | "true" | "yes" | "on") => return true,
        _ => {}
    }

    if std::env::var("NERDFONT").is_ok() || std::env::var("NERDFONT_VERSION").is_ok() {
        return true;
    }

    matches!(
        std::env::var("TERM_PROGRAM").ok().as_deref(),
        Some("WezTerm" | "kitty" | "iTerm.app" | "ghostty" | "WarpTerminal" | "tmux")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_worktree_glyph_mode_recognizes_aliases() {
        assert_eq!(parse_worktree_glyph_mode("auto"), WorktreeGlyphMode::Auto);
        assert_eq!(parse_worktree_glyph_mode("tree"), WorktreeGlyphMode::Tree);
        assert_eq!(parse_worktree_glyph_mode("nerd"), WorktreeGlyphMode::Tree);
        assert_eq!(parse_worktree_glyph_mode("wt"), WorktreeGlyphMode::Wt);
        assert_eq!(parse_worktree_glyph_mode("ascii"), WorktreeGlyphMode::Wt);
    }

    #[test]
    fn linked_worktree_marker_respects_forced_modes() {
        assert_eq!(linked_worktree_marker(WorktreeGlyphMode::Wt).plain, "wt");
        assert_eq!(
            linked_worktree_marker(WorktreeGlyphMode::Tree).plain,
            NERD_FILE_TREE_GLYPH.to_string()
        );
    }

    #[test]
    fn stax_nerd_icons_env_overrides_auto() {
        let key = "STAX_NERD_ICONS";
        let previous = std::env::var(key).ok();

        unsafe {
            std::env::set_var(key, "1");
        }
        assert!(nerd_icons_enabled(WorktreeGlyphMode::Auto));
        unsafe {
            std::env::set_var(key, "0");
        }
        assert!(!nerd_icons_enabled(WorktreeGlyphMode::Auto));

        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }
}
