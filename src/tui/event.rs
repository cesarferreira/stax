use super::keys::{self, KeyScope};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use std::time::Duration;

/// Poll for keyboard events with a timeout
pub fn poll_event(timeout: Duration) -> std::io::Result<Option<Event>> {
    if event::poll(timeout)? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

/// Key event types we care about
#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    // Navigation
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,

    // Actions
    Restack,
    RestackAll,
    Submit,
    OpenPr,
    NewBranch,
    Delete,
    Rename,

    // Modes
    Search,
    Help,
    Quit,
    ReorderMode,
    MovePicker,
    ToggleStackPane,
    ToggleSummaryPane,
    TogglePatchPane,

    // Reorder mode actions
    MoveUp,
    MoveDown,

    // Text input
    Char(char),
    Backspace,
    Home,
    End,

    // Pane navigation
    Tab,

    // Unknown
    None,
}

/// Current input context for key mapping
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyContext {
    Normal,
    Search,
    Input,
    Confirm,
    Help,
    Reorder,
    MovePicker,
}

impl KeyContext {
    /// Contexts where the user is typing get the line-editing shortcuts;
    /// the rest get motion only.
    fn key_scope(self) -> KeyScope {
        match self {
            KeyContext::Search | KeyContext::Input | KeyContext::MovePicker => {
                KeyScope::TextInput
            }
            KeyContext::Normal | KeyContext::Confirm | KeyContext::Help | KeyContext::Reorder => {
                KeyScope::Navigation
            }
        }
    }
}

impl From<KeyEvent> for KeyAction {
    fn from(key: KeyEvent) -> Self {
        Self::from_key(key, KeyContext::Normal)
    }
}

impl KeyAction {
    pub fn from_key(key: KeyEvent, context: KeyContext) -> Self {
        // Handle Ctrl+C for quit
        if keys::is_quit(key) {
            return KeyAction::Quit;
        }

        // Rewrite Ctrl-key shortcuts (Ctrl+N, Ctrl+P, ...) into the plain keys
        // the match arms below already understand.
        let key = keys::normalize(key, context.key_scope());

        // Handle Shift modifiers
        if key.modifiers.contains(KeyModifiers::SHIFT)
            && !matches!(
                context,
                KeyContext::Input | KeyContext::Search | KeyContext::MovePicker,
            )
        {
            match key.code {
                KeyCode::Char('R') | KeyCode::Char('r') => return KeyAction::RestackAll,
                KeyCode::Char('K') | KeyCode::Char('k') => return KeyAction::MoveUp,
                KeyCode::Char('J') | KeyCode::Char('j') => return KeyAction::MoveDown,
                KeyCode::Up => return KeyAction::MoveUp,
                KeyCode::Down => return KeyAction::MoveDown,
                _ => {}
            }
        }

        match key.code {
            // Navigation
            KeyCode::Up => KeyAction::Up,
            KeyCode::Down => KeyAction::Down,
            KeyCode::Left => KeyAction::Left,
            KeyCode::Right => KeyAction::Right,
            KeyCode::Enter => KeyAction::Enter,
            KeyCode::Esc => KeyAction::Escape,
            KeyCode::Home => KeyAction::Home,
            KeyCode::End => KeyAction::End,
            KeyCode::Tab => KeyAction::Tab,

            // Text input (and mode-specific shortcuts handled by each mode handler)
            KeyCode::Char(c) => KeyAction::Char(c),
            KeyCode::Backspace => KeyAction::Backspace,

            _ => KeyAction::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyAction, KeyContext};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    #[test]
    fn normal_mode_keeps_shortcuts() {
        let action = KeyAction::from_key(
            KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE),
            KeyContext::Normal,
        );
        assert_eq!(action, KeyAction::Char('n'));
    }

    #[test]
    fn input_mode_treats_shortcut_letters_as_text() {
        for c in ['n', 'r', 's', 'q', 'd', 'e', 'p', 'o', 'j', 'k'] {
            let action = KeyAction::from_key(
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                KeyContext::Input,
            );
            assert_eq!(action, KeyAction::Char(c));
        }
    }

    #[test]
    fn input_mode_accepts_all_lowercase_letters() {
        for c in 'a'..='z' {
            let action = KeyAction::from_key(
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                KeyContext::Input,
            );
            assert_eq!(action, KeyAction::Char(c));
        }
    }

    #[test]
    fn search_mode_treats_shortcut_letters_as_text() {
        let action = KeyAction::from_key(
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            KeyContext::Search,
        );
        assert_eq!(action, KeyAction::Char('q'));
    }

    #[test]
    fn input_mode_keeps_control_keys() {
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Escape
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Enter
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Backspace
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Home, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Home
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::End, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::End
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Left
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
                KeyContext::Input
            ),
            KeyAction::Right
        );
    }

    #[test]
    fn ctrl_c_quits_in_all_modes() {
        let action = KeyAction::from_key(
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            KeyContext::Input,
        );
        assert_eq!(action, KeyAction::Quit);
    }

    #[test]
    fn input_mode_allows_shifted_letters_as_text() {
        let action = KeyAction::from_key(
            KeyEvent::new(KeyCode::Char('K'), KeyModifiers::SHIFT),
            KeyContext::Input,
        );
        assert_eq!(action, KeyAction::Char('K'));
    }

    /// MovePicker is a text-input context (users type a filter query). Shift
    /// must not remap `k`/`j`/`r` to MoveUp/MoveDown/RestackAll the way it
    /// does in Normal/Reorder — those are valid characters in a branch name.
    #[test]
    fn move_picker_mode_treats_shortcut_letters_as_text() {
        for c in ['n', 'r', 's', 'q', 'd', 'e', 'p', 'o', 'm', 'j', 'k'] {
            let action = KeyAction::from_key(
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE),
                KeyContext::MovePicker,
            );
            assert_eq!(
                action,
                KeyAction::Char(c),
                "char '{}' should pass through",
                c
            );
        }
    }

    /// Ctrl+N/Ctrl+P must reach the same actions as the arrow keys, in every
    /// context — including the text-entry ones, where a bare `n`/`p` is text.
    #[test]
    fn ctrl_motion_maps_to_arrow_actions() {
        for context in [
            KeyContext::Normal,
            KeyContext::Confirm,
            KeyContext::Help,
            KeyContext::Reorder,
            KeyContext::Input,
            KeyContext::Search,
            KeyContext::MovePicker,
        ] {
            for (c, expected) in [
                ('n', KeyAction::Down),
                ('p', KeyAction::Up),
                ('f', KeyAction::Right),
                ('b', KeyAction::Left),
            ] {
                let action = KeyAction::from_key(
                    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL),
                    context,
                );
                assert_eq!(action, expected, "Ctrl+{c} in {context:?}");
            }
        }
    }

    /// Ctrl+A/Ctrl+E are line motions while typing, but `a`/`e` are shortcuts
    /// in a list view, so they stay inert there.
    #[test]
    fn ctrl_line_motion_is_limited_to_text_entry() {
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL),
                KeyContext::Input
            ),
            KeyAction::Home
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                KeyContext::Input
            ),
            KeyAction::End
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL),
                KeyContext::Normal
            ),
            KeyAction::Char('e')
        );
    }

    /// Ctrl+G backs out of a prompt but must never quit the dashboard the way
    /// a bare Esc does from a list view.
    #[test]
    fn ctrl_g_escapes_input_but_is_inert_in_normal_mode() {
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
                KeyContext::Search
            ),
            KeyAction::Escape
        );
        assert_eq!(
            KeyAction::from_key(
                KeyEvent::new(KeyCode::Char('g'), KeyModifiers::CONTROL),
                KeyContext::Normal
            ),
            KeyAction::Char('g')
        );
    }

    /// Ctrl+P must not be swallowed by the Shift remap table on terminals that
    /// report Ctrl+Shift, and must not reach `OpenPr` the way a bare `p` does.
    #[test]
    fn ctrl_motion_survives_a_stray_shift_modifier() {
        let action = KeyAction::from_key(
            KeyEvent::new(
                KeyCode::Char('p'),
                KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            ),
            KeyContext::Normal,
        );
        assert_eq!(action, KeyAction::Up);
    }

    #[test]
    fn move_picker_mode_allows_shifted_letters_as_text() {
        for c in ['K', 'J', 'R'] {
            let action = KeyAction::from_key(
                KeyEvent::new(KeyCode::Char(c), KeyModifiers::SHIFT),
                KeyContext::MovePicker,
            );
            assert_eq!(
                action,
                KeyAction::Char(c),
                "Shift+'{}' should pass through",
                c
            );
        }
    }
}
