# Worktrees

stax has two related worktree stories:

- repo-wide worktree awareness: `restack`, `sync`, `cascade`, and metadata operations run in the right linked checkout automatically
- the `st worktree` command family: `st worktree` / `st wt` creates, enters, inspects, and cleans up worktree lanes

This page is the canonical guide for the `st worktree` command. For repo-wide behavior, see [Multi-Worktree Behavior](../workflows/multi-worktree.md). For the parallel AI workflow, see [AI Worktree Lanes](../workflows/agent-worktrees.md).

## Quick Start

```bash
# Open the dashboard in an interactive terminal
st wt

# Create a fresh lane with a random funny name
st wt c

# Create or reuse a named lane
st wt c payments-api

# Start a new lane from an explicit base
st wt c payments-api --from main

# Jump back into an existing lane
st wt go payments-api

# Inventory views
st wt ls
st wt ll

# Print the absolute path for scripting
st wt path payments-api

# Remove or clean up lanes
st wt rm payments-api
st wt cleanup --dry-run
st wt prune
st wt rs
```

## How `create` And `go` Resolve Targets

`st wt c [name]` is deliberately convenience-first:

- with no `name`, it generates a random two-word lane slug
- if `name` matches an existing worktree, it reuses that lane instead of creating a duplicate
- if `name` matches an existing branch, it creates a worktree for that branch
- otherwise, it creates a new branch and a new worktree lane

`st wt go [name]` only opens an existing worktree. With no `name`, it opens an interactive picker.

Selectors accepted by `go`, `path`, `rm`, and the reuse path in `create` can match:

- the worktree name
- the full branch name
- a unique branch suffix such as `payments-api` matching `cesar/payments-api`
- an absolute worktree path

### Base Branch Rules

For a new branch created by `st wt c`:

- `--from <branch>` explicitly sets the base branch
- otherwise, if the current branch is already tracked by stax, the new lane stacks on the current branch
- otherwise, the new lane starts from trunk

Use `--pick` to choose an existing local branch interactively instead of typing a name.

Use `--name <worktree-name>` when the branch name and the worktree directory label should differ.

Use `--no-verify` on `create` or `go` to skip worktree hooks for that entry.

## Command Map

| Command | Aliases | What it does |
|---|---|---|
| `st worktree` | `st wt` | Open the interactive dashboard when stdin/stdout are TTYs; otherwise print worktree help |
| `st wt c [name]` | `st worktree create`, `st wtc` | Create or reuse a lane; supports `--from`, `--pick`, `--name`, `--agent`, `--run`, `--tmux`, `--no-verify` |
| `st wt go [name]` | `st worktree go`, `st wtgo` | Enter an existing worktree; with no name, open a picker; supports `--agent`, `--run`, `--tmux`, `--no-verify` |
| `st wt ls` | `st worktree list`, `st w`, `st wtls` | Compact `NAME / BRANCH / PATH` inventory; add `--json` for scripting |
| `st wt ll` | `st worktree ll`, `st wtll` | Rich status view with managed/dirty/rebase/conflicts/marker/prunable state; add `--json` for scripting |
| `st wt path <name>` | `st worktree path <name>` | Print the absolute path of a worktree |
| `st wt rm [name]` | `st worktree remove`, `st wtrm` | Remove one worktree; with no name, remove the current lane; supports `-f/--force` and `--delete-branch` |
| `st wt prune` | `st worktree prune`, `st wtprune` | Remove stale `git worktree` bookkeeping only |
| `st wt cleanup` | `st worktree cleanup`, `st wt clean` | Prune stale bookkeeping, then bulk-remove safe detached or managed-and-merged lanes; supports `--dry-run`, `--yes`, and `-f/--force` |
| `st wt restack` | `st worktree restack`, `st wtrs`, `st wt rs` | Restack all stax-managed worktrees |

## Launch A Tool Inside The Lane

You can enter a lane and immediately launch a tool there:

```bash
st wt c auth-refresh --agent codex -- "fix the flaky tests"
st wt go auth-refresh --agent claude
st wt go ui-polish --run "cursor ."
st wt c review-pass --agent codex --tmux -- "address the open PR comments"
st wt go review-pass --agent codex --tmux
```

Rules:

- `--agent` supports `claude`, `codex`, `gemini`, and `opencode`
- `--model` requires `--agent`
- `--run` and `--agent` are mutually exclusive
- anything after `--` is passed through to the launched agent or command
- `--tmux` creates or reuses a tmux session named after the worktree unless you override it with `--tmux-session`

If you run `create` or `go` without shell integration and without a launcher, stax prints the `cd` command you need.

## Managed Vs Unmanaged Worktrees

`st wt ls` shows every Git worktree, including ones created outside stax.

The important distinction is whether the branch has stax metadata:

- new branches created by `st wt c foo` are stax-managed
- existing tracked branches stay managed when you open a lane for them
- existing plain Git branches can get a worktree, but they stay unmanaged until you run `st branch track`

Managed lanes behave like first-class stax branches:

- they show up in `st ls`
- they participate in `restack`, `sync --restack`, and undo/redo flows
- they are targeted by `st wt restack`
- merged managed lanes are eligible for `st wt cleanup`

Unmanaged or detached worktrees still show up in `ls`, `ll`, `go`, `rm`, and `prune`, but stax keeps the history-rewriting operations conservative.

## Dashboard

Run `st wt` in an interactive terminal to open the worktree dashboard.

- Left pane: all Git worktrees, including unmanaged entries
- Right pane: branch, base, path, status, and tmux session details
- `Enter`: attach or switch to the derived tmux session for the selected worktree
- `c`: create a lane and open it in tmux
- `d`: remove the selected worktree
- `R`: restack all stax-managed worktrees
- `?`: show help
- `q` / `Esc`: quit

The dashboard is a control plane, not an embedded shell. The stack/patched-branch TUI stays documented separately in [Interactive TUI](../interface/tui.md).

## Shell Integration

Install shell integration once if you want the parent shell to move into the target directory automatically:

```bash
st shell-setup
st shell-setup --install
```

After installation:

- `st wt c ...` changes the parent shell into the new lane
- `st wt go ...` changes the parent shell into the selected lane
- `st wt rm` can relocate the shell before removing the current worktree
- `sw <name>` becomes a quick alias for `st wt go <name>`

Shell integration supports `bash`, `zsh`, and `fish`.

!!! note "Windows"
    On Windows, worktree commands still work, but the parent shell cannot auto-`cd`, `sw` is unavailable, and tmux integration is not supported. After `st wt c` or `st wt go`, manually `cd` to the printed path. See [Windows notes](../reference/windows.md).

## Cleanup And Safety

Use the worktree cleanup commands intentionally:

- `st wt rm [name]`: remove one live worktree; with no name, remove the current worktree
- `st wt rm --delete-branch`: after removing the worktree, also try to delete the branch and its stax metadata
- `st wt prune`: clear stale `git worktree` bookkeeping only; it never removes a live directory
- `st wt cleanup`: prune stale bookkeeping first, then bulk-remove safe candidates
- `st wt rs`: restack all stax-managed lanes only

`cleanup` is intentionally conservative. It only targets:

- detached worktrees
- stax-managed worktrees whose branches are already merged into trunk

It skips worktrees that are:

- current
- locked
- mid-rebase
- mid-merge
- in conflict
- dirty, unless you pass `-f` / `--force`

Use `--dry-run` to preview the prune/remove plan and `--yes` for non-interactive confirmation.

## Location, Config, And Hooks

By default, stax keeps managed worktrees outside the repository under:

```text
~/.stax/worktrees/<repo>
```

Override that under `[worktree]` in `~/.config/stax/config.toml`:

```toml
[worktree]
# Leave unset/empty for the default external root
# root_dir = ""

# Or keep worktrees inside the repo, for example:
# root_dir = ".worktrees"

[worktree.hooks]
post_create = ""
post_start = ""
post_go = ""
pre_remove = ""
post_remove = ""
```

Notes:

- relative `root_dir` values are resolved under the main repository root
- repo-local roots such as `.worktrees` are added to `.gitignore` automatically
- `post_create` and `pre_remove` are blocking hooks
- `post_start`, `post_go`, and `post_remove` run in the background
- `--no-verify` on `create` and `go` skips the hook path for that command

For the full config surface, see [Configuration](../configuration/index.md).
