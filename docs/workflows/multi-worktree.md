# Multi-Worktree Support

stax is worktree-aware. If a branch in your stack is checked out in another worktree, stax runs rebase, sync, and metadata operations in the right place automatically.

## Worktree-safe operations

- `st restack` and `st sync --restack` run `git rebase` in the target worktree when needed.
- `st cascade` fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- `st sync` updates trunk in whichever worktree currently has trunk checked out.
- Metadata (`refs/branch-metadata/*`) is shared across all worktrees automatically.

## Dirty worktrees

By default, stax fails fast when target worktrees contain uncommitted changes.

Use `--auto-stash-pop` to stash before rebase and restore afterward:

```bash
st restack --auto-stash-pop
st upstack restack --auto-stash-pop
st sync --restack --auto-stash-pop
```

If conflicts occur, stax preserves the stash entry so changes are not lost.

---

## `st worktree`

`st worktree` (alias `st wt`) is the stax-native workflow for parallel lanes. It keeps `wt ls` simple, uses `go` instead of `switch`, and layers optional AI launch on top of normal Git worktrees instead of a separate registry.

### Quick start

```bash
# Create a fresh lane with a random funny name
st wt c

# Create or reuse a named lane
st wt c payments-api

# Start from a specific base branch
st wt c payments-api --from main

# Jump back into an existing lane
st wt go payments-api

# Compact inventory
st wt ls

# Rich status view
st wt ll

# Remove a lane
st wt rm payments-api
```

### AI launch

Use `--agent` to start a supported interactive CLI inside the target worktree:

```bash
st wt c auth-refresh --agent codex -- "fix the flaky tests"
st wt go auth-refresh --agent claude
st wt go ui-polish --run "cursor ."
```

Supported agent values match the other AI-aware commands in stax: `claude`, `codex`, `gemini`, and `opencode`.

### Shell integration

`st wt c` and `st wt go` need shell integration if you want the parent shell to move into the target directory automatically.

```bash
st shell-setup
st shell-setup --install
```

After installation, both `st` and `stax` transparently handle:

- `st wt c ...`
- `st wt go ...`
- `st wt rm` when removing the current worktree
- `sw <name>` as a quick alias for `st wt go <name>`

### Command shape

| Command | Purpose |
|---|---|
| `st wt c [name]` | Create or reuse a worktree lane. With no name, generate a random lane name. |
| `st wt go [name]` | Jump to an existing worktree. With no name, open a fuzzy picker. |
| `st wt ls` | Simple `NAME / BRANCH / PATH` inventory. |
| `st wt ll` | Rich worktree status: managed, dirty, rebase/conflicts, marker, prunable, locked. |
| `st wt rm [name]` | Remove a worktree. With no name, remove the current lane. |
| `st wt prune` | Run safe `git worktree prune` housekeeping only. |
| `st wt restack` / `st wt rs` | Restack stax-managed worktrees only. |

### Managed vs unmanaged worktrees

`st wt ls` shows every Git worktree, including ones created outside stax.

`st wt restack` only targets stax-managed worktrees: linked worktrees whose branch is tracked by stax metadata. Detached worktrees and ad-hoc third-party worktrees still show up in `ls`, `ll`, `go`, `rm`, and `prune`, but they are skipped by `restack`.

### Related guide

For the AI-oriented version of this flow, including `--agent` examples and random lane creation, see [Agent Worktrees](agent-worktrees.md).
