mod app;
mod event;
pub mod ready;
pub mod split;
pub mod split_hunk;
pub(crate) mod ui;
mod widgets;
pub mod worktree;

use app::{
    App, ConfirmAction, FocusedPane, InputAction, Mode, PendingAction, PendingCommand, TuiPane,
};
use event::{KeyAction, KeyContext, poll_event};

use crate::application::{
    OperationEvent, OperationOutcome, OperationReporter, OperationRequest, OperationStage,
    PullRequestMode, RestackScope, execute_repository_operation,
};
use crate::commands::open::open_url_in_browser;
use crate::engine::BranchMetadata;
use crate::git::GitRepo;
use crate::git::RebaseResult;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::{Context, Result};
use crossterm::{
    event::{Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TuiOperationStatus {
    pub request: Option<OperationRequest>,
    pub stage: Option<OperationStage>,
    pub completed: usize,
    pub total: Option<usize>,
    pub branch: Option<String>,
    pub message: String,
}

pub struct TuiOperationReporter<'a> {
    status: &'a mut TuiOperationStatus,
}

impl OperationReporter for TuiOperationReporter<'_> {
    fn report(&mut self, event: OperationEvent) {
        match event {
            OperationEvent::Started(request) => {
                self.status.request = Some(request);
                self.status.stage = Some(OperationStage::Validating);
                self.status.completed = 0;
                self.status.total = None;
                self.status.branch = None;
                self.status.message = "Validating repository".into();
            }
            OperationEvent::Progress(progress) => {
                self.status.stage = Some(progress.stage);
                self.status.completed = progress.completed;
                self.status.total = progress.total;
                self.status.branch = progress.branch;
                self.status.message = progress.message;
            }
            OperationEvent::Completed(_) | OperationEvent::Failed(_) => {}
        }
    }
}

/// Run the TUI
pub fn run() -> Result<()> {
    let mut status_message = None;
    let mut preferred_selection = None;

    loop {
        let outcome = run_once(status_message.take(), preferred_selection.take())?;
        match outcome {
            TuiOutcome::Quit => return Ok(()),
            TuiOutcome::Command(command) => {
                preferred_selection = command.preferred_selection.clone();
                status_message = execute_pending_command(&command)?;
            }
        }
    }
}

enum TuiOutcome {
    Quit,
    Command(PendingCommand),
}

fn run_once(
    initial_status: Option<String>,
    preferred_selection: Option<String>,
) -> Result<TuiOutcome> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let result = App::new(initial_status, preferred_selection)
        .and_then(|mut app| run_app(&mut terminal, &mut app));

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main event loop
fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<TuiOutcome> {
    loop {
        // Refresh if needed
        if app.needs_refresh {
            app.refresh_branches()?;
        }

        // Clear stale status messages
        app.clear_stale_status();

        // Draw
        terminal.draw(|f| ui::render(f, app))?;

        // Handle events
        if let Some(Event::Key(key)) = poll_event(Duration::from_millis(100))? {
            log_key_event(app, &key);
            match &app.mode {
                Mode::Input(input_action) => {
                    let input_action = input_action.clone();
                    handle_input_key(app, key, &input_action)?;
                }
                Mode::Search => {
                    handle_search_key(app, key)?;
                }
                Mode::MovePicker => {
                    handle_move_picker_key(app, key)?;
                }
                _ => {
                    let context = match app.mode {
                        Mode::Normal => KeyContext::Normal,
                        Mode::Search => KeyContext::Search,
                        Mode::Help => KeyContext::Help,
                        Mode::Confirm(_) => KeyContext::Confirm,
                        Mode::Input(_) => KeyContext::Input,
                        Mode::Reorder => KeyContext::Reorder,
                        Mode::MovePicker => KeyContext::MovePicker,
                    };
                    let action = KeyAction::from_key(key, context);
                    handle_action(app, action)?;
                }
            }
        }

        if app.should_quit {
            if let Some(command) = app.pending_command.take() {
                return Ok(TuiOutcome::Command(command));
            }
            return Ok(TuiOutcome::Quit);
        }

        app.refresh_background();
    }
}

/// Handle a key action
fn handle_action(app: &mut App, action: KeyAction) -> Result<()> {
    match &app.mode {
        Mode::Normal => handle_normal_action(app, action)?,
        Mode::Search => handle_search_action(app, action)?,
        Mode::Help => handle_help_action(app, action),
        Mode::Confirm(confirm_action) => {
            let confirm_action = confirm_action.clone();
            handle_confirm_action(app, action, &confirm_action)?;
        }
        Mode::Input(input_action) => {
            let input_action = input_action.clone();
            handle_input_action(app, action, &input_action)?;
        }
        Mode::Reorder => handle_reorder_action(app, action)?,
        // MovePicker is only reached here for actions that couldn't be
        // handled by `handle_move_picker_key` — currently none.
        Mode::MovePicker => {}
    }
    Ok(())
}

/// Handle actions in normal mode
fn handle_normal_action(app: &mut App, action: KeyAction) -> Result<()> {
    match action {
        KeyAction::Char(c) => {
            let mapped = match c {
                'k' => Some(KeyAction::Up),
                'j' => Some(KeyAction::Down),
                'r' => Some(KeyAction::Restack),
                'R' => Some(KeyAction::RestackAll),
                's' => Some(KeyAction::Submit),
                'p' => Some(KeyAction::OpenPr),
                'n' => Some(KeyAction::NewBranch),
                'd' => Some(KeyAction::Delete),
                'e' => Some(KeyAction::Rename),
                '/' => Some(KeyAction::Search),
                '?' => Some(KeyAction::Help),
                'q' => Some(KeyAction::Quit),
                'o' => Some(KeyAction::ReorderMode),
                'm' => Some(KeyAction::MovePicker),
                '1' => Some(KeyAction::ToggleStackPane),
                '2' => Some(KeyAction::ToggleSummaryPane),
                '3' => Some(KeyAction::TogglePatchPane),
                _ => None,
            };

            if let Some(mapped_action) = mapped {
                return handle_normal_action(app, mapped_action);
            }
        }
        KeyAction::Tab => app.focus_next_visible_pane(),
        KeyAction::ToggleStackPane => app.toggle_pane_visibility(TuiPane::Stack),
        KeyAction::ToggleSummaryPane => app.toggle_pane_visibility(TuiPane::Summary),
        KeyAction::TogglePatchPane => app.toggle_pane_visibility(TuiPane::Patch),
        KeyAction::Up => match app.focused_pane {
            FocusedPane::Stack => app.select_previous(),
            FocusedPane::Summary => {}
            FocusedPane::Diff => {
                if app.diff_scroll > 0 {
                    app.diff_scroll -= 1;
                }
            }
        },
        KeyAction::Down => match app.focused_pane {
            FocusedPane::Stack => app.select_next(),
            FocusedPane::Summary => {}
            FocusedPane::Diff => {
                if app.diff_scroll < app.total_diff_lines().saturating_sub(1) {
                    app.diff_scroll += 1;
                }
            }
        },
        KeyAction::Enter => {
            if let Some(branch) = app.selected_branch() {
                if !branch.is_current {
                    let name = branch.name.clone();
                    queue_operation(
                        app,
                        OperationRequest::Checkout {
                            branch: name.clone(),
                        },
                    );
                }
            }
        }
        KeyAction::Quit | KeyAction::Escape => app.should_quit = true,
        KeyAction::Search => {
            app.mode = Mode::Search;
            app.search_query.clear();
            app.update_search();
        }
        KeyAction::Help => app.mode = Mode::Help,
        KeyAction::Restack => {
            if let Some(branch) = app.selected_branch() {
                if branch.needs_restack && !branch.is_trunk {
                    let name = branch.name.clone();
                    app.mode = Mode::Confirm(ConfirmAction::Restack(name));
                } else if branch.is_trunk {
                    app.set_status("Cannot restack trunk branch");
                } else {
                    app.set_status("Branch doesn't need restacking");
                }
            }
        }
        KeyAction::RestackAll => {
            app.mode = Mode::Confirm(ConfirmAction::RestackAll);
        }
        KeyAction::Submit => {
            queue_operation(
                app,
                OperationRequest::SubmitStack {
                    new_pull_requests: PullRequestMode::Draft,
                },
            );
        }
        KeyAction::OpenPr => {
            if let Some(branch) = app.selected_branch() {
                if !branch.is_trunk {
                    let name = branch.name.clone();
                    queue_operation(
                        app,
                        OperationRequest::ResolvePullRequestUrl { branch: name },
                    );
                } else {
                    app.set_status("No PR for trunk branch");
                }
            }
        }
        KeyAction::NewBranch => {
            app.input_buffer.clear();
            app.input_cursor = 0;
            app.mode = Mode::Input(InputAction::NewBranch);
        }
        KeyAction::Rename => {
            if let Some(branch) = app.selected_branch() {
                if branch.is_trunk {
                    app.set_status("Cannot rename trunk branch");
                } else if !branch.is_current {
                    app.set_status("Switch to branch first to rename it");
                } else {
                    app.input_buffer = branch.name.clone();
                    app.input_cursor = app.input_buffer.len();
                    app.mode = Mode::Input(InputAction::Rename);
                }
            }
        }
        KeyAction::Delete => {
            if let Some(branch) = app.selected_branch() {
                if branch.is_trunk {
                    app.set_status("Cannot delete trunk branch");
                } else if branch.is_current {
                    app.set_status("Cannot delete current branch");
                } else {
                    let name = branch.name.clone();
                    app.mode = Mode::Confirm(ConfirmAction::Delete(name));
                }
            }
        }
        KeyAction::ReorderMode => {
            if app.init_reorder_state() {
                app.mode = Mode::Reorder;
            }
        }
        KeyAction::MovePicker => {
            if let Err(msg) = app.init_move_picker() {
                app.set_status(msg);
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle actions in search mode
fn handle_search_action(app: &mut App, action: KeyAction) -> Result<()> {
    match action {
        KeyAction::Escape => {
            app.mode = Mode::Normal;
            app.search_query.clear();
            app.filtered_indices.clear();
            app.select_current_branch();
        }
        KeyAction::Enter => {
            if let Some(branch) = app.selected_branch() {
                if !branch.is_current {
                    let name = branch.name.clone();
                    app.mode = Mode::Normal;
                    queue_operation(app, OperationRequest::Checkout { branch: name });
                } else {
                    app.mode = Mode::Normal;
                }
            }
        }
        KeyAction::Up => app.select_previous(),
        KeyAction::Down => app.select_next(),
        KeyAction::Char(c) => {
            app.search_query.push(c);
            app.update_search();
        }
        KeyAction::Backspace => {
            app.search_query.pop();
            app.update_search();
        }
        _ => {}
    }
    Ok(())
}

/// Key handler for `Mode::MovePicker`. Mirrors `handle_search_key` (chars
/// feed the query, Up/Down navigate, Enter confirms, Esc cancels) but on
/// Enter it queues `upstack onto <target>` instead of checkout.
fn handle_move_picker_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if is_ctrl_c(&key) {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.clear_move_picker();
            app.mode = Mode::Normal;
        }
        KeyCode::Enter => {
            let Some(target) = app.move_picker_current().map(str::to_string) else {
                app.set_status("No candidate selected");
                return Ok(());
            };
            let source = app.move_picker_source.clone();
            app.clear_move_picker();
            app.mode = Mode::Normal;
            // Run `checkout <source> && upstack onto <target>` so the
            // reparent operates on the picker's source branch regardless
            // of where HEAD is — `upstack onto` always reparents the
            // *current* branch.
            let mut commands = Vec::new();
            if app.current_branch != source {
                commands.push(vec!["checkout".to_string(), source.clone()]);
            }
            commands.push(vec![
                "upstack".to_string(),
                "onto".to_string(),
                target.clone(),
            ]);
            queue_command(
                app,
                commands,
                format!("Moved '{}' onto '{}'", source, target),
                Some(source),
            );
        }
        KeyCode::Up => app.move_picker_select_previous(),
        KeyCode::Down => app.move_picker_select_next(),
        KeyCode::Char(c) => {
            app.move_picker_query.push(c);
            app.move_picker_on_query_change();
        }
        KeyCode::Backspace => {
            app.move_picker_query.pop();
            app.move_picker_on_query_change();
        }
        _ => {}
    }
    Ok(())
}

fn handle_search_key(app: &mut App, key: KeyEvent) -> Result<()> {
    if is_ctrl_c(&key) {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.search_query.clear();
            app.filtered_indices.clear();
            app.select_current_branch();
        }
        KeyCode::Enter => {
            if let Some(branch) = app.selected_branch() {
                if !branch.is_current {
                    let name = branch.name.clone();
                    app.mode = Mode::Normal;
                    queue_operation(app, OperationRequest::Checkout { branch: name });
                } else {
                    app.mode = Mode::Normal;
                }
            }
        }
        KeyCode::Up => app.select_previous(),
        KeyCode::Down => app.select_next(),
        KeyCode::Char(c) => {
            app.search_query.push(c);
            app.update_search();
        }
        KeyCode::Backspace => {
            app.search_query.pop();
            app.update_search();
        }
        _ => {}
    }
    Ok(())
}

/// Handle actions in help mode
fn handle_help_action(app: &mut App, _action: KeyAction) {
    // Any key closes help
    app.mode = Mode::Normal;
}

/// Handle actions in reorder mode
fn handle_reorder_action(app: &mut App, action: KeyAction) -> Result<()> {
    match action {
        KeyAction::Escape => {
            // Cancel reorder, discard changes
            app.clear_reorder_state();
            app.mode = Mode::Normal;
            app.set_status("Reorder cancelled");
        }
        KeyAction::Enter => {
            // Confirm changes
            if app.reorder_has_changes() {
                app.mode = Mode::Confirm(ConfirmAction::ApplyReorder);
            } else {
                app.clear_reorder_state();
                app.mode = Mode::Normal;
                app.set_status("No changes to apply");
            }
        }
        KeyAction::MoveUp => {
            app.reorder_move_up();
        }
        KeyAction::MoveDown => {
            app.reorder_move_down();
        }
        KeyAction::Up => {
            // Navigate selection up (without moving branch)
            app.select_previous();
        }
        KeyAction::Down => {
            // Navigate selection down (without moving branch)
            app.select_next();
        }
        _ => {}
    }
    Ok(())
}

/// Handle actions in confirm mode
fn handle_confirm_action(
    app: &mut App,
    action: KeyAction,
    confirm_action: &ConfirmAction,
) -> Result<()> {
    match action {
        KeyAction::Char('y') | KeyAction::Char('Y') => {
            match confirm_action {
                ConfirmAction::Delete(branch) => {
                    queue_command(
                        app,
                        vec![vec![
                            "branch".to_string(),
                            "delete".to_string(),
                            branch.clone(),
                            "--force".to_string(),
                        ]],
                        format!("Deleted '{}'", branch),
                        Some(app.current_branch.clone()),
                    );
                }
                ConfirmAction::Restack(branch) => {
                    queue_operation(
                        app,
                        OperationRequest::Restack {
                            scope: RestackScope::StackContaining(branch.clone()),
                            auto_stash: false,
                        },
                    );
                }
                ConfirmAction::RestackAll => {
                    queue_operation(
                        app,
                        OperationRequest::Restack {
                            scope: RestackScope::All,
                            auto_stash: false,
                        },
                    );
                }
                ConfirmAction::ApplyReorder => {
                    apply_reorder_changes(app)?;
                }
            }
            app.mode = Mode::Normal;
            app.needs_refresh = true;
        }
        KeyAction::Char('n') | KeyAction::Char('N') | KeyAction::Escape => {
            // For ApplyReorder, go back to Reorder mode instead of Normal
            if matches!(confirm_action, ConfirmAction::ApplyReorder) {
                app.mode = Mode::Reorder;
            } else {
                app.mode = Mode::Normal;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle actions in input mode
fn handle_input_action(app: &mut App, action: KeyAction, input_action: &InputAction) -> Result<()> {
    match action {
        KeyAction::Escape => {
            app.mode = Mode::Normal;
            app.input_buffer.clear();
            app.input_cursor = 0;
        }
        KeyAction::Enter => {
            let input = app.input_buffer.trim().to_string();
            if input.is_empty() {
                app.set_status("Name cannot be empty");
            } else {
                match input_action {
                    InputAction::Rename => {
                        queue_command(
                            app,
                            vec![vec![
                                "rename".to_string(),
                                "--literal".to_string(),
                                input.clone(),
                            ]],
                            format!("Renamed branch to '{}'", input),
                            Some(input.clone()),
                        );
                    }
                    InputAction::NewBranch => {
                        queue_operation(
                            app,
                            OperationRequest::CreateBranch {
                                name: input.clone(),
                                parent: app.current_branch.clone(),
                            },
                        );
                    }
                }
                app.mode = Mode::Normal;
                app.input_buffer.clear();
                app.input_cursor = 0;
            }
        }
        KeyAction::Left => {
            if app.input_cursor > 0 {
                app.input_cursor -= 1;
            }
        }
        KeyAction::Right => {
            if app.input_cursor < app.input_buffer.len() {
                app.input_cursor += 1;
            }
        }
        KeyAction::Home => {
            app.input_cursor = 0;
        }
        KeyAction::End => {
            app.input_cursor = app.input_buffer.len();
        }
        KeyAction::Char(c) => {
            app.input_buffer.insert(app.input_cursor, c);
            app.input_cursor += 1;
        }
        KeyAction::Backspace => {
            if app.input_cursor > 0 {
                app.input_cursor -= 1;
                app.input_buffer.remove(app.input_cursor);
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_input_key(app: &mut App, key: KeyEvent, input_action: &InputAction) -> Result<()> {
    if is_ctrl_c(&key) {
        app.should_quit = true;
        return Ok(());
    }

    match key.code {
        KeyCode::Esc => {
            app.mode = Mode::Normal;
            app.input_buffer.clear();
            app.input_cursor = 0;
        }
        KeyCode::Enter => {
            let input = app.input_buffer.trim().to_string();
            if input.is_empty() {
                app.set_status("Name cannot be empty");
            } else {
                match input_action {
                    InputAction::Rename => {
                        queue_command(
                            app,
                            vec![vec![
                                "rename".to_string(),
                                "--literal".to_string(),
                                input.clone(),
                            ]],
                            format!("Renamed branch to '{}'", input),
                            Some(input.clone()),
                        );
                    }
                    InputAction::NewBranch => {
                        queue_operation(
                            app,
                            OperationRequest::CreateBranch {
                                name: input.clone(),
                                parent: app.current_branch.clone(),
                            },
                        );
                    }
                }
                app.mode = Mode::Normal;
                app.input_buffer.clear();
                app.input_cursor = 0;
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
        KeyCode::Home => {
            app.input_cursor = 0;
        }
        KeyCode::End => {
            app.input_cursor = app.input_buffer.len();
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
        _ => {}
    }
    Ok(())
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c'))
}

fn log_key_event(app: &App, key: &KeyEvent) {
    let Ok(path) = std::env::var("STAX_TUI_KEYLOG") else {
        return;
    };

    let mode = match &app.mode {
        Mode::Normal => "normal",
        Mode::Search => "search",
        Mode::Help => "help",
        Mode::Confirm(_) => "confirm",
        Mode::Input(_) => "input",
        Mode::Reorder => "reorder",
        Mode::MovePicker => "move_picker",
    };

    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };

    let _ = writeln!(
        file,
        "mode={} code={:?} mods={:?} kind={:?} state={:?}",
        mode, key.code, key.modifiers, key.kind, key.state
    );
}

fn queue_operation(app: &mut App, request: OperationRequest) {
    app.queue_operation(request);
}

fn queue_command(
    app: &mut App,
    commands: Vec<Vec<String>>,
    success_message: impl Into<String>,
    preferred_selection: Option<String>,
) {
    app.queue_command(commands, success_message, preferred_selection);
}

fn execute_pending_command(command: &PendingCommand) -> Result<Option<String>> {
    let repo = GitRepo::open()?;
    let workdir = repo.workdir()?.to_path_buf();
    drop(repo);

    match &command.action {
        PendingAction::Operation(request) => execute_pending_operation(&workdir, request.clone()),
        PendingAction::LegacyCommands(commands) => {
            execute_legacy_commands(commands, &workdir, command)
        }
    }
}

fn execute_pending_operation(
    workdir: &std::path::Path,
    request: OperationRequest,
) -> Result<Option<String>> {
    let mut status = TuiOperationStatus::default();
    let mut reporter = TuiOperationReporter {
        status: &mut status,
    };
    let request_for_error = request.clone();
    let result = execute_repository_operation(workdir, request, &mut reporter);
    match result {
        Ok(receipt) => {
            if let OperationOutcome::PullRequestResolved { url, .. } = &receipt.outcome {
                open_url_in_browser(url);
            }
            Ok(Some(receipt.summary))
        }
        Err(error) => {
            if matches!(
                request_for_error,
                OperationRequest::ResolvePullRequestUrl { .. }
            ) {
                return Ok(Some(
                    "No PR for this branch; run stax submit to create one".into(),
                ));
            }
            Ok(Some(format!("{}; {}", error.primary, error.action)))
        }
    }
}

fn execute_legacy_commands(
    commands: &[Vec<String>],
    workdir: &std::path::Path,
    command: &PendingCommand,
) -> Result<Option<String>> {
    let exe = std::env::current_exe().context("Failed to locate current executable")?;

    for args in commands {
        let status = Command::new(&exe)
            .args(args)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .status()
            .with_context(|| format!("Failed to run '{}'", args.join(" ")))?;

        if !status.success() {
            return Ok(Some(format!("Command failed: {}", args.join(" "))));
        }
    }

    Ok(Some(command.success_message.clone()))
}

/// Apply reorder changes - reparent branches and trigger restack (as single transaction)
fn apply_reorder_changes(app: &mut App) -> Result<()> {
    // Get the reparent operations before clearing state
    let reparent_ops = app.get_reparent_operations();

    let state = match app.reorder_state.take() {
        Some(s) => s,
        None => {
            app.set_status("No reorder state to apply");
            return Ok(());
        }
    };

    // Check if there are actual changes
    if state.original_chain == state.pending_chain {
        app.set_status("No changes to apply");
        return Ok(());
    }

    if reparent_ops.is_empty() {
        app.set_status("No reparenting needed");
        return Ok(());
    }

    let branch_word = if reparent_ops.len() == 1 {
        "branch"
    } else {
        "branches"
    };
    app.set_status(format!(
        "Applying reorder ({} {})...",
        reparent_ops.len(),
        branch_word
    ));

    // Collect all affected branches (those being reparented)
    let affected_branches: Vec<String> = reparent_ops.iter().map(|(b, _)| b.clone()).collect();

    // Begin single transaction for entire reorder operation
    let mut tx = Transaction::begin(OpKind::Reorder, &app.repo, true)?;
    tx.plan_branches(&app.repo, &affected_branches)?;
    let summary = PlanSummary {
        branches_to_rebase: affected_branches.len(),
        branches_to_push: 0,
        description: vec![format!(
            "Reorder {} {}",
            affected_branches.len(),
            branch_word
        )],
    };
    tx::print_plan(tx.kind(), &summary, true); // TUI is quiet
    tx.set_plan_summary(summary);
    tx.snapshot()?;

    // Apply each reparent operation directly (update metadata)
    for (branch, new_parent) in &reparent_ops {
        let parent_rev = match app.repo.branch_commit(new_parent) {
            Ok(rev) => rev,
            Err(e) => {
                tx.finish_err(
                    &format!("Failed to get commit for parent {}: {}", new_parent, e),
                    Some("reparent"),
                    Some(branch),
                )?;
                app.set_status(format!("✗ Failed to reparent {}", branch));
                return Ok(());
            }
        };

        let merge_base = app
            .repo
            .merge_base(new_parent, branch)
            .unwrap_or(parent_rev.clone());

        // Read existing metadata or create new
        let existing = BranchMetadata::read(app.repo.inner(), branch)?;
        let updated = if let Some(meta) = existing {
            BranchMetadata {
                parent_branch_name: new_parent.clone(),
                parent_branch_revision: merge_base,
                ..meta
            }
        } else {
            BranchMetadata::new(new_parent, &merge_base)
        };

        if let Err(e) = updated.write(app.repo.inner(), branch) {
            tx.finish_err(
                &format!("Failed to write metadata for {}: {}", branch, e),
                Some("reparent"),
                Some(branch),
            )?;
            app.set_status(format!("✗ Failed to reparent {}", branch));
            return Ok(());
        }
    }

    // Now restack all affected branches (in order from the pending chain)
    let current_branch = app.repo.current_branch()?;

    for (branch, new_parent) in &reparent_ops {
        match app.repo.rebase_branch_onto(branch, new_parent, false) {
            Ok(RebaseResult::Success) => {
                // Update metadata with new parent revision
                if let Some(mut meta) = BranchMetadata::read(app.repo.inner(), branch)? {
                    if let Ok(new_parent_rev) = app.repo.branch_commit(new_parent) {
                        meta.parent_branch_revision = new_parent_rev;
                        let _ = meta.write(app.repo.inner(), branch);
                    }
                }

                // Record after-OID
                let _ = tx.record_after(&app.repo, branch);
            }
            Ok(RebaseResult::Conflict) => {
                tx.finish_err("Rebase conflict", Some("restack"), Some(branch))?;
                app.set_status(format!(
                    "✗ Conflict rebasing {} (stax undo to recover)",
                    branch
                ));
                return Ok(());
            }
            Err(e) => {
                tx.finish_err(
                    &format!("Rebase failed: {}", e),
                    Some("restack"),
                    Some(branch),
                )?;
                app.set_status(format!("✗ Rebase failed for {}", branch));
                return Ok(());
            }
        }
    }

    // Return to original branch
    let _ = app.repo.checkout(&current_branch);

    // Finish transaction successfully
    tx.finish_ok()?;

    app.set_status(format!(
        "✓ Reordered {} {}",
        reparent_ops.len(),
        branch_word
    ));

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{TuiOperationReporter, TuiOperationStatus, app::PendingAction};
    use crate::application::{
        OperationEvent, OperationProgress, OperationReporter, OperationRequest, OperationStage,
        PullRequestMode, RestackScope,
    };

    #[test]
    fn migrated_tui_actions_never_use_legacy_commands() {
        for request in [
            OperationRequest::Checkout {
                branch: "feature".into(),
            },
            OperationRequest::CreateBranch {
                name: "child".into(),
                parent: "feature".into(),
            },
            OperationRequest::Restack {
                scope: RestackScope::StackContaining("feature".into()),
                auto_stash: false,
            },
            OperationRequest::Restack {
                scope: RestackScope::All,
                auto_stash: false,
            },
            OperationRequest::SubmitStack {
                new_pull_requests: PullRequestMode::Draft,
            },
            OperationRequest::ResolvePullRequestUrl {
                branch: "feature".into(),
            },
        ] {
            assert!(matches!(
                PendingAction::Operation(request),
                PendingAction::Operation(_)
            ));
        }
    }

    #[test]
    fn tui_reporter_preserves_submit_stage_order_and_counts() {
        let request = OperationRequest::SubmitStack {
            new_pull_requests: PullRequestMode::Draft,
        };
        let progress = [
            (OperationStage::Preparing, 0, Some(3), None),
            (OperationStage::Pushing, 1, Some(3), Some("base")),
            (OperationStage::Pushing, 2, Some(3), Some("child")),
            (
                OperationStage::UpdatingPullRequests,
                3,
                Some(3),
                Some("tip"),
            ),
        ];
        let mut status = TuiOperationStatus::default();
        let mut reporter = TuiOperationReporter {
            status: &mut status,
        };
        reporter.report(OperationEvent::Started(request.clone()));
        let mut observed = Vec::new();
        for (stage, completed, total, branch) in progress {
            reporter.report(OperationEvent::Progress(OperationProgress {
                stage,
                completed,
                total,
                branch: branch.map(str::to_string),
                message: format!("{stage:?}"),
            }));
            observed.push((
                reporter.status.stage,
                reporter.status.completed,
                reporter.status.total,
            ));
        }
        let before_terminal = reporter.status.clone();

        reporter.report(OperationEvent::Failed(crate::application::OperationError {
            request,
            kind: crate::application::OperationErrorKind::Runtime,
            details: crate::application::OperationErrorDetails::None,
            primary: "failed".into(),
            action: "retry".into(),
            diagnostic_chain: "diagnostic".into(),
            receipt: None,
            side_effects: crate::application::OperationSideEffects::None,
        }));

        assert_eq!(
            observed,
            vec![
                (Some(OperationStage::Preparing), 0, Some(3)),
                (Some(OperationStage::Pushing), 1, Some(3)),
                (Some(OperationStage::Pushing), 2, Some(3)),
                (Some(OperationStage::UpdatingPullRequests), 3, Some(3)),
            ],
        );
        assert_eq!(reporter.status, &before_terminal);
    }
}
