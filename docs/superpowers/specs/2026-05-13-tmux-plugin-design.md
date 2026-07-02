# stax tmux Plugin — Design Spec

**Date:** 2026-05-13  
**Status:** Implemented  
**Plan:** `docs/superpowers/plans/2026-05-13-tmux-plugin.md`

## Context

stax already ships significant tmux integration in `stax lane` and `stax worktree` (session creation, probe/fallback, window management). This feature extends that foundation by shipping a first-class **TPM-compatible tmux plugin** that gives stax users deeper terminal integration without any extra configuration burden.

### Problem

Users working with stacked PRs in tmux had no way to see stack/PR/CI state at a glance, no keybindings for common operations, and no automatic window labeling when navigating branches.

### Solution

Two deliverables:
1. A `stax tmux` Rust subcommand (in the stax binary) that provides data and rendering
2. A `stax.tmux` TPM plugin (separate repo) that wires everything into tmux

---

## Architecture

**Boundary:** stax owns all data and rendering logic. The TPM plugin is thin config only — no domain knowledge.

```
┌─────────────────────────────────────────┐
│  ~/.tmux.conf (via TPM)                 │
│  stax.tmux plugin                       │
│    ├── status-right → scripts/status.sh │  calls  stax tmux status
│    ├── prefix+S     → display-popup     │  calls  stax watch --current
│    ├── prefix+]     → run-shell         │  calls  stax down
│    ├── prefix+[     → run-shell         │  calls  stax up
│    └── prefix+M-s   → run-shell         │  calls  stax sync
└─────────────────────────────────────────┘
         │
         ▼
┌─────────────────────────────────────────┐
│  stax binary                            │
│    stax tmux status  →  format_status_line()  │  reads Stack + CiCache
│    stax tmux popup   →  tmux display-popup stax watch --current
└─────────────────────────────────────────┘
```

---

## Status Bar Segment

`stax tmux status` outputs a single tmux-formatted line:

```
 feat/login-flow [3/5] #42  ● passing
```

| Field | Format | Source |
|-------|--------|--------|
| Branch name | Truncated at 20 chars with `…` | `repo.current_branch()` |
| Stack position | `[pos/total]` | `Stack::current_stack()` |
| PR state | `⊘` none · `#N` open · `#N draft` · `#N merged` | `StackBranch.pr_number/pr_state/pr_is_draft` |
| CI state | `● passing` · `✗ failing` · `⟳ running` · `– no CI` | `CiCache.get_ci_state()` |

Colors use tmux format strings (`#[fg=green]`, `#[fg=red]`, etc.). Outputs nothing on trunk or outside a stax repo (graceful degradation). Reads from the existing CI cache — no live GitHub API calls on status-bar polling.

---

## Keybindings

| Binding | Command | Default key |
|---------|---------|-------------|
| Popup stack viewer | `display-popup -E stax watch --current` | `prefix + S` |
| Navigate toward trunk | `stax down` | `prefix + ]` |
| Navigate away from trunk | `stax up` | `prefix + [` |
| Sync stack | `stax sync` | `prefix + M-s` |

All keys are user-configurable via `@stax-*` tmux options. Setting a key to `''` disables the binding.

---

## Window Rename on Checkout

The plugin ships `scripts/window-rename.sh`, a shell snippet that installs a `precmd` hook (zsh) or `PROMPT_COMMAND` entry (bash). After every command prompt, if inside tmux, it reads the current git branch and renames the active tmux window to match.

No stax binary changes required — this is pure shell.

---

## Implementation

### Rust (`src/commands/tmux.rs`)

```
TmuxCommand::Status  →  run_status()
TmuxCommand::Popup   →  run_popup()
format_status_line() — pure function, fully unit-tested (7 tests)
```

### TPM Plugin (`stax.tmux` repo)

```
stax.tmux                  # TPM entry point — reads @stax-* options, sets keybindings + status-right
scripts/status.sh          # calls stax tmux status
scripts/window-rename.sh   # precmd/PROMPT_COMMAND hook
README.md                  # installation + config docs
```

### Files modified in stax repo

| File | Change |
|------|--------|
| `src/commands/tmux.rs` | New: TmuxCommand, format_status_line, run_status, run_popup |
| `src/commands/mod.rs` | +1 line: `pub mod tmux;` |
| `src/cli.rs` | +7 lines: Tmux variant + dispatch |
