# PR Readiness TUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `st ready` an interactive Ratatui dashboard with colors, arrow-key selection, Enter-to-open PRs, and progressive row hydration.

**Architecture:** Keep readiness classification and static output in `src/commands/ready.rs`, and extract reusable scope/row/fetch primitives for both static and TUI modes. Add `src/tui/ready/` as a focused TUI module with app state, rendering, and event loop, following existing `src/tui/worktree` patterns.

**Tech Stack:** Rust, clap, crossterm, ratatui, std `mpsc` channels, existing `ForgeClient`, existing `open_url_in_browser`.

---

### Task 1: CLI Mode Selection

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/commands/pr.rs`
- Modify: `src/commands/ready.rs`
- Test: `tests/cli_tests.rs`

- [x] Add failing CLI tests for `st ready --plain` and `st pr list --ready --plain`.
- [x] Run `cargo test --test cli_tests ready` and confirm the new tests fail.
- [x] Add `plain: bool` to `Commands::Ready` and `PrCommands::List`.
- [x] Update dispatch so `ready::run(all, json, plain)` receives mode flags.
- [x] In `commands::ready::run`, route `--json` to JSON, `--plain` or non-interactive use to static output, and interactive terminal use to `tui::ready::run`.
- [x] Run `cargo test --test cli_tests ready` and confirm the tests pass.

### Task 2: Shared Readiness Data

**Files:**
- Modify: `src/commands/ready.rs`

- [x] Add failing unit tests for placeholder rows and PR URL derivation.
- [x] Run `cargo test ready::tests --lib` and confirm failures.
- [x] Add `ReadyBranch`, `ReadyScope`, `ReadyRowState`, placeholder creation, PR URL derivation, and reusable `fetch_row_for_branch`.
- [x] Keep existing static table/JSON behavior by using the shared helpers.
- [x] Run `cargo test ready::tests --lib` and confirm tests pass.

### Task 3: Readiness TUI App State

**Files:**
- Create: `src/tui/ready/app.rs`

- [x] Add failing app tests for selection bounds, refresh resetting rows to loading, loaded row updates, and selected PR URL lookup.
- [x] Run `cargo test ready_tui --lib` and confirm failures.
- [x] Implement `ReadyTuiApp`, `ReadyTuiUpdate`, row update polling, selection movement, refresh state reset, loading count, status messages, and selected URL access.
- [x] Run `cargo test ready_tui --lib` and confirm tests pass.

### Task 4: Readiness TUI Rendering And Event Loop

**Files:**
- Create: `src/tui/ready/mod.rs`
- Create: `src/tui/ready/ui.rs`
- Modify: `src/tui/mod.rs`

- [x] Add failing rendering tests for action color mapping and help/details text.
- [x] Run `cargo test ready_tui --lib` and confirm failures.
- [x] Implement terminal setup, event loop, key handling, refresh loader spawning, open-on-Enter/`o`, quit/help behavior, and Ratatui rendering.
- [x] Run `cargo test ready_tui --lib` and confirm tests pass.

### Task 5: Documentation And Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/commands/core.md`
- Modify: `docs/commands/reference.md`
- Modify: `skills.md`

- [x] Update docs to describe interactive default, `--plain`, key bindings, and non-interactive agent usage.
- [x] Run `cargo fmt`.
- [x] Run targeted tests: `cargo test ready::tests --lib`, `cargo test ready_tui --lib`, and `cargo test --test cli_tests ready`.
- [x] Run smoke checks: `cargo run --quiet -- ready --plain` and `cargo run --quiet -- ready --json`.
- [x] Run `make test` when Docker is available; if Docker is unavailable, record the exact failure and run `cargo check --all-targets`.
- [x] Commit and `st submit --draft --yes --no-prompt --no-template` to update PR #447.
