mod app;
mod ui;

use anyhow::{Context, Result};
use app::{DashboardMode, PendingCommand, WorktreeApp};
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::git::GitRepo;

pub fn run() -> Result<()> {
    let mut status_message = None;
    let mut preferred_selection = None;

    loop {
        let outcome = run_once(status_message.take(), preferred_selection.take())?;
        match outcome {
            DashboardOutcome::Quit => return Ok(()),
            DashboardOutcome::Command(command) => {
                preferred_selection = selection_after_command(&command);
                status_message = execute_dashboard_command(&command)?;
            }
        }
    }
}

enum DashboardOutcome {
    Quit,
    Command(PendingCommand),
}

fn run_once(
    initial_status: Option<String>,
    preferred_selection: Option<String>,
) -> Result<DashboardOutcome> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = WorktreeApp::new(initial_status, preferred_selection)
        .and_then(|mut app| run_app(&mut terminal, &mut app));

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut WorktreeApp,
) -> Result<DashboardOutcome> {
    loop {
        app.refresh_background();
        terminal.draw(|f| ui::render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                handle_key(app, key.code, key.modifiers)?;
            }
        }
        app.refresh_background();

        if app.should_quit {
            if let Some(command) = app.pending_command.take() {
                return Ok(DashboardOutcome::Command(command));
            }
            return Ok(DashboardOutcome::Quit);
        }
    }
}

fn handle_key(app: &mut WorktreeApp, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match app.mode {
        DashboardMode::Normal => handle_normal_key(app, code, modifiers),
        DashboardMode::Help => {
            app.mode = DashboardMode::Normal;
            Ok(())
        }
        DashboardMode::CreateInput => handle_create_key(app, code),
        DashboardMode::ConfirmDelete => handle_delete_key(app, code),
    }
}

fn handle_normal_key(app: &mut WorktreeApp, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('?') => app.mode = DashboardMode::Help,
        KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
        KeyCode::Down | KeyCode::Char('j') => app.select_next(),
        KeyCode::Enter => app.request_go(),
        KeyCode::Char('c') => app.request_create(),
        KeyCode::Char('d') => app.request_delete(),
        KeyCode::Char('R') => app.request_restack(),
        KeyCode::Char('r') if modifiers.contains(KeyModifiers::SHIFT) => app.request_restack(),
        _ => {}
    }
    Ok(())
}

fn handle_create_key(app: &mut WorktreeApp, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc => {
            app.mode = DashboardMode::Normal;
            app.input_buffer.clear();
            app.input_cursor = 0;
        }
        KeyCode::Enter => app.confirm_create(),
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
        KeyCode::Home => app.input_cursor = 0,
        KeyCode::End => app.input_cursor = app.input_buffer.len(),
        KeyCode::Backspace => {
            if app.input_cursor > 0 {
                app.input_cursor -= 1;
                app.input_buffer.remove(app.input_cursor);
            }
        }
        KeyCode::Char(ch) => {
            app.input_buffer.insert(app.input_cursor, ch);
            app.input_cursor += 1;
        }
        _ => {}
    }
    Ok(())
}

fn handle_delete_key(app: &mut WorktreeApp, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.mode = DashboardMode::Normal;
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_delete(),
        _ => {}
    }
    Ok(())
}

fn selection_after_command(command: &PendingCommand) -> Option<String> {
    match command {
        PendingCommand::Go { name }
        | PendingCommand::Create { name: Some(name) }
        | PendingCommand::Remove { name } => Some(name.clone()),
        PendingCommand::Create { name: None } | PendingCommand::Restack => None,
    }
}

fn execute_dashboard_command(command: &PendingCommand) -> Result<Option<String>> {
    let repo = GitRepo::open()?;
    let exe = std::env::current_exe().context("Failed to locate current executable")?;
    let args = command.args();
    let workdir = repo.workdir()?;

    match command {
        PendingCommand::Go { .. } | PendingCommand::Create { .. } => {
            let status = Command::new(&exe)
                .args(&args)
                .current_dir(workdir)
                .status()
                .with_context(|| format!("Failed to run '{}'", args.join(" ")))?;

            if status.success() {
                Ok(None)
            } else {
                Ok(Some(format!("Command failed: {}", args.join(" "))))
            }
        }
        PendingCommand::Remove { name } => run_captured_command(&exe, workdir, &args)
            .map(|status| status.or_else(|| Some(format!("Removed '{}'", name)))),
        PendingCommand::Restack => run_captured_command(&exe, workdir, &args)
            .map(|status| status.or_else(|| Some("Restacked managed worktrees".to_string()))),
    }
}

fn run_captured_command(exe: &Path, cwd: &Path, args: &[String]) -> Result<Option<String>> {
    let output = Command::new(exe)
        .args(args)
        .current_dir(cwd)
        .output()
        .with_context(|| format!("Failed to run '{}'", args.join(" ")))?;

    if output.status.success() {
        return Ok(None);
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let message = stderr
        .lines()
        .chain(stdout.lines())
        .find(|line| !line.trim().is_empty())
        .unwrap_or("Command failed")
        .to_string();

    Ok(Some(message))
}
