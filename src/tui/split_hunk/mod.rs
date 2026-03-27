mod app;
mod diff_parser;
mod ui;

use anyhow::Result;
use app::{HunkSplitApp, HunkSplitMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::time::Duration;

/// Run the hunk-based split TUI
pub fn run() -> Result<()> {
    let mut app = HunkSplitApp::new()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match result {
        Ok(true) => {
            app.finalize()?;
            println!("Split complete! Created {} branches.", app.round);
            println!("Use `stax ls` to see the new stack structure.");
            Ok(())
        }
        Ok(false) => {
            app.rollback();
            println!("Split aborted.");
            Ok(())
        }
        Err(e) => {
            app.rollback();
            Err(e)
        }
    }
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut HunkSplitApp,
) -> Result<bool> {
    loop {
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && matches!(key.code, KeyCode::Char('c'))
                {
                    return Ok(false);
                }

                match &app.mode {
                    HunkSplitMode::List => handle_list_key(app, key.code),
                    HunkSplitMode::Sequential => handle_sequential_key(app, key.code),
                    HunkSplitMode::Naming => handle_naming_key(app, key.code),
                    HunkSplitMode::ConfirmAbort => handle_confirm_abort_key(app, key.code),
                    HunkSplitMode::Help => handle_help_key(app, key.code),
                }
            }
        }

        if app.should_quit {
            return Ok(false);
        }

        if app.round_complete {
            app.round_complete = false;
            let branch_name = app.input_buffer.trim().to_string();
            app.input_buffer.clear();
            app.input_cursor = 0;
            let has_remaining = app.commit_round(&branch_name)?;
            if has_remaining {
                app.mode = HunkSplitMode::List;
                app.status_message = Some(format!(
                    "Committed '{}'. Select hunks for round {}.",
                    branch_name, app.round
                ));
            } else {
                app.all_done = true;
            }
        }

        if app.all_done {
            return Ok(true);
        }
    }
}

fn try_finish_round(app: &mut HunkSplitApp) {
    if app.selected_count() > 0 {
        app.mode = HunkSplitMode::Naming;
        app.input_buffer = app.suggest_branch_name();
        app.input_cursor = app.input_buffer.len();
    } else {
        app.status_message = Some("No hunks selected".to_string());
    }
}

fn handle_list_key(app: &mut HunkSplitApp, code: KeyCode) {
    match code {
        KeyCode::Down | KeyCode::Char('j') => app.move_cursor_down(),
        KeyCode::Up | KeyCode::Char('k') => app.move_cursor_up(),
        KeyCode::Char(' ') => app.toggle_current(),
        KeyCode::Char('a') => app.toggle_file(),
        KeyCode::Char('u') => app.undo(),
        KeyCode::Tab => {
            app.mode = HunkSplitMode::Sequential;
            app.status_message = Some("Sequential mode: y/n to accept/skip hunks".to_string());
        }
        KeyCode::Enter => try_finish_round(app),
        KeyCode::Char('q') | KeyCode::Esc => app.mode = HunkSplitMode::ConfirmAbort,
        KeyCode::Char('?') => app.mode = HunkSplitMode::Help,
        _ => {}
    }
}

fn handle_sequential_key(app: &mut HunkSplitApp, code: KeyCode) {
    match code {
        KeyCode::Char('y') => app.accept_and_advance(),
        KeyCode::Char('n') => app.skip_and_advance(),
        KeyCode::Char('a') => {
            app.toggle_file();
            app.advance_past_current_file();
        }
        KeyCode::Char('u') => app.undo(),
        KeyCode::Tab => {
            app.mode = HunkSplitMode::List;
            app.status_message = Some("List mode".to_string());
        }
        KeyCode::Enter => try_finish_round(app),
        KeyCode::Char('q') | KeyCode::Esc => app.mode = HunkSplitMode::ConfirmAbort,
        KeyCode::Char('?') => app.mode = HunkSplitMode::Help,
        _ => {}
    }
}

fn handle_naming_key(app: &mut HunkSplitApp, code: KeyCode) {
    match code {
        KeyCode::Enter => {
            let name = app.input_buffer.trim().to_string();
            match app.validate_branch_name(&name) {
                Ok(()) => {
                    app.round_complete = true;
                }
                Err(msg) => {
                    app.status_message = Some(msg);
                }
            }
        }
        KeyCode::Esc => {
            app.mode = HunkSplitMode::List;
            app.input_buffer.clear();
            app.input_cursor = 0;
        }
        KeyCode::Char(c) => {
            app.input_buffer.insert(app.input_cursor, c);
            app.input_cursor += 1;
        }
        KeyCode::Backspace => {
            if app.input_cursor > 0 {
                app.input_cursor -= 1;
                app.input_buffer.remove(app.input_cursor);
            }
        }
        KeyCode::Left => {
            if app.input_cursor > 0 {
                app.input_cursor -= 1;
            }
        }
        KeyCode::Right => {
            if app.input_cursor < app.input_buffer.len() {
                app.input_cursor += 1;
            }
        }
        _ => {}
    }
}

fn handle_confirm_abort_key(app: &mut HunkSplitApp, code: KeyCode) {
    match code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            app.should_quit = true;
        }
        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
            app.mode = HunkSplitMode::List;
        }
        _ => {}
    }
}

fn handle_help_key(app: &mut HunkSplitApp, _code: KeyCode) {
    app.mode = HunkSplitMode::List;
}
