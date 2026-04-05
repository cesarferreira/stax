# AI Worktree Lanes

`st lane` is the high-level shortcut for running parallel AI coding sessions on top of `st wt`.

Each lane is a real Git worktree backed by a real branch, so the work stays visible to stax instead of disappearing into a pile of ad-hoc terminals.

This page is the dedicated guide for the AI workflow. For the full `st worktree` / `st wt` command surface, cleanup semantics, shell integration, dashboard controls, and config, see [Worktrees](../worktrees/index.md).

## What `st lane` Actually Does

`st lane <name> [prompt]` is the fast path for:

1. finding or creating a named worktree lane
2. resolving the branch that lane should use
3. launching your configured AI agent inside it
4. preferring tmux when available so you can resume the lane later

That gives you isolated working directories **without** giving up normal stax behavior.

A lane created by stax is still a normal tracked branch, so it can participate in:

- `st ls`
- `st restack`
- `st sync --restack`
- `st wt rs`
- undo / redo flows
- normal worktree cleanup

## Why This Is Better Than "Just Open Another Terminal"

AI lanes are useful when you want multiple active coding threads at once, but still want the repo to stay understandable.

With `st lane`, each task gets:

- its own worktree
- its own branch
- optional tmux session reuse
- visibility in the rest of stax
- a clean path back into the lane later

That means you can do things like:

- fix flaky tests in one lane
- address PR comments in another
- poke at a risky refactor in a third
- restack them all after trunk moves
- remove the finished ones cleanly

## The Main Flows

### 1. Start a new lane fast

```bash
st lane flaky-tests "stabilize the flaky test suite"
```

If the lane does not exist yet, stax will:

- create a worktree for it
- create a branch when needed
- write normal stax metadata for new managed branches
- launch your configured AI agent in that lane

### 2. Re-enter an existing lane

```bash
st lane flaky-tests
```

If the lane already exists, stax reuses it instead of creating a duplicate.

### 3. Browse lanes interactively

```bash
st lane
```

With no lane name, stax opens an interactive picker of **stax-managed** lanes.

From there you can:

- jump into an existing lane
- create a new lane
- filter the list fuzzily
- see which lane is current
- see concise tmux and status columns before entering

If there are no managed lanes yet, stax falls back to prompting for a new one.

## Tmux Behavior

The tmux behavior is one of the most useful parts of `st lane`, and it is worth being explicit about.

When tmux is available, `st lane <name> [prompt]` defaults to tmux-backed launches.

### Existing tmux session + no prompt

```bash
st lane review-pass
```

If the lane already has a tmux session and you do **not** pass a new prompt, stax reattaches / switches to that existing session.

This is the "resume exactly where I left off" path.

### Existing tmux session + new prompt

```bash
st lane review-pass "address the latest review comments"
```

If the lane already has a tmux session **and** you pass a new prompt, stax opens a **new tmux window** in that same session and starts the agent there.

This is the "same lane, fresh subtask" path.

### No tmux available

If tmux is unavailable, stax falls back to launching directly in the lane and tells you it is doing that.

### Force direct terminal mode

```bash
st lane review-pass --no-tmux
```

Use `--no-tmux` when you want the agent to launch directly in the terminal even if tmux is installed.

### Override the derived tmux session name

```bash
st lane review-pass --tmux-session pr-review
```

`--tmux-session` only works with an explicit lane name.

## Agent Selection

Supported `--agent` values are:

- `claude`
- `codex`
- `gemini`
- `opencode`

Examples:

```bash
st lane api-tests --agent gemini
st lane api-tests --agent opencode --model opencode/gpt-5.1-codex
st lane review-pass --agent codex "address the open PR comments"
```

Notes:

- `--model` requires `--agent`
- if you omit `--agent`, stax uses your configured default agent
- the optional `[prompt]` is passed through to the launched agent

## A Realistic Daily Workflow

```bash
# Start a few parallel lanes
st lane auth-refresh "fix token refresh edge cases"
st lane flaky-tests "stabilize the flaky test suite"
st wt c ui-polish --run "cursor ."
st lane review-pass "address the open PR comments"

# They are still visible as branches
st ls

# Jump back into one lane later
st lane flaky-tests

# Or browse first
st lane

# Trunk moved while those sessions were in flight
st wt rs

# Check operational state
st wt ll

# Clean up after merge
st wt cleanup --dry-run
st wt rm auth-refresh --delete-branch
```

## Managed vs Unmanaged Matters

The docs around lanes are much easier to understand once you separate **worktree existence** from **stax management**.

### Managed lanes

A lane is stax-managed when its branch has stax metadata.

Common cases:

- a new lane created by stax is managed
- an already tracked stax branch stays managed when opened as a lane

Managed lanes are the ones that fully participate in stack-aware flows like:

- `st ls`
- `st restack`
- `st sync --restack`
- `st wt rs`
- cleanup of merged managed lanes

### Unmanaged worktrees

If you open an existing plain Git branch as a worktree, stax can still navigate to it and list it as a worktree, but it stays unmanaged until you explicitly track it.

So the practical rule is:

- **all lanes are worktrees**
- **only managed lanes are first-class stax stack entries**

If you need the lower-level tracking details, see [Worktrees](../worktrees/index.md).

## The Interactive Picker

Bare `st lane` is not just a shortcut to a list. It is a specific lane-oriented entry flow.

The picker shows stax-managed lanes with columns for:

- lane name
- branch
- tmux state
- status summary

The tmux column uses concise labels:

- `attached`: session exists and has attached clients
- `ready`: session exists and can be resumed
- `new`: tmux is available but no session exists yet
- `off`: tmux is unavailable

The status column compresses lane state into a small summary such as:

- `clean`
- `dirty`
- `rebasing`
- conflict-related states

And because this flow is interactive, it requires a real terminal when you run `st lane` with no name.

## When To Use `st lane` vs `st wt`

Use `st lane` when the thing you want is:

- "make or resume an AI lane"
- "launch my configured agent there"
- "reuse tmux if possible"

Use lower-level `st wt` commands when you want more manual control or a non-AI launcher.

Examples:

```bash
st wt c ui-polish --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux
```

That split is useful:

- `st lane` = opinionated AI shortcut
- `st wt` = general worktree control plane

## Setup Once

Install shell integration if you want lane creation and navigation to move the parent shell automatically:

```bash
st shell-setup --install
```

After shell integration is installed:

- `st wt c ...` can move the parent shell into the new lane
- `st wt go ...` can move the parent shell into the selected lane
- `st lane ...` can move the parent shell into the selected lane

For shell integration details, cleanup semantics, hooks, dashboard behavior, and Windows limitations, use the canonical [Worktrees](../worktrees/index.md) page.
