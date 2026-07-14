# Worktrees

stax has two related worktree stories:

- **Repo-wide awareness** — `restack`, `sync`, `cascade`, and metadata operations run in the right linked checkout automatically. See [Multi-worktree behavior](../workflows/multi-worktree.md).
- **The `st worktree` command family** — create, enter, inspect, and clean up worktree lanes. This page is the canonical guide.

For the parallel AI workflow built on top, see [AI worktree lanes](../workflows/agent-worktrees.md).

## Quick start

```bash
# Open the interactive dashboard
st wt

# Create a fresh lane with a random name
st wt c

# Create or reuse a named lane
st wt c payments-api

# Check out a fetched remote branch into a worktree
st wt c origin/payments-api

# Start a new lane from an explicit base
st wt c payments-api --from main

# Jump back into a lane
st wt go payments-api

# Make this lane the branch checked out in the main worktree
st wt promote

# Inventory
st wt ls
st wt ll

# Print the absolute path (scripting)
st wt path payments-api

# Remove / clean up
st wt rm payments-api
st wt cleanup --dry-run
st wt prune
st wt rs
```

## How `create` and `go` resolve targets

`st wt c [name]` is convenience-first:

- no `name` → random two-word lane slug
- `name` matches an existing worktree → reuse it
- `name` matches an existing branch → create a worktree for that branch
- `name` matches a fetched branch on the configured remote → create a local tracking branch and worktree
- otherwise → create a new branch and a new worktree

`st wt go [name]` only opens existing worktrees. With no `name`, it opens a picker.

Selectors accepted by `go`, `path`, `rm`, and reuse paths in `create`:

- worktree name
- full branch name
- unique branch suffix (e.g. `payments-api` matching `cesar/payments-api`)
- fetched remote branch name or configured-remote-qualified name (e.g. `payments-api` or `origin/payments-api`)
- absolute worktree path

### Base branch rules for new lanes

- fetched remote branches keep their remote tip and upstream tracking branch
- `--from <branch>` explicitly sets the base
- otherwise if the current branch is tracked by stax → new lane stacks on current
- otherwise → new lane starts from trunk

`--pick` chooses an existing local branch interactively. `--name <label>` lets the worktree directory label differ from the branch name. `--no-verify` skips worktree hooks for that command.

## Command map

| Command | Aliases | What it does |
|---|---|---|
| `st worktree` | `st wt` | Interactive dashboard (TTY) or worktree help |
| `st wt c [name]` | `st worktree create`, `st wtc` | Create or reuse a lane; supports `--from`, `--pick`, `--name`, `--agent`, `--run`, `--tmux`, `--no-verify` |
| `st lane [name] [prompt]` | | Fast AI-lane entrypoint (see [AI lanes](../workflows/agent-worktrees.md)) |
| `st wt go [name]` | `st worktree go`, `st wtgo` | Enter an existing worktree; supports `--agent`, `--run`, `--tmux`, `--no-verify` |
| `st wt ls` | `st worktree list`, `st w`, `st wtls` | Compact `NAME / BRANCH / PATH` inventory (`--json`) |
| `st wt ll` | `st worktree ll`, `st wtll` | Rich status view with managed/dirty/rebase/conflict/marker/prunable state (`--json`) |
| `st wt path <name>` | `st worktree path <name>` | Print absolute path |
| `st wt rm [name]` | `st worktree remove`, `st wtrm` | Remove one worktree (`wt rm` removes current); supports `-f/--force`, `--delete-branch` |
| `st wt promote` | `st worktree promote` | Retire the current linked worktree and check its branch out in the main worktree |
| `st wt prune` | `st worktree prune`, `st wtprune` | Remove stale `git worktree` bookkeeping only |
| `st wt cleanup` | `st worktree cleanup`, `st wt clean` | Prune + bulk-remove safe detached/merged lanes (`--dry-run`, `--yes`, `-f`) |
| `st wt restack` | `st worktree restack`, `st wtrs`, `st wt rs` | Restack all stax-managed worktrees |

## Launch other tools inside a lane

```bash
st wt c ui-polish  --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux
```

- `--agent` supports `claude`, `codex`, `gemini`, `opencode`
- `--model` requires `--agent`
- `--run` and `--agent` are mutually exclusive
- anything after `--` is passed through to the agent/command
- `--tmux` creates/reuses a session named after the worktree unless `--tmux-session` overrides

If you run `create` / `go` without shell integration and without a launcher, stax prints the `cd` command to copy.

## Managed vs unmanaged

`st wt ls` shows every Git worktree, including ones created outside stax. The important distinction is whether the branch has stax metadata:

- new branches created by `st wt c foo` → **managed**
- existing tracked branches opened as lanes → stay **managed**
- fetched remote branches opened as local tracking branches → **unmanaged** until `st branch track`
- existing plain Git branches opened as worktrees → **unmanaged** until `st branch track`

Managed lanes behave like first-class stax branches: they show in `st ls`, participate in restack/sync/undo, and are targeted by `st wt restack`. Unmanaged lanes still show up in `ls`, `ll`, `go`, `rm`, and `prune`, but stax keeps history-rewriting operations conservative.

## Dashboard

Run `st wt` in an interactive terminal.

- Left pane: all Git worktrees, including unmanaged
- Right pane: branch, base, path, status, tmux session details

| Key | Action |
|---|---|
| `Enter` | Attach / switch to the tmux session for the selected worktree |
| `c` | Create a lane and open it in tmux |
| `d` | Remove the selected worktree |
| `R` | Restack all stax-managed worktrees |
| `?` | Show help |
| `q` / `Esc` | Quit |

The dashboard is a control plane, not an embedded shell. The stack/patch TUI is documented separately in [Interactive TUI](../interface/tui.md).

## Shell integration

`st setup` is the one-shot onboarding command for shell integration, AI skills, and auth:

```bash
st setup                    # full interactive onboarding
st setup --yes              # accept defaults, install skills, import auth from gh
st setup --install-skills   # install shell integration + skills without prompting
st setup --skip-skills      # install shell integration without the skills prompt
st setup --auth-from-gh     # install shell integration and import auth from gh
st setup --skip-auth        # install shell integration without auth onboarding
st setup --print            # print the snippet for manual install
```

After installation:

- `st wt c ...` moves the parent shell into the new lane
- `st wt go ...` moves the parent shell into the selected lane
- `st wt promote` moves the parent shell to the main worktree after a successful handoff
- `st lane ...` moves the parent shell into the selected lane
- `st wt rm` (no arg) can relocate the shell before removing the current worktree
- `sw <name>` becomes a quick alias for `st wt go <name>`

Supports `bash`, `zsh`, and `fish`.

!!! note "Windows"
    On Windows, worktree commands work but the parent shell cannot auto-`cd`, `sw` is unavailable, and tmux integration is not supported. Manually `cd` to the printed path after `st wt c`, `st wt go`, or `st wt promote`. See [Windows notes](../reference/windows.md).

## Promote a lane to the main worktree

Run `st wt promote` inside a linked worktree when you want to continue that
branch in the repository's main worktree. Stax detaches the linked checkout,
checks the same branch out in the main worktree, then removes or parks the old
lane according to the existing warm-slot configuration.

Promotion preserves the branch, commits, upstream, PR linkage, and stax
metadata. It does not merge, delete, untrack, or automatically stash anything.
Both the current lane and main worktree must be clean, unlocked, and free of an
in-progress merge, rebase, or conflicts. If switching fails, Stax restores the
original checkouts and reports any rollback problem. Git may report a removal
failure only after it has already unregistered the old lane; in that case Stax
keeps the completed handoff in the main worktree, removes a dangling `.git` file
when safe, and tells you to inspect any remaining files in the old lane path.

With shell integration, the current shell moves to the main worktree only after
the handoff succeeds. Without it, Stax prints the main worktree path and a
copyable `cd` command.

## Cleanup and safety

| Command | What it does |
|---|---|
| `st wt rm [name]` | Remove one live worktree (no name = current) |
| `st wt rm --delete-branch` | Also delete the branch and its stax metadata |
| `st wt promote` | Keep the branch and make it the main-worktree checkout |
| `st wt prune` | Clear stale `git worktree` bookkeeping only — never removes a live directory |
| `st wt cleanup` | Prune bookkeeping, then bulk-remove safe candidates |
| `st wt rs` | Restack all stax-managed lanes |

`cleanup` is intentionally conservative. It only targets:

- detached worktrees
- stax-managed worktrees whose branches are already merged into trunk

It skips worktrees that are current, locked, mid-rebase, mid-merge, in conflict, or dirty (unless `-f`/`--force`). Use `--dry-run` to preview and `--yes` for non-interactive runs.

## Location, config, and hooks

By default, managed worktrees live outside the repository at:

```text
~/.stax/worktrees/<repo>
```

Override in `~/.config/stax/config.toml`, or set shared project overrides in repo-root `stax.toml`:

```toml
[worktree]
# root_dir = ""             # default external root
# root_dir = ".worktrees"   # keep worktrees inside the repo

# Warm-slot recycling: a removed worktree is parked and reused by the next lane
# instead of being deleted, keeping built gitignored deps on disk.
reuse_slots = true        # default; set false to always cold-create / real-remove

# Maximum number of idle warm slots to keep parked.
max_idle_slots = 4

# Optional: command run (non-fatally) inside a slot after it is adopted, to
# re-sync deps (e.g. "pnpm install", "uv sync").
# reconcile = "pnpm install"

[worktree.hooks]
post_create = ""   # blocking hook before launch
post_start  = ""   # background hook after creation
post_go     = ""   # background hook after entering an existing worktree
pre_remove  = ""   # blocking hook before removal
post_remove = ""   # background hook after removal
```

- Relative `root_dir` values resolve under the main repo root.
- Repo-local roots like `.worktrees` are added to `.gitignore` automatically.
- Warm-slot recycling (default): removing a clean, merged-equivalent worktree **parks** it instead of deleting it — stax moves it off its lane branch, resets it hard to trunk, and runs `git clean -fd`. That never uses `-x`, so gitignored dependency directories (`node_modules`, `.venv`, `vendor`, …) survive on disk.
- The next `create` / `lane` **adopts** an idle parked slot instead of a cold `git worktree add`: it switches the slot to the fresh lane branch, resets to the base, cleans untracked files, and runs the `reconcile` hook. Adopting reuses the same directory, so the built deps are already there.
- `reconcile` re-syncs deps after adoption (e.g. `pnpm install`, `uv sync`). It is **non-fatal** — a missing or failing command only warns and never fails the create.
- `max_idle_slots` caps how many idle slots are kept; parking beyond the cap falls back to a real removal, and `worktree cleanup` evicts the oldest excess slots.
- A `--force` (or confirmed) dirty removal **never** parks — it always does a real `git worktree remove`.
- Set `reuse_slots = false` to opt out entirely: cold `git worktree add` on create, real `git worktree remove` on removal, and no pool manifest.
- `post_create` and `pre_remove` are **blocking**; `post_start`, `post_go`, `post_remove` run in the **background**.
- `--no-verify` on `create` / `go` skips hooks for that command.

For the full config surface, see [Configuration](../configuration/index.md).
