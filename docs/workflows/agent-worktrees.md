# AI worktree lanes

`st lane` runs parallel AI coding sessions, each on its own Git worktree and branch, all tracked as normal stax stack entries. No hidden scratch directories, no lost work.

For the full `st worktree` / `st wt` command surface and cleanup semantics, see [Worktrees](../worktrees/index.md).

## What `st lane` does

`st lane <name> [prompt]`:

1. Finds or creates a named worktree lane
2. Resolves the branch for that lane (creates one if needed, writes stax metadata for new managed branches)
3. Launches your configured AI agent inside it
4. Prefers tmux when available so you can resume later

A lane is a real tracked branch. It participates in `st ls`, `st restack`, `st sync --restack`, `st wt rs`, undo/redo, and normal worktree cleanup.

## The main flows

### Start a new lane

```bash
st lane flaky-tests "stabilize the flaky test suite"
```

### Re-enter an existing lane

```bash
st lane flaky-tests
```

If the lane exists, stax reuses it.

### Browse lanes interactively

```bash
st lane
```

Opens a picker of stax-managed lanes with columns for lane, branch, tmux state, and status (`clean`, `dirty`, `rebasing`, conflict states). Falls back to prompting for a new lane if none exist. Requires a TTY.

## A realistic daily flow

```bash
# Start a few parallel lanes
st lane auth-refresh "fix token refresh edge cases"
st lane flaky-tests  "stabilize the flaky test suite"
st wt c ui-polish --run "cursor ."
st lane review-pass  "address the open PR comments"

# All visible as branches
st ls

# Jump back into a lane
st lane flaky-tests

# Or browse first
st lane

# Trunk moved while sessions were in flight
st wt rs

# Check operational state, then clean up merged lanes
st wt ll
st wt cleanup --dry-run
st wt rm auth-refresh --delete-branch
```

## tmux behavior

When tmux is available, `st lane <name> [prompt]` defaults to tmux-backed launches.

| Invocation | Behavior |
|---|---|
| `st lane review-pass` (existing session) | Reattach / switch to the existing session |
| `st lane review-pass "new task"` (existing session) | Open a **new tmux window** in that session and run the agent there |
| `st lane review-pass` (no tmux available) | Launch directly in the terminal |
| `st lane review-pass --no-tmux` | Force direct terminal mode |
| `st lane review-pass --tmux-session pr-review` | Override the derived session name |

Once a lane is running, attach from any shell:

```bash
tmux attach -t <lane-name>
```

## Agent selection

Supported `--agent` values: `claude`, `codex`, `gemini`, `opencode`.

```bash
st lane api-tests   --agent gemini
st lane api-tests   --agent opencode --model opencode/gpt-5.1-codex
st lane review-pass --agent codex "address the open PR comments"
```

- `--model` requires `--agent`
- Without `--agent`, stax uses your configured default
- The optional `[prompt]` is passed through to the agent

## `--yolo` — auto-accept permission prompts

For well-scoped work in an isolated lane, let the agent run autonomously:

| Agent | Injected flag |
|---|---|
| `claude` | `--dangerously-skip-permissions` |
| `codex` | `--dangerously-bypass-approvals-and-sandbox` |
| `gemini` | `--yolo` |
| `opencode` | *not supported — use `--agent-arg` instead* |

```bash
st lane fix-flaky --agent claude --yolo "stabilize the flaky test suite"
st lane refactor  --agent codex  --yolo "split the auth module"
```

`--yolo` requires `--agent`. It's a no-op when reattaching to an existing tmux session — pass a new prompt to open a fresh agent window where the flag takes effect.

**Use with care.** The lane's worktree is isolated, but everything the agent does still runs as you.

## `--agent-arg` — extra agent flags

Repeatable. Values forward verbatim after the model and yolo flags:

```bash
st lane big-refactor --agent claude --agent-arg=--verbose "pull apart the auth module"
```

Do not pass `--model` via `--agent-arg` — stax handles that via `--model`. Like `--yolo`, ignored when reattaching.

## VS Code / Cursor integration

Keep your existing VS Code window aware of every new lane as an extra folder in the Explorer:

```toml
# ~/.config/stax/config.toml
[worktree.hooks]
post_start = "code --add ."   # fires when a lane is freshly created
post_go    = "code --add ."   # fires when re-entering an existing lane
# For Cursor, replace both with "cursor --add ."
```

Both hooks are needed: `post_start` runs on first creation, `post_go` every re-entry. `code --add .` is idempotent. Both hooks run in the background, so they don't block the agent launch.

After configuring:

```bash
st lane fix-flaky --agent claude "stabilize the flaky test suite"
```

- stax creates the worktree and launches the agent in tmux
- your VS Code window grows a new folder pointing at the lane
- each lane has its own file tree, terminal tabs, and git state while sharing one VS Code process

### Persist across restarts

`code --add` needs a workspace file to remember folders. Create one **outside the repo**:

```json
// ~/Documents/code-workspaces/<repo>.code-workspace
{ "folders": [{ "path": "/absolute/path/to/your/repo" }] }
```

Open VS Code via `code /path/to/<repo>.code-workspace` (not by opening the folder directly). Now every `code --add` writes into that workspace file and closing/reopening restores the full multi-lane layout.

### Caveats

- `code` / `cursor` must be on `$PATH`. On macOS: run **Shell Command: Install 'code' command in PATH** from the Command Palette once. Background hooks swallow stderr, so a missing binary fails silently — to debug, temporarily switch to `post_create` (blocking) to surface the error.
- Designed for local worktrees — over SSH/remote, `code --add .` adds as a local path in your controlling window, which is almost never what a remote user wants.
- VS Code and Cursor share the `--add` flag but only the most recently active window receives the folder. Pick one.

## Managed vs unmanaged

A lane is **managed** when its branch has stax metadata. New lanes created by stax, and existing tracked stax branches opened as lanes, are managed. Plain Git branches opened as worktrees stay **unmanaged** until you run `st branch track`.

Only managed lanes fully participate in:

- `st ls`
- `st restack` / `st sync --restack`
- `st wt rs`
- cleanup of merged lanes

All lanes are worktrees; only managed lanes are first-class stax stack entries.

## When to use `st lane` vs `st wt`

- `st lane` — opinionated AI shortcut; launches your configured agent, prefers tmux
- `st wt` — general worktree control plane; use for non-AI launchers or manual control

```bash
st wt c ui-polish --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux
```

## Setup

```bash
st setup --yes               # shell integration + skills + auth in one go
st setup --install-skills    # skip auth/skills prompt, accept skills
```

After shell integration, `st lane ...`, `st wt c ...`, and `st wt go ...` can `cd` the parent shell into the lane.

## Related

- [Worktrees (`st wt`)](../worktrees/index.md)
- [Multi-worktree behavior](multi-worktree.md)
- [Configuration](../configuration/index.md)
