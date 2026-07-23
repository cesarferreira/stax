//! The `Ctrl`-key text navigation and editing shortcuts shared by every stax
//! TUI — the same set macOS text fields bind by default (`Ctrl+A`/`E` for
//! line ends, `Ctrl+F`/`B` and `Ctrl+N`/`P` to move, `Ctrl+D`/`K` to delete).
//!
//! Each TUI matches on plain [`KeyCode`]s (`Up`, `Home`, `Backspace`, ...). Rather
//! than teach every one of those match arms about `Ctrl`, [`normalize`] rewrites
//! a shortcut into the plain key it stands for *before* dispatch, so the existing
//! arms keep working untouched. [`edit`] then owns the editing keys themselves
//! for the four prompts that share a `(String, usize)` buffer/cursor pair,
//! including the one shortcut — `Ctrl+K` — with no plain-key equivalent.
//!
//! Bindings are deliberately conservative — a user who has never learned them
//! should not be able to lose work or exit the app by fat-fingering one:
//!
//! - `Ctrl+C` is never rewritten; it keeps the quit meaning each TUI gives it.
//! - `Ctrl+G` maps to `Esc` only while typing, so it cancels a prompt but can
//!   never quit the app from a list view the way a bare `Esc` does.
//! - `Ctrl+A`/`E`/`D`/`K` apply only while typing. In a list, `d` and `e` are
//!   already destructive shortcuts and `Ctrl+D` conventionally means
//!   "half page down", so overloading them there would surprise people.
//! - `Ctrl+V`/`Alt+V` page only in list views. In a text field `Ctrl+V` reads as
//!   "paste" to most users, so it does nothing there rather than scrolling.
//! - `Ctrl+H` is intentionally *not* bound: `Backspace` already works, and on
//!   terminals that distinguish the two it would shadow a help key.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Which subset of the shortcuts is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    /// List/dashboard views: motion only, nothing destructive.
    Navigation,
    /// Text entry: motion plus the line-editing shortcuts.
    TextInput,
}

/// Rewrite a `Ctrl`-key shortcut into the plain key it stands for.
///
/// Keys that aren't shortcuts — and `Ctrl+C` — are returned unchanged.
pub fn normalize(key: KeyEvent, context: KeyScope) -> KeyEvent {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    if !ctrl && !alt {
        return key;
    }

    // Every TUI treats Ctrl+C as quit; never shadow it.
    if is_quit(key) {
        return key;
    }

    let editing = context == KeyScope::TextInput;

    let code = match key.code {
        KeyCode::Char('n') if ctrl => KeyCode::Down,
        KeyCode::Char('p') if ctrl => KeyCode::Up,
        KeyCode::Char('f') if ctrl => KeyCode::Right,
        KeyCode::Char('b') if ctrl => KeyCode::Left,
        KeyCode::Char('v') if ctrl && !editing => KeyCode::PageDown,
        KeyCode::Char('v') if alt && !editing => KeyCode::PageUp,
        KeyCode::Char('g') if ctrl && editing => KeyCode::Esc,
        KeyCode::Char('a') if ctrl && editing => KeyCode::Home,
        KeyCode::Char('e') if ctrl && editing => KeyCode::End,
        KeyCode::Char('d') if ctrl && editing => KeyCode::Delete,
        _ => return key,
    };

    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        kind: key.kind,
        state: key.state,
    }
}

/// Apply the cursor motion and editing keys that every stax text prompt shares.
///
/// `key` must already have been through [`normalize`] with
/// [`KeyScope::TextInput`]; the shortcuts with a plain-key equivalent arrive
/// here as that plain key. `Ctrl+K` has no such equivalent, so it is handled
/// directly rather than special-cased by each caller.
///
/// Returns `true` when the key was consumed. Callers keep only the arms that
/// are theirs alone — typically `Esc` and `Enter` — and delegate the rest.
pub fn edit(key: KeyEvent, buffer: &mut String, cursor: &mut usize) -> bool {
    if is_kill_to_end(key) {
        // Guards a cursor that has drifted past the end or into a multi-byte
        // character; `truncate` would panic on either.
        if buffer.is_char_boundary(*cursor) {
            buffer.truncate(*cursor);
        }
        return true;
    }

    match key.code {
        KeyCode::Left if *cursor > 0 => *cursor -= 1,
        KeyCode::Right if *cursor < buffer.len() => *cursor += 1,
        KeyCode::Home => *cursor = 0,
        KeyCode::End => *cursor = buffer.len(),
        KeyCode::Backspace if *cursor > 0 => {
            *cursor -= 1;
            buffer.remove(*cursor);
        }
        KeyCode::Delete if *cursor < buffer.len() => {
            buffer.remove(*cursor);
        }
        KeyCode::Char(c) => {
            buffer.insert(*cursor, c);
            *cursor += 1;
        }
        _ => return false,
    }
    true
}

/// True when `key` is `Ctrl+C`, the quit shortcut every TUI honors.
///
/// [`normalize`] never rewrites it, and each event loop checks it up front to
/// exit before any other dispatch.
pub fn is_quit(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c'))
}

/// True when `key` is `Ctrl+K` (kill-to-end-of-line).
fn is_kill_to_end(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('k'))
}

#[cfg(test)]
mod tests {
    use super::{KeyScope, edit, normalize};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn alt(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT)
    }

    fn code_in(key: KeyEvent, context: KeyScope) -> KeyCode {
        normalize(key, context).code
    }

    #[test]
    fn motion_bindings_apply_in_both_contexts() {
        for context in [KeyScope::Navigation, KeyScope::TextInput] {
            assert_eq!(code_in(ctrl('n'), context), KeyCode::Down);
            assert_eq!(code_in(ctrl('p'), context), KeyCode::Up);
            assert_eq!(code_in(ctrl('f'), context), KeyCode::Right);
            assert_eq!(code_in(ctrl('b'), context), KeyCode::Left);
        }
    }

    #[test]
    fn editing_bindings_apply_only_while_typing() {
        assert_eq!(code_in(ctrl('a'), KeyScope::TextInput), KeyCode::Home);
        assert_eq!(code_in(ctrl('e'), KeyScope::TextInput), KeyCode::End);
        assert_eq!(code_in(ctrl('d'), KeyScope::TextInput), KeyCode::Delete);

        // In a list `d`/`e` are destructive shortcuts — leave them inert.
        for c in ['a', 'e', 'd'] {
            assert_eq!(
                code_in(ctrl(c), KeyScope::Navigation),
                KeyCode::Char(c),
                "Ctrl+{c} must not act in a list view"
            );
        }
    }

    #[test]
    fn paging_applies_only_in_lists() {
        assert_eq!(
            code_in(ctrl('v'), KeyScope::Navigation),
            KeyCode::PageDown
        );
        assert_eq!(code_in(alt('v'), KeyScope::Navigation), KeyCode::PageUp);

        // Ctrl+V means "paste" to most people; don't scroll a text field.
        assert_eq!(
            code_in(ctrl('v'), KeyScope::TextInput),
            KeyCode::Char('v')
        );
        assert_eq!(
            code_in(alt('v'), KeyScope::TextInput),
            KeyCode::Char('v')
        );
    }

    /// Ctrl+G must cancel a prompt but never quit the app, so it only maps to
    /// Esc where Esc means "back out of what I'm typing".
    #[test]
    fn ctrl_g_cancels_input_but_does_not_quit() {
        assert_eq!(code_in(ctrl('g'), KeyScope::TextInput), KeyCode::Esc);
        assert_eq!(
            code_in(ctrl('g'), KeyScope::Navigation),
            KeyCode::Char('g')
        );
    }

    #[test]
    fn ctrl_c_is_never_rewritten() {
        for context in [KeyScope::Navigation, KeyScope::TextInput] {
            let out = normalize(ctrl('c'), context);
            assert_eq!(out.code, KeyCode::Char('c'));
            assert!(out.modifiers.contains(KeyModifiers::CONTROL));
        }
    }

    /// Ctrl+H is deliberately unbound so it can't shadow a help key.
    #[test]
    fn ctrl_h_is_unbound() {
        for context in [KeyScope::Navigation, KeyScope::TextInput] {
            assert_eq!(code_in(ctrl('h'), context), KeyCode::Char('h'));
        }
    }

    #[test]
    fn unmodified_keys_pass_through_untouched() {
        for c in 'a'..='z' {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            assert_eq!(normalize(key, KeyScope::Navigation), key);
            assert_eq!(normalize(key, KeyScope::TextInput), key);
        }
    }

    #[test]
    fn translated_keys_drop_their_modifiers() {
        // Downstream match arms test for a bare Up/Down, so the chord's Ctrl
        // must not survive translation.
        assert!(
            normalize(ctrl('n'), KeyScope::Navigation)
                .modifiers
                .is_empty()
        );
    }

    /// Drive `edit` the way a prompt does, returning the resulting buffer.
    fn edited(start: &str, cursor: usize, key: KeyEvent) -> (String, usize, bool) {
        let mut buffer = String::from(start);
        let mut cursor = cursor;
        let consumed = edit(key, &mut buffer, &mut cursor);
        (buffer, cursor, consumed)
    }

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn ctrl_k_kills_to_end_of_line() {
        assert_eq!(edited("feature/login", 8, ctrl('k')).0, "feature/");
        assert_eq!(edited("abc", 0, ctrl('k')).0, "");
        assert_eq!(edited("abc", 3, ctrl('k')).0, "abc");
    }

    /// A bare `k` is text, not a kill — only the chord truncates.
    #[test]
    fn plain_k_inserts_instead_of_killing() {
        let (buffer, cursor, _) = edited("abc", 1, plain(KeyCode::Char('k')));
        assert_eq!(buffer, "akbc");
        assert_eq!(cursor, 2);
    }

    #[test]
    fn ctrl_k_ignores_a_cursor_past_the_end() {
        assert_eq!(edited("abc", 99, ctrl('k')).0, "abc");
    }

    #[test]
    fn edit_moves_the_cursor_without_touching_the_buffer() {
        assert_eq!(edited("abc", 2, plain(KeyCode::Home)).1, 0);
        assert_eq!(edited("abc", 0, plain(KeyCode::End)).1, 3);
        assert_eq!(edited("abc", 1, plain(KeyCode::Left)).1, 0);
        assert_eq!(edited("abc", 1, plain(KeyCode::Right)).1, 2);
    }

    /// Motion at either end is a no-op rather than an underflow or overrun.
    #[test]
    fn edit_clamps_motion_at_the_buffer_edges() {
        assert_eq!(edited("abc", 0, plain(KeyCode::Left)).1, 0);
        assert_eq!(edited("abc", 3, plain(KeyCode::Right)).1, 3);
        assert_eq!(edited("abc", 0, plain(KeyCode::Backspace)), ("abc".into(), 0, false));
        assert_eq!(edited("abc", 3, plain(KeyCode::Delete)), ("abc".into(), 3, false));
    }

    #[test]
    fn edit_deletes_on_either_side_of_the_cursor() {
        assert_eq!(edited("abc", 2, plain(KeyCode::Backspace)), ("ac".into(), 1, true));
        assert_eq!(edited("abc", 1, plain(KeyCode::Delete)), ("ac".into(), 1, true));
    }

    /// Keys the prompt owns — Esc, Enter — must fall through to the caller.
    #[test]
    fn edit_declines_keys_it_does_not_own() {
        assert!(!edited("abc", 0, plain(KeyCode::Esc)).2);
        assert!(!edited("abc", 0, plain(KeyCode::Enter)).2);
    }
}
