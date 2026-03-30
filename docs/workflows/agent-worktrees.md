# Worktree Lanes For AI

`st wt` lets you run multiple AI coding sessions in parallel while keeping them inside stax's normal branch model. In an interactive terminal, bare `st wt` opens the worktree dashboard so you can browse lanes and re-enter tmux-backed sessions quickly.

This is the important part: these lanes are not a separate subsystem and they are not hidden scratch directories. When stax creates the branch for a lane, it writes normal stax metadata, so the lane behaves like a first-class branch in your stack.

That means you can:

- see it in `st ls`
- restack it safely when trunk or a parent moves
- jump back into it with `st wt go`
- inspect it with `st wt ll`
- clean it up with `st wt rm`
- keep several lanes alive at once without them touching the same checkout

## Why this is powerful

Raw `git worktree` gives you extra directories. This feature gives you a parallel workflow:

- create a lane and immediately start Claude/Codex/Gemini/OpenCode inside it
- keep each session isolated to its own working tree and branch
- let those branches stay visible to stax instead of disappearing into ad-hoc directories
- recover cleanly when trunk moves by restacking all managed lanes together
- come back hours later and resume any lane without remembering where you left it

If you are working with multiple coding tools at once, this is the difference between "a pile of terminals" and "several active branches that stax still understands."

## The workflow in one example

```bash
# Start three parallel lanes
st wt c auth-refresh --agent claude -- "fix token refresh edge cases"
st wt c flaky-tests --agent codex -- "stabilize the flaky test suite"
st wt c ui-polish --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"

# They are normal stax branches
st ls

# Jump back into any lane later
st wt go flaky-tests --agent codex
st wt go review-pass --agent codex --tmux

# Trunk moved while those sessions were in flight
st wt rs

# See which lanes are dirty / rebasing / managed
st wt ll

# Remove finished work
st wt rm auth-refresh --delete-branch
```

## What `st wt c` actually gives you

`st wt c` is intentionally convenience-first:

- with no name, it creates a new lane with a random funny slug
- with a new name, it creates a new branch + worktree and takes you there
- with an existing branch, it creates a worktree for that branch
- with an existing worktree target, it reuses it instead of duplicating it

So the command is less "make a raw worktree" and more "make sure this lane exists and put me in it."

## First-class branch behavior

When stax creates a new branch for the lane, it writes normal branch metadata. That is why the lane participates in the usual stax flows:

- `st ls` shows the branch in the stack
- `st restack`, `st sync --restack`, and `st wt rs` can reason about it
- undo/redo still operate on the branch history
- the TUI keeps showing the stack while the separate worktrees panel shows the linked directories

Tracking nuance:

- new lane name via `st wt c foo`: tracked by stax
- existing already-tracked branch via `st wt c some-branch`: still tracked
- existing plain Git branch via `st wt c some-branch`: worktree exists, but the branch stays untracked until you run `st branch track`

## Quick start

```bash
# Fastest possible scratch lane
st wt c --agent codex -- "fix flaky tests"

# Create or reuse a named lane
st wt c auth-refresh

# Re-enter a lane and relaunch the tool there
st wt go auth-refresh --agent claude
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux

# Rich status + cleanup
st wt ll
st wt prune
st wt rm auth-refresh --delete-branch
```

## Why the command shape feels native to stax

stax already had strong verbs for this workflow:

- `create` means "make the thing and take me there"
- `go` means "jump to the existing thing"
- `ls` means "show me the inventory"
- `ll` means "show me the richer view"

The AI launch is an option on top of those verbs, not a separate command family.

## Random no-arg creation

`st wt c` with no arguments generates a funny two-word slug from bundled word lists and uses it for the lane name:

```bash
st wt c
# creates something like:
#   ~/.stax/worktrees/stax/cheeky-bagel
#   branch cheeky-bagel (or your configured branch.format variant)
```

This is the fastest way to spin up an isolated scratch lane.

## Agent launch

`--agent` launches a supported interactive CLI inside the target worktree after creation or navigation.

```bash
st wt c api-tests --agent codex -- "write the missing integration tests"
st wt go api-tests --agent gemini
st wt go api-tests --agent opencode -- "--resume"
```

Supported values:

- `claude`
- `codex`
- `gemini`
- `opencode`

Use `--model` with `--agent` when you want an explicit override.

Use `--run` when you want an arbitrary launcher instead:

```bash
st wt go api-tests --run "cursor ."
```

Add `--tmux` if you want the lane to create or attach to a tmux session named after the worktree:

```bash
st wt c api-tests --agent codex --tmux -- "write the missing integration tests"
st wt go api-tests --agent codex --tmux
```

Behavior:

- first entry creates the tmux session and launches the requested command there
- later entries attach to the existing session instead of relaunching the command
- inside an existing tmux client, stax switches to the lane's session instead of nesting tmux

## Base branch behavior

For new branches:

- `--from <branch>` explicitly sets the base branch
- otherwise, if the current branch is already tracked by stax, the new lane stacks on the current branch
- otherwise, the new lane starts from trunk

## Status views

`st wt ls` stays intentionally simple:

```text
NAME   BRANCH   PATH
```

`st wt ll` adds the richer operational state:

- managed vs unmanaged
- dirty state
- rebase/merge/conflict state
- optional marker
- locked/prunable state
- stack parent/base

## Restacking lanes

`st wt rs` restacks only stax-managed worktrees. It skips:

- detached worktrees
- stale prunable entries
- worktrees created outside stax that do not have branch metadata

This keeps third-party or ad-hoc worktrees visible without making `restack` dangerous.

## Prune vs remove

Use `st wt rm` when you want to delete a live worktree.

Use `st wt prune` when Git still remembers a dead worktree path that no longer exists on disk. `prune` is safe housekeeping only; it does not bulk-delete merged branches or guess what should disappear.

## Shell integration

Install once:

```bash
st shell-setup --install
```

This writes a static shell snippet under `~/.config/stax/` and sources it from your shell config instead of executing `stax` during shell startup. When that generated file already exists, newer `stax` versions refresh it automatically on later runs, so your shell config can stay as the single `source ...` line.

After that, `st wt c` and `st wt go` change the parent shell directory directly, and `st wt rm` can safely relocate the shell before removing the current worktree.

!!! note "Windows"
    Shell integration and `--tmux` require a Unix shell (bash/zsh/fish) and tmux. On Windows, worktree commands work but auto-`cd` and tmux session management are unavailable. See [Windows notes](../reference/windows.md).

## Hooks

Worktree hooks live under `[worktree.hooks]` in `~/.config/stax/config.toml`:

```toml
[worktree]
# Leave unset/empty for the default external root (~/.stax/worktrees/<repo>)
# root_dir = ""
# Or opt back into repo-local lanes:
# root_dir = ".worktrees"

[worktree.hooks]
post_create = ""
post_start = ""
post_go = ""
pre_remove = ""
post_remove = ""
```

Use these for lightweight local automation such as dependency bootstrap or editor/session setup.
