mod app;
mod event;
pub mod split;
mod ui;
mod widgets;

use app::{App, ConfirmAction, FocusedPane, InputAction, Mode};
use event::{poll_event, KeyAction};

use crate::engine::BranchMetadata;
use crate::git::RebaseResult;
use crate::ops::receipt::{OpKind, PlanSummary};
use crate::ops::tx::{self, Transaction};
use anyhow::Result;
use crossterm::{
    event::Event,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::process::Command;
use std::time::Duration;

/// Run the TUI
pub fn run() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app state
    let result = App::new().and_then(|mut app| run_app(&mut terminal, &mut app));

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main event loop
fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
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
            let action = KeyAction::from(key);
            handle_action(app, action)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
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
    }
    Ok(())
}

/// Handle actions in normal mode
fn handle_normal_action(app: &mut App, action: KeyAction) -> Result<()> {
    match action {
        KeyAction::Tab => {
            app.focused_pane = match app.focused_pane {
                FocusedPane::Stack => FocusedPane::Diff,
                FocusedPane::Diff => FocusedPane::Stack,
            };
        }
        KeyAction::Up => {
            match app.focused_pane {
                FocusedPane::Stack => app.select_previous(),
                FocusedPane::Diff => {
                    if app.diff_scroll > 0 {
                        app.diff_scroll -= 1;
                    }
                }
            }
        }
        KeyAction::Down => {
            match app.focused_pane {
                FocusedPane::Stack => app.select_next(),
                FocusedPane::Diff => {
                    if app.diff_scroll < app.total_diff_lines().saturating_sub(1) {
                        app.diff_scroll += 1;
                    }
                }
            }
        }
        KeyAction::Enter => {
            if let Some(branch) = app.selected_branch() {
                if !branch.is_current {
                    let name = branch.name.clone();
                    checkout_branch(app, &name)?;
                }
            }
        }
        KeyAction::Quit | KeyAction::Escape => app.should_quit = true,
        KeyAction::Search => {
            app.mode = Mode::Search;
            app.search_query.clear();
            app.filtered_indices.clear();
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
            // Use --no-prompt since TUI can't handle interactive stdin
            run_external_command(app, &["submit", "--no-prompt"])?;
        }
        KeyAction::OpenPr => {
            if let Some(branch) = app.selected_branch() {
                if branch.pr_number.is_some() {
                    let name = branch.name.clone();
                    // Checkout the branch first if needed, then open PR
                    if !branch.is_current {
                        checkout_branch(app, &name)?;
                    }
                    run_external_command(app, &["pr"])?;
                } else {
                    app.set_status("No PR for this branch");
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
                    checkout_branch(app, &name)?;
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
fn handle_confirm_action(app: &mut App, action: KeyAction, confirm_action: &ConfirmAction) -> Result<()> {
    match action {
        KeyAction::Char('y') | KeyAction::Char('Y') => {
            match confirm_action {
                ConfirmAction::Delete(branch) => {
                    run_external_command(app, &["branch", "delete", branch, "--force"])?;
                }
                ConfirmAction::Restack(branch) => {
                    // Checkout branch first if not current
                    if app.current_branch != *branch {
                        checkout_branch(app, branch)?;
                    }
                    run_external_command(app, &["restack", "--quiet"])?;
                }
                ConfirmAction::RestackAll => {
                    run_external_command(app, &["restack", "--all", "--quiet"])?;
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
                        run_external_command(app, &["rename", "--literal", &input])?;
                    }
                    InputAction::NewBranch => {
                        run_external_command(app, &["create", &input])?;
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

/// Checkout a branch
fn checkout_branch(app: &mut App, branch: &str) -> Result<()> {
    app.repo.checkout(branch)?;
    app.current_branch = branch.to_string();
    app.needs_refresh = true;
    app.set_status(format!("Switched to '{}'", branch));
    Ok(())
}

/// Run an external stax command
fn run_external_command(app: &mut App, args: &[&str]) -> Result<()> {
    // Get the current exe path
    let exe = std::env::current_exe()?;
    let workdir = app.repo.workdir()?;

    let output = Command::new(&exe)
        .args(args)
        .current_dir(workdir)
        .output()?;

    if output.status.success() {
        app.needs_refresh = true;
        app.set_status(format!("✓ {} completed", args.join(" ")));
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        app.set_status(format!("✗ {}", stderr.lines().next().unwrap_or("Command failed")));
    }

    Ok(())
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

    let branch_word = if reparent_ops.len() == 1 { "branch" } else { "branches" };
    app.set_status(format!("Applying reorder ({} {})...", reparent_ops.len(), branch_word));

    // Collect all affected branches (those being reparented)
    let affected_branches: Vec<String> = reparent_ops.iter().map(|(b, _)| b.clone()).collect();

    // Begin single transaction for entire reorder operation
    let mut tx = Transaction::begin(OpKind::Reorder, &app.repo, true)?;
    tx.plan_branches(&app.repo, &affected_branches)?;
    let summary = PlanSummary {
        branches_to_rebase: affected_branches.len(),
        branches_to_push: 0,
        description: vec![format!("Reorder {} {}", affected_branches.len(), branch_word)],
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
        
        let merge_base = app.repo.merge_base(new_parent, branch).unwrap_or(parent_rev.clone());
        
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
        // Checkout and rebase
        if let Err(e) = app.repo.checkout(branch) {
            tx.finish_err(
                &format!("Failed to checkout {}: {}", branch, e),
                Some("restack"),
                Some(branch),
            )?;
            app.set_status(format!("✗ Failed to checkout {}", branch));
            return Ok(());
        }
        
        match app.repo.rebase(new_parent) {
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
                tx.finish_err(
                    "Rebase conflict",
                    Some("restack"),
                    Some(branch),
                )?;
                app.set_status(format!("✗ Conflict rebasing {} (stax undo to recover)", branch));
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
    
    app.set_status(format!("✓ Reordered {} {}", reparent_ops.len(), branch_word));
    
    Ok(())
}
