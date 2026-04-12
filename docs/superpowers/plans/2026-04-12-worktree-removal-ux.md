# Worktree Removal UX Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add two-stage confirmation for dirty worktrees and keep TUI active during removal with live progress updates.

**Architecture:** Extends existing background loader pattern (mpsc channels + threads) to handle removal operations. Removal stays in-TUI instead of exiting to shell.

**Tech Stack:** Rust, ratatui, crossterm, std::sync::mpsc, std::thread

---

## File Structure

### Files to Modify

1. **`src/tui/worktree/app.rs`** - Core app state and logic
   - Add `ConfirmForceDelete` to `DashboardMode` enum
   - Add `RemovalUpdate` enum for background thread communication
   - Add fields to `WorktreeApp`: `removal_operation`, `removal_in_progress`, `removal_status`, `repo_path`
   - Add methods: `start_removal()`, `confirm_force_delete()`, `apply_removal_update()`, `reload_worktrees()`
   - Modify `confirm_delete()` to check dirty state and transition to force confirmation
   - Modify `refresh_background()` to poll removal channel
   - Add `spawn_removal_operation()` function

2. **`src/tui/worktree/ui.rs`** - Rendering logic
   - Modify `render_delete_modal()` to handle both `ConfirmDelete` and `ConfirmForceDelete` modes
   - Modify `render_status_bar()` to prioritize `removal_status` messages

3. **`src/tui/worktree/mod.rs`** - Event handling and command execution
   - Add `handle_force_delete_key()` function
   - Modify `handle_key()` to route `ConfirmForceDelete` mode
   - Remove `PendingCommand::Remove` variant
   - Update `PendingCommand::args()` to remove Remove arm
   - Update `selection_after_command()` to remove Remove arm
   - Update `execute_dashboard_command()` to remove Remove arm

### No New Files

All changes are modifications to existing files.

---

## Task 1: Add ConfirmForceDelete Mode and RemovalUpdate Enum

**Files:**
- Modify: `src/tui/worktree/app.rs:14-19` (DashboardMode enum)
- Modify: `src/tui/worktree/app.rs:37-50` (add RemovalUpdate enum after LoaderUpdate)

- [ ] **Step 1: Add ConfirmForceDelete variant to DashboardMode**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DashboardMode {
    Normal,
    Help,
    CreateInput,
    ConfirmDelete,
    ConfirmForceDelete,
}
```

- [ ] **Step 2: Add RemovalUpdate enum after LoaderUpdate**

Insert after line 50 (after `LoaderUpdate` enum):

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

- [ ] **Step 3: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS (no compilation errors)

- [ ] **Step 4: Commit**

```bash
git add src/tui/worktree/app.rs
git commit -m "feat(tui): add ConfirmForceDelete mode and RemovalUpdate enum

Add new dashboard mode for force confirmation of dirty worktree removal.
Add RemovalUpdate enum for background removal progress communication.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 2: Add New Fields to WorktreeApp

**Files:**
- Modify: `src/tui/worktree/app.rs:114-125` (WorktreeApp struct)
- Modify: `src/tui/worktree/app.rs:127-161` (WorktreeApp::new method)

- [ ] **Step 1: Add new fields to WorktreeApp struct**

```rust
pub struct WorktreeApp {
    pub records: Vec<WorktreeRecord>,
    pub selected_index: usize,
    pub mode: DashboardMode,
    pub input_buffer: String,
    pub input_cursor: usize,
    pub status_message: Option<String>,
    pub should_quit: bool,
    pub pending_command: Option<PendingCommand>,
    tmux_availability: TmuxAvailability,
    loader: Option<Receiver<LoaderUpdate>>,
    removal_operation: Option<Receiver<RemovalUpdate>>,
    removal_in_progress: bool,
    removal_status: Option<String>,
    repo_path: PathBuf,
}
```

- [ ] **Step 2: Initialize new fields in WorktreeApp::new**

In the return statement of `new()`, add the new fields:

```rust
Ok(Self {
    records,
    selected_index,
    mode: DashboardMode::Normal,
    input_buffer: String::new(),
    input_cursor: 0,
    status_message: initial_status,
    should_quit: false,
    pending_command: None,
    tmux_availability: TmuxAvailability::Loading,
    loader,
    removal_operation: None,
    removal_in_progress: false,
    removal_status: None,
    repo_path,
})
```

- [ ] **Step 3: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add src/tui/worktree/app.rs
git commit -m "feat(tui): add removal operation state fields to WorktreeApp

Add fields for tracking background removal operations:
- removal_operation: channel receiver for progress updates
- removal_in_progress: flag indicating active removal
- removal_status: current removal progress message
- repo_path: stored for background thread access

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 3: Implement confirm_force_delete Method

**Files:**
- Modify: `src/tui/worktree/app.rs:264` (add after confirm_delete method)

- [ ] **Step 1: Add confirm_force_delete method**

Insert after the `confirm_delete()` method (around line 264):

```rust
pub fn confirm_force_delete(&mut self) {
    self.start_removal(true);
    self.mode = DashboardMode::Normal;
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: ERROR - `start_removal` method not found (this is expected, we'll add it next)

- [ ] **Step 3: Commit preparation**

Wait for next task to implement `start_removal()` before committing.

---

## Task 4: Implement start_removal Method

**Files:**
- Modify: `src/tui/worktree/app.rs:265` (add after confirm_force_delete)

- [ ] **Step 1: Add start_removal method**

```rust
fn start_removal(&mut self, force: bool) {
    let Some(record) = self.selected() else {
        return;
    };

    let worktree_name = record.info.name.clone();
    let repo_path = self.repo_path.clone();

    self.removal_operation = Some(spawn_removal_operation(repo_path, worktree_name, force));
    self.removal_in_progress = true;
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: ERROR - `spawn_removal_operation` function not found (expected, we'll add it later)

- [ ] **Step 3: Commit preparation**

Wait to commit until `spawn_removal_operation()` is implemented.

---

## Task 5: Modify confirm_delete to Check Dirty State

**Files:**
- Modify: `src/tui/worktree/app.rs:257-264` (confirm_delete method)

- [ ] **Step 1: Replace confirm_delete method logic**

Replace the existing `confirm_delete()` method body:

```rust
pub fn confirm_delete(&mut self) {
    if let Some(record) = self.selected() {
        // Check if worktree is dirty and details are loaded
        if let Some(details) = &record.details {
            if details.dirty {
                // Dirty worktree: show force confirmation
                self.mode = DashboardMode::ConfirmForceDelete;
                return;
            }
        }

        // Clean worktree or details not loaded: proceed with removal
        self.start_removal(false);
        self.mode = DashboardMode::Normal;
    }
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: ERROR - `spawn_removal_operation` not found (still expected)

- [ ] **Step 3: Commit preparation**

Wait for spawn_removal_operation to be implemented before committing.

---

## Task 6: Implement spawn_removal_operation Function

**Files:**
- Modify: `src/tui/worktree/app.rs:1` (add imports at top)
- Modify: `src/tui/worktree/app.rs:434` (add after spawn_loader function)

- [ ] **Step 1: Add required imports**

Add to the imports at the top of the file:

```rust
use crate::commands::worktree::shared::{find_worktree, remove_worktree_with_hooks};
use crate::config::Config;
```

- [ ] **Step 2: Implement spawn_removal_operation function**

Add after the `spawn_loader()` function (around line 434):

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

        let _ = sender.send(RemovalUpdate::RunningPreHook);
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

- [ ] **Step 3: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 4: Commit**

```bash
git add src/tui/worktree/app.rs
git commit -m "feat(tui): implement removal operation methods

Add methods for handling worktree removal in background:
- start_removal(): initiates background removal operation
- confirm_force_delete(): confirms force removal of dirty worktree
- spawn_removal_operation(): background thread for removal
- Modified confirm_delete(): checks dirty state before removal

Two-stage confirmation flow now functional for dirty worktrees.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 7: Implement apply_removal_update Method

**Files:**
- Modify: `src/tui/worktree/app.rs:385` (add after apply_loader_update method)

- [ ] **Step 1: Add apply_removal_update method**

Insert after the `apply_loader_update()` method:

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

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: ERROR - `reload_worktrees` method not found (expected)

- [ ] **Step 3: Commit preparation**

Wait for reload_worktrees to be implemented.

---

## Task 8: Implement reload_worktrees Method

**Files:**
- Modify: `src/tui/worktree/app.rs:386` (add after apply_removal_update)

- [ ] **Step 1: Add reload_worktrees method**

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

    if self.selected_index >= self.records.len() && !self.records.is_empty() {
        self.selected_index = self.records.len() - 1;
    }

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

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add src/tui/worktree/app.rs
git commit -m "feat(tui): implement removal progress handling

Add methods for handling removal operation updates:
- apply_removal_update(): processes progress messages from background thread
- reload_worktrees(): refreshes worktree list after successful removal

Status bar now shows live progress during removal operations.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 9: Modify refresh_background to Poll Removal Channel

**Files:**
- Modify: `src/tui/worktree/app.rs:303-322` (refresh_background method)

- [ ] **Step 1: Add removal channel polling to refresh_background**

Add after the existing loader polling loop (before the closing brace):

```rust
pub fn refresh_background(&mut self) {
    // Existing loader polling
    loop {
        let update = match self.loader.as_ref() {
            Some(loader) => match loader.try_recv() {
                Ok(update) => Some(update),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => {
                    self.loader = None;
                    None
                }
            },
            None => None,
        };

        let Some(update) = update else {
            break;
        };
        self.apply_loader_update(update);
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

        let Some(update) = update else {
            break;
        };
        self.apply_removal_update(update);
    }
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Run existing tests**

Run: `cargo nextest run -p stax worktree::app`
Expected: PASS (all existing tests should still pass)

- [ ] **Step 4: Commit**

```bash
git add src/tui/worktree/app.rs
git commit -m "feat(tui): poll removal channel in refresh_background

Extend refresh_background() to poll removal operation channel
in addition to loader channel. Enables real-time progress updates
for background removal operations.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 10: Update render_delete_modal for Two-Stage Confirmation

**Files:**
- Modify: `src/tui/worktree/ui.rs:369-393` (render_delete_modal function)

- [ ] **Step 1: Replace render_delete_modal function**

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
                    Span::styled(
                        "Warning: ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
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

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add src/tui/worktree/ui.rs
git commit -m "feat(tui): add force confirmation modal rendering

Update render_delete_modal() to handle both ConfirmDelete and
ConfirmForceDelete modes. Force confirmation shows warning about
uncommitted changes.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 11: Update Status Bar to Show Removal Progress

**Files:**
- Modify: `src/tui/worktree/ui.rs` (find render_status_bar function)

- [ ] **Step 1: Find the render_status_bar function**

Run: `rg -n "fn render_status_bar" src/tui/worktree/ui.rs`
Expected: Find the function definition line number

- [ ] **Step 2: Locate status text assignment**

Search for where `status_text` or the status message is constructed in `render_status_bar()`.

- [ ] **Step 3: Update status message priority logic**

Find the line that constructs the status message and modify it to check `removal_status` first:

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

- [ ] **Step 4: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 5: Commit**

```bash
git add src/tui/worktree/ui.rs
git commit -m "feat(tui): prioritize removal status in status bar

Update status bar rendering to show removal_status messages
with highest priority, ensuring live progress updates are visible
during background removal operations.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 12: Add handle_force_delete_key Function

**Files:**
- Modify: `src/tui/worktree/mod.rs:156` (add after handle_delete_key)

- [ ] **Step 1: Add handle_force_delete_key function**

Insert after `handle_delete_key()` function:

```rust
fn handle_force_delete_key(app: &mut WorktreeApp, code: KeyCode) -> Result<()> {
    match code {
        KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
            app.mode = DashboardMode::Normal;
        }
        KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_force_delete(),
        _ => {}
    }
    Ok(())
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Commit preparation**

Wait for next task to update handle_key routing.

---

## Task 13: Update handle_key to Route ConfirmForceDelete Mode

**Files:**
- Modify: `src/tui/worktree/mod.rs:84-94` (handle_key function)

- [ ] **Step 1: Add ConfirmForceDelete arm to handle_key**

```rust
fn handle_key(app: &mut WorktreeApp, code: KeyCode, modifiers: KeyModifiers) -> Result<()> {
    match app.mode {
        DashboardMode::Normal => handle_normal_key(app, code, modifiers),
        DashboardMode::Help => {
            app.mode = DashboardMode::Normal;
            Ok(())
        }
        DashboardMode::CreateInput => handle_create_key(app, code),
        DashboardMode::ConfirmDelete => handle_delete_key(app, code),
        DashboardMode::ConfirmForceDelete => handle_force_delete_key(app, code),
    }
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Commit**

```bash
git add src/tui/worktree/mod.rs
git commit -m "feat(tui): add force delete key handling

Add handle_force_delete_key() to handle user input in
ConfirmForceDelete mode. Routes y/n/Esc keys appropriately.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 14: Remove PendingCommand::Remove Variant

**Files:**
- Modify: `src/tui/worktree/app.rs:88-112` (PendingCommand enum and impl)

- [ ] **Step 1: Remove Remove variant from PendingCommand enum**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCommand {
    Go { name: String },
    Create { name: Option<String> },
    Restack,
}
```

- [ ] **Step 2: Update PendingCommand::args() to remove Remove arm**

```rust
impl PendingCommand {
    pub fn args(&self) -> Vec<String> {
        match self {
            Self::Go { name } => vec!["wt".into(), "go".into(), name.clone(), "--tmux".into()],
            Self::Create { name } => {
                let mut args = vec!["wt".into(), "c".into()];
                if let Some(name) = name {
                    args.push(name.clone());
                }
                args.push("--tmux".into());
                args
            }
            Self::Restack => vec!["wt".into(), "rs".into()],
        }
    }
}
```

- [ ] **Step 3: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS or potential warnings about unused Remove patterns elsewhere

- [ ] **Step 4: Commit preparation**

Wait to commit until all Remove references are removed.

---

## Task 15: Update selection_after_command Function

**Files:**
- Modify: `src/tui/worktree/mod.rs:158-165` (selection_after_command function)

- [ ] **Step 1: Remove Remove arm from selection_after_command**

```rust
fn selection_after_command(command: &PendingCommand) -> Option<String> {
    match command {
        PendingCommand::Go { name } | PendingCommand::Create { name: Some(name) } => {
            Some(name.clone())
        }
        PendingCommand::Create { name: None } | PendingCommand::Restack => None,
    }
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Commit preparation**

Wait to commit until execute_dashboard_command is updated.

---

## Task 16: Update execute_dashboard_command Function

**Files:**
- Modify: `src/tui/worktree/mod.rs:167-192` (execute_dashboard_command function)

- [ ] **Step 1: Remove Remove arm from execute_dashboard_command**

```rust
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
        PendingCommand::Restack => run_captured_command(&exe, workdir, &args)
            .map(|status| status.or_else(|| Some("Restacked managed worktrees".to_string()))),
    }
}
```

- [ ] **Step 2: Verify code compiles**

Run: `cargo check`
Expected: SUCCESS

- [ ] **Step 3: Run tests**

Run: `cargo nextest run -p stax tui::worktree`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/tui/worktree/app.rs src/tui/worktree/mod.rs
git commit -m "refactor(tui): remove PendingCommand::Remove variant

Remove the Remove variant from PendingCommand enum as removal
operations now happen in-TUI via background threads instead of
exiting to shell.

Updated:
- PendingCommand enum definition
- PendingCommand::args() method
- selection_after_command()
- execute_dashboard_command()

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 17: Run Full Test Suite

**Files:**
- Test: All existing tests

- [ ] **Step 1: Run full test suite**

Run: `just test` (or `make test` or `cargo nextest run`)
Expected: All tests PASS

- [ ] **Step 2: Check for clippy warnings**

Run: `cargo clippy -- -D warnings`
Expected: No warnings in modified files

- [ ] **Step 3: Format code**

Run: `cargo fmt`
Expected: Code formatted successfully

- [ ] **Step 4: Commit formatting if needed**

```bash
git add -A
git commit -m "chore: format code

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 18: Manual Testing - Clean Worktree Removal

**Files:**
- Test: Manual TUI interaction

- [ ] **Step 1: Create a clean test worktree**

Run:
```bash
cd /Users/cesarferreira/code/github/stax
cargo build
./target/debug/st wt c test-clean-removal
```

Expected: Worktree created successfully

- [ ] **Step 2: Launch TUI**

Run: `./target/debug/st wt ls`
Expected: TUI opens with worktree list

- [ ] **Step 3: Navigate to test worktree and delete**

1. Use arrow keys to select `test-clean-removal`
2. Press `d`
3. Verify: Modal appears with "Remove test-clean-removal?"
4. Press `y`
5. Verify: Status bar shows "Removing worktree..."
6. Verify: List refreshes and worktree is removed
7. Verify: Status bar shows "Removed 'test-clean-removal'"

Expected: Single confirmation, worktree removed, TUI stays active

- [ ] **Step 4: Document results**

Note: Clean worktree removal works with single confirmation ✓

---

## Task 19: Manual Testing - Dirty Worktree Removal

**Files:**
- Test: Manual TUI interaction

- [ ] **Step 1: Create a dirty test worktree**

Run:
```bash
./target/debug/st wt c test-dirty-removal
cd .worktrees/test-dirty-removal
echo "test" > test.txt
git add test.txt
cd ../..
```

Expected: Worktree with uncommitted changes

- [ ] **Step 2: Launch TUI and verify dirty badge**

Run: `./target/debug/st wt ls`
Expected: TUI shows "dirty" badge on test-dirty-removal

- [ ] **Step 3: Attempt to delete dirty worktree**

1. Select `test-dirty-removal`
2. Press `d`
3. Verify: First modal appears "Remove test-dirty-removal?"
4. Press `y`
5. Verify: Second modal appears "Warning: test-dirty-removal has uncommitted changes. Force remove anyway?"
6. Press `y`
7. Verify: Status bar shows progress messages
8. Verify: Worktree is removed
9. Verify: Status bar shows success message

Expected: Two-stage confirmation, forced removal succeeds

- [ ] **Step 4: Document results**

Note: Dirty worktree removal requires two confirmations ✓

---

## Task 20: Manual Testing - Cancellation

**Files:**
- Test: Manual TUI interaction

- [ ] **Step 1: Test cancellation at first confirmation**

1. Create test worktree: `./target/debug/st wt c test-cancel-1`
2. Launch TUI: `./target/debug/st wt ls`
3. Select worktree, press `d`
4. Press `Esc` (or `n`)
5. Verify: Modal closes, returns to Normal mode
6. Verify: Worktree still exists

Expected: Cancellation works at first stage ✓

- [ ] **Step 2: Test cancellation at force confirmation**

1. Make worktree dirty: `cd .worktrees/test-cancel-1 && echo "test" > test.txt && git add test.txt && cd ../..`
2. Launch TUI: `./target/debug/st wt ls`
3. Select dirty worktree, press `d`
4. Press `y` at first confirmation
5. Press `Esc` at force confirmation
6. Verify: Modal closes, returns to Normal mode
7. Verify: Worktree still exists

Expected: Cancellation works at second stage ✓

- [ ] **Step 3: Clean up test worktrees**

Run:
```bash
./target/debug/st wt rm test-cancel-1 --force
```

- [ ] **Step 4: Document results**

Note: Cancellation works at both stages ✓

---

## Task 21: Manual Testing - Error Handling

**Files:**
- Test: Manual TUI interaction

- [ ] **Step 1: Test removing main worktree (should show error)**

1. Launch TUI: `./target/debug/st wt ls`
2. Select the main worktree (likely first in list)
3. Press `d`
4. Verify: Status bar shows "Cannot remove the main worktree"
5. Verify: No modal appears

Expected: Error shown, operation prevented ✓

- [ ] **Step 2: Test removing current worktree from dashboard**

1. Create and enter worktree: `./target/debug/st wt c test-current && cd .worktrees/test-current`
2. Launch TUI from that directory: `../../target/debug/st wt ls`
3. The current worktree should be highlighted
4. Press `d`
5. Verify: Status bar shows "Cannot remove the current worktree from the dashboard"
6. Return to main: `cd ../..`

Expected: Error shown, operation prevented ✓

- [ ] **Step 3: Clean up**

Run: `./target/debug/st wt rm test-current`

- [ ] **Step 4: Document results**

Note: Error handling works correctly ✓

---

## Task 22: Final Integration Test

**Files:**
- Test: End-to-end workflow

- [ ] **Step 1: Create realistic scenario**

```bash
# Create multiple worktrees
./target/debug/st wt c feature-1
./target/debug/st wt c feature-2
cd .worktrees/feature-2
echo "work in progress" > wip.txt
git add wip.txt
cd ../..
```

- [ ] **Step 2: Launch TUI and remove clean worktree**

1. Run: `./target/debug/st wt ls`
2. Select `feature-1` (clean)
3. Press `d`, then `y`
4. Verify: Single confirmation, removed successfully
5. Verify: List refreshes, feature-1 gone

- [ ] **Step 3: Remove dirty worktree**

1. Select `feature-2` (dirty badge visible)
2. Press `d`
3. Press `y` at first confirmation
4. Verify: Second modal appears with warning
5. Press `y` at force confirmation
6. Verify: Progress messages appear
7. Verify: Worktree removed successfully

- [ ] **Step 4: Verify TUI stability**

1. Navigate remaining worktrees
2. Press `?` to view help
3. Press Esc to return
4. Press `q` to quit
5. Verify: No crashes, clean exit

Expected: Complete workflow works smoothly ✓

- [ ] **Step 5: Document completion**

Note: All integration tests pass ✓

---

## Task 23: Update MEMORY.md

**Files:**
- Modify: `/Users/cesarferreira/.claude/projects/-Users-cesarferreira-code-github-stax/memory/MEMORY.md`

- [ ] **Step 1: Add implementation notes to MEMORY.md**

Add to the appropriate section:

```markdown
## TUI Patterns
- Background operations use mpsc channels + threads (see worktree removal as example)
- `RemovalUpdate` enum pattern mirrors `LoaderUpdate` for progress updates
- `refresh_background()` polls multiple channels (loader + removal)
- Two-stage confirmation: check dirty state in first confirmation, show force modal if needed
- Status bar priority: `removal_status` > `status_message` > `loading_summary` > default
- `reload_worktrees()` pattern: re-list, rebuild records, adjust selection, restart loader
```

- [ ] **Step 2: Commit memory update**

```bash
git add .claude/projects/-Users-cesarferreira-code-github-stax/memory/MEMORY.md
git commit -m "docs: document TUI background operation patterns

Add notes about worktree removal implementation patterns
for future reference.

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

---

## Task 24: Final Verification and Cleanup

**Files:**
- Test: All modified files

- [ ] **Step 1: Run full test suite one final time**

Run: `just test`
Expected: All tests PASS

- [ ] **Step 2: Check git status**

Run: `git status`
Expected: All changes committed, working tree clean

- [ ] **Step 3: Verify spec success criteria**

Check against spec requirements:
- ✓ Dirty worktrees show two-stage confirmation
- ✓ Clean worktrees show single-stage confirmation
- ✓ TUI stays active during removal
- ✓ Status bar shows real-time progress messages
- ✓ Worktree list refreshes automatically after removal
- ✓ Errors displayed gracefully without crashing TUI
- ✓ All existing tests pass
- ✓ No changes to CLI removal behavior

- [ ] **Step 4: Create summary commit if needed**

If any final touchups were made:

```bash
git add -A
git commit -m "feat(tui): complete worktree removal UX improvements

Summary of changes:
- Two-stage confirmation for dirty worktrees
- Background removal with live progress updates
- TUI stays active during removal operations
- Removed PendingCommand::Remove variant

Spec: docs/superpowers/specs/2026-04-12-worktree-removal-ux-design.md

Co-Authored-By: Claude Sonnet 4.5 <noreply@anthropic.com>"
```

- [ ] **Step 5: Mark implementation complete**

Implementation complete ✓

---

## Self-Review Checklist

**Spec coverage:**
- ✓ Two-stage confirmation flow (Tasks 1-6, 10, 12-13)
- ✓ Background removal operation (Tasks 6-9)
- ✓ Progress updates and UI changes (Tasks 10-11)
- ✓ Error handling (covered in implementation)
- ✓ PendingCommand cleanup (Tasks 14-16)
- ✓ Testing strategy (Tasks 17-22)

**Placeholder scan:**
- ✓ No TBD, TODO, or "implement later" markers
- ✓ All code blocks are complete
- ✓ All commands have expected output
- ✓ All file paths are exact

**Type consistency:**
- ✓ `RemovalUpdate` enum variants used consistently
- ✓ `DashboardMode::ConfirmForceDelete` used consistently
- ✓ Method names match across tasks (`confirm_force_delete`, `start_removal`, etc.)
- ✓ Field names match (`removal_operation`, `removal_in_progress`, `removal_status`, `repo_path`)

**Execution flow:**
- ✓ Tasks build incrementally (can't compile until spawn_removal_operation is added, which is correct)
- ✓ Commits are at logical boundaries
- ✓ Tests run at appropriate points
- ✓ Manual testing covers all user flows

All checks pass ✓
