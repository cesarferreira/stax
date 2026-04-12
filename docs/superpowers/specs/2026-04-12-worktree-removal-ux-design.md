# Worktree Removal UX Improvements

**Date:** 2026-04-12
**Status:** Approved
**Author:** Claude Sonnet 4.5

## Problem Statement

The current worktree TUI (`st wt ls`) has two UX issues when removing worktrees:

1. **No force confirmation for dirty worktrees**: When a user tries to remove a worktree with uncommitted changes, the TUI shows a warning badge ("dirty") but the deletion modal doesn't highlight this risk or provide a second confirmation step. The CLI version (`st wt rm`) correctly shows a warning and asks "Remove anyway?" - the TUI should match this behavior.

2. **TUI exits during removal**: When the user confirms deletion, the TUI exits to the shell to run the removal command, then re-enters with a status message. This breaks the flow and makes the operation feel less integrated. The TUI should stay active and show real-time progress updates during the removal.

## Goals

1. Add two-stage confirmation for removing worktrees with uncommitted changes
2. Keep the TUI active during removal with live status updates
3. Follow existing patterns in the codebase (loader infrastructure, background threads, mpsc channels)
4. Maintain backwards compatibility with CLI removal behavior

## Non-Goals

- Changing the CLI removal behavior
- Building a generic background operation framework (future work)
- Adding undo/rollback for worktree removal
- Changing other TUI operations (restack, create, etc.) - though this pattern can be reused later

## Architecture Overview

The implementation extends the existing TUI background loading pattern to support background removal operations.

### New Components

1. **`RemovalUpdate` enum** - Similar to `LoaderUpdate`, carries progress messages from background thread
2. **`DashboardMode::ConfirmForceDelete` state** - Second confirmation modal for dirty worktrees
3. **`removal_operation` field in `WorktreeApp`** - Holds the removal channel receiver
4. **`spawn_removal_operation()` function** - Spawns background thread for removal

### Modified Components

1. **`WorktreeApp::request_delete()`** - Checks if worktree is dirty before showing modal
2. **`WorktreeApp::refresh_background()`** - Polls removal channel in addition to loader channel
3. **`ui::render_delete_modal()`** - Renders different modals based on confirmation stage
4. **`mod.rs::execute_dashboard_command()`** - No longer handles Remove commands (removal stays in TUI)

### High-Level Flow

```
User presses 'd'
  ↓
App checks if worktree is dirty
  ↓
Show appropriate confirmation modal
  ↓
User confirms
  ↓
If dirty → Show force confirmation modal
If clean → Start background removal
  ↓
User confirms force (if needed)
  ↓
Spawn background removal thread
  ↓
Background thread sends progress updates via channel
  ↓
Main thread polls channel, updates status bar
  ↓
On completion, refresh worktree list and show success
```

## Detailed Design

### 1. Two-Stage Confirmation Flow

#### New Mode State

```rust
pub enum DashboardMode {
    Normal,
    Help,
    CreateInput,
    ConfirmDelete,      // existing - first confirmation
    ConfirmForceDelete, // new - second confirmation for dirty worktrees
}
```

#### Logic in `request_delete()`

Current behavior (unchanged for validation):
- Check if worktree is main → show error status, return
- Check if worktree is current → show error status, return
- Check if worktree is prunable/missing → show error status, return

New behavior:
- After validation passes, set `mode = DashboardMode::ConfirmDelete`
- The modal rendering will check dirty state and display appropriate text

#### Logic in `confirm_delete()`

Currently this method sets `pending_command = Remove` and `should_quit = true`.

New behavior:
- Get selected worktree record
- Check if details have loaded and worktree is dirty:
  - If `details.is_some()` and `details.dirty == true`:
    - Transition to `mode = DashboardMode::ConfirmForceDelete`
    - Return (don't start removal yet)
  - If details not loaded yet or clean:
    - Call `start_removal(false)` (new method, force = false)
    - Set `mode = DashboardMode::Normal`

Note: If details haven't loaded yet, we treat it as clean and let the removal operation handle any dirty state (it will be checked again in the background thread via `is_dirty_at()` in the existing `remove.rs` code).

#### New Method: `confirm_force_delete()`

```rust
pub fn confirm_force_delete(&mut self) {
    self.start_removal(true); // force = true
    self.mode = DashboardMode::Normal;
}
```

#### New Method: `start_removal(force: bool)`

```rust
fn start_removal(&mut self, force: bool) {
    let Some(record) = self.selected() else { return };

    let repo_path = /* get from app state or re-open repo */;
    let worktree_name = record.info.name.clone();

    self.removal_operation = Some(spawn_removal_operation(
        repo_path,
        worktree_name,
        force,
    ));
    self.removal_in_progress = true;
}
```

#### Key Binding Updates

In `handle_delete_key()`:
- `y/Y` → calls `app.confirm_delete()` (existing, but modified logic)
- `Esc/n/N` → returns to Normal mode (existing)

New function `handle_force_delete_key()`:
- `y/Y` → calls `app.confirm_force_delete()` (new)
- `Esc/n/N` → returns to Normal mode

Update `handle_key()` to route `ConfirmForceDelete` mode to `handle_force_delete_key()`.

### 2. Background Removal Operation

#### RemovalUpdate Enum

```rust
#[derive(Debug)]
enum RemovalUpdate {
    RunningPreHook,
    RemovingWorktree,
    RunningPostHook,
    Success { removed_name: String },
    Error { message: String },
}
```

#### App State Changes

```rust
pub struct WorktreeApp {
    // ... existing fields
    removal_operation: Option<Receiver<RemovalUpdate>>,
    removal_in_progress: bool,
    removal_status: Option<String>,
}
```

#### spawn_removal_operation()

Located in `src/tui/worktree/app.rs`, similar to `spawn_loader()`:

```rust
fn spawn_removal_operation(
    repo_path: PathBuf,
    worktree_name: String,
    force: bool,
) -> Receiver<RemovalUpdate> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let repo = match GitRepo::open_from_path(&repo_path) {
            Ok(repo) => repo,
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to open repository: {}", e),
                });
                return;
            }
        };

        let config = match Config::load() {
            Ok(config) => config,
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to load config: {}", e),
                });
                return;
            }
        };

        let worktree = match find_worktree(&repo, &worktree_name) {
            Ok(Some(wt)) => wt,
            Ok(None) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Worktree '{}' not found", worktree_name),
                });
                return;
            }
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Failed to find worktree: {}", e),
                });
                return;
            }
        };

        // Pre-hook
        let _ = sender.send(RemovalUpdate::RunningPreHook);
        // Note: We can't easily intercept individual hook stages from remove_worktree_with_hooks
        // so we'll send a single update before calling it

        // Actual removal
        let _ = sender.send(RemovalUpdate::RemovingWorktree);
        match remove_worktree_with_hooks(&repo, &config, &worktree, force) {
            Ok(display_name) => {
                let _ = sender.send(RemovalUpdate::Success {
                    removed_name: display_name,
                });
            }
            Err(e) => {
                let _ = sender.send(RemovalUpdate::Error {
                    message: format!("Removal failed: {}", e),
                });
            }
        }
    });

    receiver
}
```

**Note:** The `remove_worktree_with_hooks()` function already handles pre/post hooks and the actual removal. We send progress updates before calling it, but we can't intercept individual stages without refactoring that function. For the initial implementation, we'll send updates at coarse-grained boundaries. Future work could refactor the removal function to accept a progress callback.

### 3. Progress Updates and UI Changes

#### Polling in `refresh_background()`

Currently polls `loader` channel. Add polling for `removal_operation`:

```rust
pub fn refresh_background(&mut self) {
    // Existing loader polling
    loop {
        let update = match self.loader.as_ref() { /* ... */ };
        // ...
    }

    // New: removal operation polling
    loop {
        let update = match self.removal_operation.as_ref() {
            Some(rx) => match rx.try_recv() {
                Ok(update) => Some(update),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.removal_operation = None;
                    self.removal_in_progress = false;
                    None
                }
            },
            None => None,
        };

        let Some(update) = update else { break };
        self.apply_removal_update(update);
    }
}
```

#### apply_removal_update()

```rust
fn apply_removal_update(&mut self, update: RemovalUpdate) {
    match update {
        RemovalUpdate::RunningPreHook => {
            self.removal_status = Some("Running pre-remove hook...".to_string());
        }
        RemovalUpdate::RemovingWorktree => {
            self.removal_status = Some("Removing worktree...".to_string());
        }
        RemovalUpdate::RunningPostHook => {
            self.removal_status = Some("Running post-remove hook...".to_string());
        }
        RemovalUpdate::Success { removed_name } => {
            self.removal_in_progress = false;
            self.removal_operation = None;
            self.removal_status = None;
            self.set_status(format!("Removed '{}'", removed_name));

            // Trigger full reload of worktree list
            self.reload_worktrees();
        }
        RemovalUpdate::Error { message } => {
            self.removal_in_progress = false;
            self.removal_operation = None;
            self.removal_status = None;
            self.set_status(format!("Error: {}", message));
        }
    }
}
```

#### reload_worktrees()

```rust
fn reload_worktrees(&mut self) {
    let repo = match GitRepo::open() {
        Ok(r) => r,
        Err(e) => {
            self.set_status(format!("Failed to reload: {}", e));
            return;
        }
    };

    let repo_path = match repo.git_dir() {
        Ok(p) => p.to_path_buf(),
        Err(e) => {
            self.set_status(format!("Failed to get repo path: {}", e));
            return;
        }
    };

    let worktrees = match repo.list_worktrees() {
        Ok(wts) => wts,
        Err(e) => {
            self.set_status(format!("Failed to list worktrees: {}", e));
            return;
        }
    };

    self.records = worktrees.into_iter().map(WorktreeRecord::new).collect();

    // Adjust selection if needed
    if self.selected_index >= self.records.len() && !self.records.is_empty() {
        self.selected_index = self.records.len() - 1;
    }

    // Restart loader for new records
    if !self.records.is_empty() {
        self.loader = Some(spawn_loader(
            repo_path,
            self.records.iter().map(|r| r.info.clone()).collect(),
        ));
    } else {
        self.loader = None;
    }
}
```

#### Status Bar Rendering

In `render_status_bar()`, check `removal_status` first:

```rust
let status_text = if let Some(removal_msg) = &app.removal_status {
    removal_msg.clone()
} else if let Some(msg) = &app.status_message {
    msg.clone()
} else if let Some(loading_msg) = app.loading_summary() {
    loading_msg
} else {
    "Press ? for help".to_string()
};
```

This ensures removal progress messages take precedence over normal status.

#### Modal Rendering

Update `render_delete_modal()` to handle both confirmation stages:

```rust
fn render_delete_modal(f: &mut Frame, app: &WorktreeApp) {
    let area = centered_rect(52, 20, f.area());
    f.render_widget(Clear, area);

    let name = app
        .selected()
        .map(|record| record.info.name.clone())
        .unwrap_or_else(|| "this worktree".to_string());

    let (title, lines) = match app.mode {
        DashboardMode::ConfirmDelete => {
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("Remove ", Style::default().add_modifier(Modifier::BOLD)),
                    Span::styled(&name, Style::default().fg(Color::Red)),
                    Span::raw("?"),
                ]),
                Line::from(""),
                Line::from("Press y to confirm or Esc to cancel."),
            ];
            (" Confirm Remove ", lines)
        }
        DashboardMode::ConfirmForceDelete => {
            let lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("Warning: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                    Span::styled(&name, Style::default().fg(Color::Red)),
                    Span::raw(" has uncommitted changes."),
                ]),
                Line::from(""),
                Line::from("Force remove anyway?"),
                Line::from(""),
                Line::from("Press y to force remove or Esc to cancel."),
            ];
            (" Force Remove ", lines)
        }
        _ => return, // shouldn't happen
    };

    let widget = Paragraph::new(lines).block(
        Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Red)),
    );
    f.render_widget(widget, area);
}
```

### 4. Error Handling

#### Pre-removal Validation

These checks remain in `request_delete()` and prevent entering confirmation mode:

- Is main worktree? → `set_status("Cannot remove the main worktree")`, return
- Is current worktree? → `set_status("Cannot remove the current worktree from the dashboard")`, return
- Is prunable/missing? → `set_status("Missing worktree entries should be cleaned with 'st wt prune'")`, return

#### Background Thread Errors

All errors in the removal thread are caught and sent as `RemovalUpdate::Error`:

- Failed to open repo
- Failed to load config
- Worktree not found (race condition - removed between request and execution)
- Pre-remove hook failure (captured by `remove_worktree_with_hooks`)
- Removal failure (git error, locked worktree, etc.)
- Post-remove hook failure

Errors are displayed in the status bar and don't crash the TUI. The user can navigate away and retry if desired.

#### User Cancellation

Users can cancel at either confirmation stage by pressing Esc or 'n'. Once the background operation starts, there's no cancellation mechanism (matches CLI behavior - hooks and git operations aren't interruptible).

### 5. Changes to execute_dashboard_command()

Currently, `PendingCommand::Remove` exits the TUI and runs the command via `run_captured_command()`.

With this design, Remove commands no longer exit the TUI. We remove the `Remove` variant from `PendingCommand` entirely:

```rust
pub enum PendingCommand {
    Go { name: String },
    Create { name: Option<String> },
    // Remove { name: String }, // DELETED - now handled in-TUI
    Restack,
}
```

Required updates:
- **`PendingCommand::args()`** - remove `Remove` arm from match
- **`selection_after_command()`** - remove `Remove` arm from match
- **`execute_dashboard_command()`** - remove `Remove` arm from match (only Go, Create, Restack remain)
- **`confirm_delete()`** - no longer sets `pending_command`, calls `start_removal()` instead

### 6. Testing Strategy

#### Unit Tests

Add to `src/tui/worktree/app.rs`:

```rust
#[test]
fn two_stage_delete_for_dirty_worktree() {
    // Mock a WorktreeApp with a dirty worktree selected
    // Call request_delete() → mode should be ConfirmDelete
    // Call confirm_delete() → mode should be ConfirmForceDelete (not Normal)
    // Call confirm_force_delete() → removal should start
}

#[test]
fn single_stage_delete_for_clean_worktree() {
    // Mock a WorktreeApp with a clean worktree selected
    // Call request_delete() → mode should be ConfirmDelete
    // Call confirm_delete() → removal should start immediately
}

#[test]
fn removal_update_handling() {
    // Test apply_removal_update() with each variant
    // Verify status messages are set correctly
    // Verify removal_in_progress flag changes
}
```

#### Integration Tests

These would be manual or require a more sophisticated test harness (TUI testing is complex):

1. Create a worktree with uncommitted changes
2. Launch `st wt ls` TUI
3. Navigate to the dirty worktree (should show "dirty" badge)
4. Press 'd' → verify first modal appears
5. Press 'y' → verify second modal appears with warning text
6. Press 'y' → verify removal proceeds
7. Verify status bar shows progress messages
8. Verify worktree list refreshes and worktree is gone

#### Manual Testing Checklist

- [ ] Clean worktree removal shows single confirmation
- [ ] Dirty worktree removal shows two confirmations
- [ ] Cancel at first confirmation returns to Normal mode
- [ ] Cancel at second confirmation returns to Normal mode
- [ ] Status bar shows "Running pre-remove hook..." during removal
- [ ] Status bar shows "Removing worktree..." during removal
- [ ] Status bar shows success message after completion
- [ ] Worktree list refreshes automatically after removal
- [ ] Selection adjusts correctly after removal (doesn't go out of bounds)
- [ ] Error handling: try removing a locked worktree
- [ ] Error handling: try removing while hook fails

## Implementation Notes

### Dependencies

Need to import in `src/tui/worktree/app.rs`:
- `Config` from `crate::config::Config`
- `find_worktree` and `remove_worktree_with_hooks` from `crate::commands::worktree::shared`

### Code Organization

- All new logic lives in `src/tui/worktree/app.rs` and `src/tui/worktree/ui.rs`
- No changes to CLI commands (`src/commands/worktree/remove.rs`)
- Minimal changes to `src/tui/worktree/mod.rs` (routing the new mode)

### Potential Future Improvements

1. **Refactor `remove_worktree_with_hooks()`** to accept a progress callback, enabling finer-grained progress updates (individual hook start/end, etc.)

2. **Generalize to other operations**: The background operation pattern (spawn thread, send updates, poll in refresh) can be reused for Restack, Prune, and other long-running operations

3. **Cancellation support**: Add ability to cancel in-flight removals (requires more complex thread coordination)

4. **Retry mechanism**: Allow user to retry failed removals without exiting TUI

## Success Criteria

- [ ] Dirty worktrees show two-stage confirmation
- [ ] Clean worktrees show single-stage confirmation (no regression)
- [ ] TUI stays active during removal
- [ ] Status bar shows real-time progress messages
- [ ] Worktree list refreshes automatically after removal
- [ ] Errors are displayed gracefully without crashing TUI
- [ ] All existing TUI tests still pass
- [ ] No changes to CLI removal behavior

## Open Questions

None - all design decisions have been made.

## References

- Existing loader pattern: `src/tui/worktree/app.rs:388-434` (`spawn_loader()`)
- CLI removal logic: `src/commands/worktree/remove.rs:54-129`
- Removal with hooks: `src/commands/worktree/remove.rs:16-52`
