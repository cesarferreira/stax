# Agent Worktrees

`stax agent` lets you spin up isolated Git worktrees for parallel AI agents (Cursor, Codex, Aider, etc.) while keeping every branch visible and manageable inside stax.

Each agent gets its own directory and branch. The main checkout stays clean. Stax metadata, restack, undo, and the TUI all work across agent worktrees automatically.

## Quick start

```bash
# Create a worktree + stacked branch and open it in Cursor
stax agent create "Add dark mode" --open-cursor

# Reattach to a closed agent session
stax agent open add-dark-mode

# See all active worktrees
stax agent list

# Restack all agent branches at once
stax agent sync

# Remove a finished worktree (optionally delete the branch too)
stax agent remove add-dark-mode --delete-branch

# Clean up dead entries
stax agent prune
```

## How it works

```
stax agent create "Add dark mode" --open-cursor
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

### `stax agent create <title>`

| Flag | Description |
|------|-------------|
| `--base <branch>` | Base branch (defaults to current) |
| `--stack-on <branch>` | Same as `--base` |
| `--open` | Open in default editor after creation |
| `--open-cursor` | Open in Cursor |
| `--open-codex` | Open in Codex |
| `--no-hook` | Skip `post_create_hook` for this run |

The title is slugified into both the folder name and the branch name. For example, `"Add dark mode system"` becomes folder `add-dark-mode-system` and branch `add-dark-mode-system` (or `cesar/add-dark-mode-system` if your `branch.format` includes `{user}`).

### `stax agent open [name]` / `stax agent attach [name]`

Reopens a registered worktree in the configured editor. If no name is given, an interactive fuzzy picker is shown.

### `stax agent list` / `stax agent ls`

Prints a table of all registered worktrees with their branch, existence status, and the open command.

### `stax agent register`

Registers the current directory as a managed agent worktree. Useful when you created a worktree manually and want stax to track it.

### `stax agent remove [name]`

| Flag | Description |
|------|-------------|
| `--force` | Force removal even if the worktree has uncommitted changes |
| `--delete-branch` | Also delete the branch and its stax metadata |

### `stax agent prune`

Removes registry entries whose worktree paths no longer exist, then runs `git worktree prune` to clean up Git's internal state.

### `stax agent sync`

Restacks every registered agent worktree by running `stax restack --all` inside each one. Reports a per-worktree pass/fail summary.

## TUI integration

When agent worktrees are registered, the TUI shows an "Agents" panel at the bottom of the left column. Each row shows the worktree name, short branch name, and whether the path still exists.

## Editor auto-detection

Priority for `--open` / `open` in `stax agent open`:

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
| [`examples/cursor/stax-new-agent.md`](../../examples/cursor/stax-new-agent.md) | Cursor | `stax agent create "{{input}}" --open-cursor` |
| [`examples/codex/stax-new-agent.md`](../../examples/codex/stax-new-agent.md) | Codex | `stax agent create "{{input}}" --open-codex` |
| [`examples/generic/stax-new-agent.md`](../../examples/generic/stax-new-agent.md) | Any (auto-detect) | `stax agent create "{{input}}" --open` |

Each creates a stacked branch + worktree and opens it in the target editor in one step.

## Relationship to `stax undo`

Agent worktrees use standard stax branch metadata, so `stax undo` and `stax redo` work as normal. The branch operations recorded in `.git/stax/ops/` cover create, restack, and any other operations you run inside the worktree.
