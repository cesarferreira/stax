# Worktree Lanes For AI

The old `st agent` experiment is gone. The stax-native way to run parallel AI sessions is normal `st wt` worktrees plus `--agent`.

That keeps the workflow familiar to existing stax users:

- `wt c` creates a lane
- `wt go` jumps back into a lane
- `wt ls` stays compact
- `wt ll` shows richer status
- `wt rs` restacks stax-managed lanes

## Quick start

```bash
# Create a random lane and start Codex there
st wt c --agent codex -- "fix flaky tests"

# Create or reuse a named lane
st wt c auth-refresh

# Jump back into an existing lane and launch Claude
st wt go auth-refresh --agent claude

# Rich status view
st wt ll

# Safe cleanup
st wt prune
st wt rm auth-refresh --delete-branch
```

## Why this shape

stax already had strong verbs for this workflow:

- `create` means “make the thing and take me there”
- `go` means “jump to the existing thing”
- `ls` means “show me the inventory”
- `ll` means “show me the richer view”

The AI behavior is an option on top of that, not a separate subsystem.

## Random no-arg creation

`st wt c` with no arguments generates a funny two-word slug from bundled word lists and uses it for the lane name:

```bash
st wt c
# creates something like:
#   .worktrees/cheeky-bagel
#   branch cheeky-bagel (or your configured branch.format variant)
```

This is the fastest way to spin up an isolated scratch lane for an agent.

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

## Base branch behavior

For new branches:

- `--from <branch>` explicitly sets the base branch
- otherwise, if the current branch is already tracked by stax, the new lane stacks on the current branch
- otherwise, the new lane starts from trunk

When stax creates a new branch for the lane, it writes normal stax branch metadata, so restack/sync/undo continue to work as expected.

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

After that, `st wt c` and `st wt go` change the parent shell directory directly, and `st wt rm` can safely relocate the shell before removing the current worktree.

## Hooks

Worktree hooks live under `[worktree.hooks]` in `~/.config/stax/config.toml`:

```toml
[worktree]
root_dir = ".worktrees"

[worktree.hooks]
post_create = ""
post_start = ""
post_go = ""
pre_remove = ""
post_remove = ""
```

Use these for lightweight local automation such as dependency bootstrap or editor/session setup.
