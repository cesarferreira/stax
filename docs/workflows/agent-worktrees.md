# Agent Worktrees

`st agent` lets you spin up isolated Git worktrees for parallel AI agents (Cursor, Codex, Aider, etc.) while keeping every branch visible and manageable inside stax.

Each agent gets its own directory and branch. The main checkout stays clean. Stax metadata, restack, undo, and the TUI all work across agent worktrees automatically.

## Quick start

```bash
# Create a worktree + stacked branch and open it in Cursor
st agent create "Add dark mode" --open-cursor

# Reattach to a closed agent session
st agent open add-dark-mode

# See all active worktrees
st agent list

# Restack all agent branches at once
st agent sync

# Remove a finished worktree (optionally delete the branch too)
st agent remove add-dark-mode --delete-branch

# Clean up dead entries
st agent prune
```

## Real-world example: running 3 agents in parallel

Say you have a feature branch and want Codex, Claude Code, and OpenCode each tackling a different sub-task simultaneously — without them touching each other's files.

### Step 1 — spin up three isolated worktrees

```bash
st agent create "Add dark mode" --open-codex
st agent create "Fix auth token refresh" --open-cursor
st agent create "Write API integration tests"
```

Each command creates an isolated directory under `.stax/trees/` with its own stacked branch. Your main checkout is untouched.

```
main
 └── feature/my-feature                    ← your main checkout
      ├── add-dark-mode                     ← Codex working here
      ├── fix-auth-token-refresh            ← Cursor / Claude Code working here
      └── write-api-integration-tests       ← OpenCode / terminal working here
```

### Step 2 — point each agent at its directory

- **Codex** opened automatically via `--open-codex`
- **Claude Code**: `claude` inside `.stax/trees/fix-auth-token-refresh`
- **OpenCode**: `opencode` inside `.stax/trees/write-api-integration-tests`

Each agent sees only its own branch. They cannot conflict with each other.

### Step 3 — check on things while agents run

```bash
st agent list   # all three worktrees, their branches, existence status
st status       # all three branches appear in the normal stack tree
```

### Step 4 — come back later and reattach

```bash
st agent open                           # fuzzy picker
st agent open fix-auth-token-refresh    # or by name
```

### Step 5 — trunk moved while agents were running

```bash
git pull
st agent sync   # restacks all three branches at once
```

### Step 6 — review and submit each branch normally

```bash
st checkout add-dark-mode
st submit
```

### Step 7 — clean up

```bash
st agent remove add-dark-mode --delete-branch
st agent remove fix-auth-token-refresh --delete-branch
st agent remove write-api-integration-tests --delete-branch
```

> **What stax does not do:** it doesn't talk to the agents or assign them tasks — that's still you. What it solves is directory isolation, branch tracking, restack-after-trunk-moves, and the "where did I leave that session" problem that makes running parallel agents messy in practice.

## How it works

```
st agent create "Add dark mode" --open-cursor
  │
  ├─ slugifies title → "add-dark-mode"
  ├─ creates branch (respects your branch.format config)
  ├─ git worktree add .stax/trees/add-dark-mode <branch>
  ├─ writes stax metadata (parent branch + revision)
  ├─ registers in .git/stax/agent-worktrees.json
  ├─ adds .stax/trees/ to .gitignore
  └─ opens cursor -n .stax/trees/add-dark-mode
```

The registry lives at `.git/stax/agent-worktrees.json` and is never committed.

## Commands

### `st agent create <title>`

| Flag | Description |
|------|-------------|
| `--base <branch>` | Base branch (defaults to current) |
| `--stack-on <branch>` | Same as `--base` |
| `--open` | Open in default editor after creation |
| `--open-cursor` | Open in Cursor |
| `--open-codex` | Open in Codex |
| `--no-hook` | Skip `post_create_hook` for this run |

The title is slugified into both the folder name and the branch name. For example, `"Add dark mode system"` becomes folder `add-dark-mode-system` and branch `add-dark-mode-system` (or `cesar/add-dark-mode-system` if your `branch.format` includes `{user}`).

### `st agent open [name]` / `st agent attach [name]`

Reopens a registered worktree in the configured editor. If no name is given, an interactive fuzzy picker is shown.

### `st agent list` / `st agent ls`

Prints a table of all registered worktrees with their branch, existence status, and the open command.

### `st agent register`

Registers the current directory as a managed agent worktree. Useful when you created a worktree manually and want stax to track it.

### `st agent remove [name]`

| Flag | Description |
|------|-------------|
| `--force` | Force removal even if the worktree has uncommitted changes |
| `--delete-branch` | Also delete the branch and its stax metadata |

### `st agent prune`

Removes registry entries whose worktree paths no longer exist, then runs `git worktree prune` to clean up Git's internal state.

### `st agent sync`

Restacks every registered agent worktree by running `st restack --all` inside each one. Reports a per-worktree pass/fail summary.

## TUI integration

When agent worktrees are registered, the TUI shows an "Agents" panel at the bottom of the left column. Each row shows the worktree name, short branch name, and whether the path still exists.

## Editor auto-detection

Priority for `--open` / `open` in `st agent open`:

1. `--open-cursor` flag → `cursor -n <path>`
2. `--open-codex` flag → `codex <path>`
3. `config.agent.default_editor` (if not `auto`)
4. Auto-detect: `cursor` if on PATH, else `code`

## Configuration

```toml
# ~/.config/stax/config.toml
[agent]
worktrees_dir = ".stax/trees"    # relative to repo root
default_editor = "auto"          # "auto" | "cursor" | "codex" | "code"
post_create_hook = "npm install" # optional: run in new worktree after creation
```

## Editor slash-command recipes

Ready-to-import slash command recipes live in `examples/`:

| File | Editor | Command |
|------|--------|---------|
| [`examples/cursor/stax-new-agent.md`](../../examples/cursor/stax-new-agent.md) | Cursor | `st agent create "{{input}}" --open-cursor` |
| [`examples/codex/stax-new-agent.md`](../../examples/codex/stax-new-agent.md) | Codex | `st agent create "{{input}}" --open-codex` |
| [`examples/generic/stax-new-agent.md`](../../examples/generic/stax-new-agent.md) | Any (auto-detect) | `st agent create "{{input}}" --open` |

Each creates a stacked branch + worktree and opens it in the target editor in one step.

## Relationship to `st undo`

Agent worktrees use standard stax branch metadata, so `st undo` and `st redo` work as normal. The branch operations recorded in `.git/stax/ops/` cover create, restack, and any other operations you run inside the worktree.
