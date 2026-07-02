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

impl From<KeyEvent> for KeyAction {
    fn from(key: KeyEvent) -> Self {
        Self::from_key(key, KeyContext::Normal)
    }
}

impl KeyAction {
    pub fn from_key(key: KeyEvent, context: KeyContext) -> Self {
        // Handle Ctrl+C for quit
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c') = key.code {
                return KeyAction::Quit;
            }
        }

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
