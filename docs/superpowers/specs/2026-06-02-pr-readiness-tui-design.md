# PR readiness TUI

**Date:** 2026-06-02
**Status:** Design approved, self-reviewed, pending implementation

## Problem

`st ready` now provides the right readiness data, but the static table is still a one-shot report. The next workflow is interactive triage:

- See readiness rows with color.
- Move through PRs with arrow keys.
- Press `Enter` to open the selected PR.
- Watch fields fill in as live forge data arrives.

## Goal

Turn `st ready` into an interactive Ratatui dashboard by default in an interactive terminal, while preserving script-friendly and plain-text output modes.

## Command Behavior

| Command | Behavior |
|---|---|
| `st ready` | Launch interactive readiness TUI when stdin/stdout are terminals. |
| `st ready --all` | Same TUI, scoped to all tracked branch PRs. |
| `st ready --json` | Keep existing JSON output; never launch TUI. |
| `st ready --plain` | Render the current static readiness table. |
| `st pr list --ready` | Match `st ready` behavior. |
| `st pr list --ready --plain` | Static table under the PR-list entrypoint. |
| non-interactive `st ready` | Fall back to plain table so pipes and captured output do not receive TUI escape sequences. |

## TUI Layout

The main screen is a full-screen Ratatui app:

```text
 PR Readiness  current stack · fresh 12:04:31 · 4 PRs · loading 2
┌──────────────────────────────────────────────────────────────────────────────┐
│ ACTION   PR       BRANCH                REVIEWS         CI        TITLE      │
│ ✓ merge  #115665  cesar/tbt-poc         2 approvals     passed    Internal…  │
│ ● ping   #114961  cesar/ruby-version    missing review  passed    Enforce…   │
│ ✕ fix    #112732  codex/versioning      1 approval      2 failed  Date…      │
│ ○ wait   #107328  codex/bazel-docker    loading…        loading…  loading…   │
└──────────────────────────────────────────────────────────────────────────────┘
 ↑/↓ move  Enter open PR  r refresh  o open  ? help  q quit
```

On wide terminals, split horizontally:

- Left: readiness table.
- Right: details for the selected row: reason, mergeability, review decision, approvals, CI summary, PR title, branch, PR URL, and any load error.

On narrow terminals, use the table-only layout and surface details in the status bar.

## Colors

Use color as a status cue, while keeping the text labels readable without color:

| Action | Color |
|---|---|
| `✓ merge` | green |
| `● ping` | yellow |
| `✕ fix` | red |
| `○ wait` | blue for mergeability wait; yellow when CI is active |
| `◌ draft` | dark gray / dim |
| loading placeholders | dim |
| selected row | reversed or highlighted row style matching existing TUI patterns |

## Interaction

| Key | Action |
|---|---|
| `Up` / `k` | Select previous row. |
| `Down` / `j` | Select next row. |
| `Enter` | Open selected PR in the default browser. |
| `o` | Open selected PR in the default browser. |
| `r` | Refresh all live data. |
| `?` | Show help overlay. |
| `q` / `Esc` | Quit. |

Opening a PR uses the existing `open_url_in_browser` helper. If a selected row has no PR URL or PR number, show a status message instead of exiting.

## Progressive Hydration

The TUI shows useful local data immediately, then hydrates rows as forge responses arrive:

1. Load repo, config, stack, remote, and branch scope synchronously.
2. Create one row per branch in scope with:
   - branch name
   - PR number from stack metadata when available
   - action/review/CI/title set to `loading…`
3. Spawn a background loader thread.
4. The loader resolves any missing PR numbers by branch lookup.
5. For each row independently, fetch:
   - PR merge status
   - detailed checks for the PR head SHA
6. Send row updates through an `mpsc` channel as soon as each row is ready.
7. The app polls the receiver each render tick and updates rows in place.
8. When the loader finishes, the header changes from `loading N` to `fresh HH:MM:SS`.

Refresh (`r`) clears loaded live fields back to loading state and starts a new loader. Existing branch/PR-number placeholders remain visible during refresh.

## Data Model

Refactor `src/commands/ready.rs` so the static renderer and TUI share core data:

- `ReadyScope`: branch list, repo label, remote info, current branch, all/current-stack scope label.
- `ReadyPlaceholderRow`: local row before live data arrives.
- `PrReadinessRow`: current computed live row, extended with PR URL or enough data to derive it.
- `ReadyRowState`: `Loading`, `Loaded(PrReadinessRow)`, `Unavailable { message }`.
- `ReadyAction` and `ReadyReason`: unchanged classification enums.

The TUI app owns `Vec<ReadyRowState>` plus selected index, status message, loading count, and optional background receiver.

## Architecture

Add:

- `src/tui/ready/mod.rs`: terminal setup, event loop, key handling, browser-open command execution.
- `src/tui/ready/app.rs`: app state, row selection, refresh orchestration, background update polling.
- `src/tui/ready/ui.rs`: Ratatui rendering, colors, help overlay.

Modify:

- `src/tui/mod.rs`: expose `ready`.
- `src/commands/ready.rs`: choose TUI/plain/JSON mode and expose reusable readiness fetch/render helpers.
- `src/cli/args.rs`: add `--plain` to `Ready` and ready-list options.
- `src/cli/mod.rs` and `src/commands/pr.rs`: pass the new plain flag.

## Error Handling

- Missing auth or remote: return a normal CLI error before entering alternate-screen mode.
- Row-level fetch failure: show the row as unavailable with `unknown` fields and a details-pane error; do not classify as `merge`.
- Loader disconnect: keep current rows, stop spinner/loading count, and show a status message.
- Browser open failure: rely on `open_url_in_browser` warning behavior and keep the TUI running.

## Testing

Add unit tests for:

- TUI row state transitions from placeholder to loaded.
- Selection movement boundaries.
- `Enter`/`o` open behavior choosing the selected PR URL.
- Refresh resetting loaded rows to loading.
- Color/style mapping for each `ReadyAction`.
- CLI parsing for `st ready --plain` and `st pr list --ready --plain`.

Run targeted tests:

```bash
cargo test ready::tests --lib
cargo test ready_tui --lib
cargo test --test cli_tests ready
```

For full validation, run `make test` when Docker is available.

## Documentation

Update:

- `README.md`: note that `st ready` is interactive by default and `--plain` renders a static table.
- `docs/commands/core.md`: include key bindings.
- `docs/commands/reference.md`: list `--plain`.
- `skills.md`: teach agents to use `st ready --plain` or `--json` for non-interactive use.

## Out of Scope

- Editing reviewers or re-requesting review from the TUI.
- Merging from the TUI.
- Opening comments or checks pages separately.
- Embedding this into the main bare `st` stack TUI.
