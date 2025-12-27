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

impl From<KeyEvent> for KeyAction {
    fn from(key: KeyEvent) -> Self {
        // Handle Ctrl+C for quit
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c') = key.code {
                return KeyAction::Quit;
            }
        }

        // Handle Shift for uppercase
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            if let KeyCode::Char('R') | KeyCode::Char('r') = key.code {
                return KeyAction::RestackAll;
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

            // Vim navigation
            KeyCode::Char('k') => KeyAction::Up,
            KeyCode::Char('j') => KeyAction::Down,

            // Actions
            KeyCode::Char('r') => KeyAction::Restack,
            KeyCode::Char('s') => KeyAction::Submit,
            KeyCode::Char('p') => KeyAction::OpenPr,
            KeyCode::Char('n') => KeyAction::NewBranch,
            KeyCode::Char('d') => KeyAction::Delete,
            KeyCode::Char('e') => KeyAction::Rename,

            // Modes
            KeyCode::Char('/') => KeyAction::Search,
            KeyCode::Char('?') => KeyAction::Help,
            KeyCode::Char('q') => KeyAction::Quit,

            // Text input (for search mode)
            KeyCode::Char(c) => KeyAction::Char(c),
            KeyCode::Backspace => KeyAction::Backspace,

            _ => KeyAction::None,
        }
    }
}
