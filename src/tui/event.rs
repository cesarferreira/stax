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

    // Search input
    Char(char),
    Backspace,

    // Unknown
    None,
}

impl From<KeyEvent> for KeyAction {
    fn from(key: KeyEvent) -> Self {
        match key.code {
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => KeyAction::Up,
            KeyCode::Down | KeyCode::Char('j') => KeyAction::Down,
            KeyCode::Enter => KeyAction::Enter,
            KeyCode::Esc => KeyAction::Escape,

            // Actions (only in normal mode - checked by caller)
            KeyCode::Char('r') if key.modifiers.is_empty() => KeyAction::Restack,
            KeyCode::Char('R') => KeyAction::RestackAll,
            KeyCode::Char('s') => KeyAction::Submit,
            KeyCode::Char('p') => KeyAction::OpenPr,
            KeyCode::Char('n') => KeyAction::NewBranch,
            KeyCode::Char('d') => KeyAction::Delete,
            KeyCode::Char('e') => KeyAction::Rename,

            // Modes
            KeyCode::Char('/') => KeyAction::Search,
            KeyCode::Char('?') => KeyAction::Help,
            KeyCode::Char('q') => KeyAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => KeyAction::Quit,

            // Text input
            KeyCode::Char(c) => KeyAction::Char(c),
            KeyCode::Backspace => KeyAction::Backspace,

            _ => KeyAction::None,
        }
    }
}
