# AI Worktree Lanes

`st wt` lets you run multiple AI coding sessions in parallel while keeping them inside stax's normal branch model.

This page focuses on the AI workflow. For the full `st worktree` / `st wt` command reference, dashboard behavior, cleanup semantics, shell integration, and configuration, see [Worktrees](../worktrees/index.md).

## Why Use AI Lanes

Worktree lanes solve a specific problem: you want several active coding sessions at once without making them invisible to the rest of your stack.

With stax lanes you can:

- isolate each agent session to its own worktree and branch
- keep those branches visible in `st ls`
- restack them safely when trunk or a parent moves
- jump back into them with `st wt go`
- inspect them with `st wt ll`
- clean them up with normal stax worktree commands

That is the difference between "a pile of terminals" and "several active branches that stax still understands."

## Example Workflow

```bash
# Start three parallel lanes
st lane auth-refresh "fix token refresh edge cases"
st lane flaky-tests "stabilize the flaky test suite"
st wt c ui-polish --run "cursor ."
st lane review-pass "address the open PR comments"

# They are normal stax branches
st ls

# Jump back into any lane later
st lane flaky-tests
st lane

# Trunk moved while those sessions were in flight
st wt rs

# See which lanes are dirty / rebasing / managed
st wt ll

# Remove finished work
st wt rm auth-refresh --delete-branch
```

## Primary Commands

```bash
st lane api-tests "write the missing integration tests"
st lane api-tests --agent gemini
st lane api-tests --agent opencode --model opencode/gpt-5.1-codex
st lane review-pass "address the open PR comments"
st lane
```

`st lane <name> [prompt]` is the canonical explicit form:

- create or reuse the named lane
- default to your configured AI agent
- default to tmux when available
- use `--no-tmux` when you want a direct terminal launch instead

`st lane` also supports the interactive path:

- `st lane` opens an interactive picker of stax-managed lanes
- `st lane <name> [prompt]` is the short explicit form
- if a lane already has a tmux session, stax reattaches to it
- if you pass a new prompt to an existing tmux-backed lane, stax opens a fresh tmux window in that session

Supported `--agent` values are:

- `claude`
- `codex`
- `gemini`
- `opencode`

Use `--model` with `--agent` when you want an explicit model override.

Use `--no-tmux` when you want each AI lane to launch directly in the terminal instead of tmux.

## Advanced Patterns

The older worktree launch flags still exist for lower-level or non-AI use cases:

```bash
st wt c ui-polish --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux
```

Use `--run` when you want a non-agent launcher such as an editor.

## Why The Lanes Stay First-Class

When stax creates a new branch for the lane, it writes normal stax metadata. That is why the lane participates in normal stax flows:

- `st ls` shows the branch in the stack
- `st restack`, `st sync --restack`, and `st wt rs` can reason about it
- undo/redo still operate on the branch history
- the stack TUI and the worktree dashboard both understand the lane

Tracking nuance:

- a new lane created by `st wt c foo` is stax-managed
- an already tracked branch stays managed when you open a worktree for it
- an existing plain Git branch gets a worktree, but it stays unmanaged until `st branch track`

## Recommended Loop

```bash
# Start a scratch lane fast
st lane flaky-tests "fix flaky tests"

# Re-enter it later
st lane flaky-tests

# Or browse your active lanes first
st lane

# Trunk moved while the lane was in flight
st wt rs

# Inspect operational state
st wt ll

# Clean up after merge
st wt cleanup --dry-run
st wt rm flaky-tests --delete-branch
```

## Setup Once

Install shell integration if you want `create` and `go` to move the parent shell automatically:

```bash
st shell-setup --install
```

For shell integration details, cleanup semantics, hooks, dashboard behavior, and Windows limitations, use the canonical [Worktrees](../worktrees/index.md) page.
