mod app;
mod ui;

use crate::tui::keys::{self, KeyScope};
use anyhow::Result;
use app::{SplitApp, SplitMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::time::Duration;

/// Run the split TUI
pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let result = SplitApp::new().and_then(|mut app| run_app(&mut terminal, &mut app));

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main event loop
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut SplitApp,
) -> Result<()> {
    loop {
        // Draw
        terminal.draw(|f| ui::render(f, app))?;

        // Handle events with timeout
        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            let key = keys::normalize(key, key_scope(&app.mode));
            handle_key(app, key)?;
        }

        if app.should_quit {
            break;
        }

        if app.execute_requested {
            app.execute_split()?;
            break;
        }
    }

    Ok(())
}

fn key_scope(mode: &SplitMode) -> KeyScope {
    match mode {
        SplitMode::Naming => KeyScope::TextInput,
        SplitMode::Normal | SplitMode::Confirm | SplitMode::Help => KeyScope::Navigation,
    }
}

/// Handle a key press
fn handle_key(app: &mut SplitApp, key: KeyEvent) -> Result<()> {
    match &app.mode {
        SplitMode::Normal => handle_normal_key(app, key.code, key.modifiers),
        SplitMode::Naming => handle_naming_key(app, key),
        SplitMode::Confirm => handle_confirm_key(app, key.code),
        SplitMode::Help => handle_help_key(app, key.code),
    }
}

fn handle_normal_key(app: &mut SplitApp, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('?') => app.mode = SplitMode::Help,
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Char('s') => {
            // Mark split point - enter naming mode
            if app.can_split_at_current() {
                app.input_buffer.clear();
                app.input_cursor = 0;
                app.mode = SplitMode::Naming;
            } else {
                app.status_message = Some("Cannot split here".to_string());
            }
        }
        KeyCode::Char('d') => {
            // Remove split point at current position
            app.remove_split_at_current();
        }
        KeyCode::Enter => {
            if !app.split_points.is_empty() {
                app.mode = SplitMode::Confirm;
            } else {
                app.status_message = Some("No split points defined".to_string());
            }
        }
        KeyCode::Char('K') if modifiers.contains(KeyModifiers::SHIFT) => {
            // Move split point up
            app.move_split_up();
        }
        KeyCode::Char('J') if modifiers.contains(KeyModifiers::SHIFT) => {
            // Move split point down
            app.move_split_down();
        }
        _ => {}
    }
    Ok(())
}

fn handle_naming_key(app: &mut SplitApp, key: KeyEvent) -> Result<()> {
    match key.code {
        KeyCode::Esc => {
            app.mode = SplitMode::Normal;
            app.input_buffer.clear();
        }
        KeyCode::Enter => {
            let name = app.input_buffer.trim().to_string();
            if name.is_empty() {
                app.status_message = Some("Branch name cannot be empty".to_string());
            } else if app.branch_name_exists(&name) {
                app.status_message = Some(format!("Branch '{}' already exists", name));
            } else {
                app.add_split_at_current(name);
                app.mode = SplitMode::Normal;
            }
            app.input_buffer.clear();
        }
        _ => {
            keys::edit(key, &mut app.input_buffer, &mut app.input_cursor);
        }
    }
    Ok(())
}

fn handle_confirm_key(app: &mut SplitApp, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.execute_requested = true;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = SplitMode::Normal;
        }
        _ => {}
    }
    Ok(())
}

fn handle_help_key(app: &mut SplitApp, _code: KeyCode) -> Result<()> {
    // Any key closes help
    app.mode = SplitMode::Normal;
    Ok(())
}
