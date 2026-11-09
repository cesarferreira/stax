# Undo and redo

stax makes history rewriting safer with transactional operations and built-in recovery.

```bash
st restack
# ... conflict or unwanted outcome
st undo
```

## Transaction model

For potentially destructive operations (`restack`, `submit`, `sync`, TUI reorder, `split`, `fix`), stax:

1. Snapshots affected branch SHAs
2. Creates backup refs at `refs/stax/backups/<op-id>/<branch>`
3. Executes the operation
4. Writes a receipt to `.git/stax/ops/<op-id>.json`

`st undo` restores branches to their exact pre-operation commits.

### What a sync receipt covers

`stax sync` uses a single lazily-snapshotted transaction. Each branch is snapshotted
right before it is mutated, so `stax undo` can restore:

- **Trunk head** — local trunk is rolled back, regardless of whether trunk was checked
  out in the main worktree or in a linked worktree. A sync whose only change is a pure
  fast-forward of trunk writes no receipt and no backup refs, because no commits are
  lost; `st undo` then targets the previous operation. Trunk is recorded when the same
  sync also changes something else (deleted branches, reparenting, a restack, ...), or
  when trunk was reset or had diverged from its remote.
- **Deleted branch heads** — branches removed as merged or upstream-gone are re-created
  at their original tips.
- **Deleted metadata refs** — stax metadata (`refs/branch-metadata/<branch>`) is restored.
- **Reparented children's metadata** — when a deleted branch's children were moved to a
  new parent, their parent metadata is restored to point back to the deleted branch.
- **Rebased squash-merge children** — children rebased onto trunk during squash-merge
  cleanup are restored to their pre-rebase SHAs.
- **Restack phase** — if `--restack` was used, branches rebased during the restack phase
  are also restored.

`stax redo` replays the operation forward: deleted branches are re-deleted and trunk is
re-advanced to the post-sync SHA.

**Not restored by undo** (intentional scope limits):

- Removed linked worktrees (`stax worktree remove` within sync is not tracked).
- Forge PR base updates (GitHub/GitLab API side-effects are external to git).
- CI history writes.

## Retention

Receipts and backup refs would otherwise pile up, so `st sync` prunes old ones. Pruning
runs at most once a day per repository (or linked worktree), after a successful sync, and
is silent unless you pass `--verbose`. An operation is deleted only when **all** of these hold:

- it is not among the newest `keep_last` operations (default 50, minimum 1; the newest
  operation is always kept because it is the `st undo` / `st redo` target);
- it is older than `keep_days` days (default 14; `0` means age never protects an operation);
- it finished successfully.

Failed and in-progress operations, and receipts stax cannot parse, are never pruned
automatically. Pruning deletes the operation's receipt and its
`refs/stax/backups/<op-id>/*` refs. Backup refs without a receipt in the current
repository directory are left alone, because linked worktrees share backup refs but each
keeps its own receipts.

```toml
[undo]
auto_prune = true # set to false to keep every receipt and backup ref
keep_last = 50
keep_days = 14
```

Things to know:

- `st undo <op-id>` for a pruned operation fails with "Failed to read receipt".
- Pruning itself is not undoable.
- Once the backup refs are gone, `git gc` may delete commits that only they reached.

See [Configuration](../configuration/index.md) for the full config reference.

## Commands

| Command | Description |
|---|---|
| `st undo` | Undo the last operation |
| `st undo <op-id>` | Undo a specific operation |
| `st redo` | Re-apply the last undone operation |

## Flags

- `--yes` — auto-approve prompts
- `--no-push` — restore local branches only

If the operation force-pushed remote branches, stax offers to restore them too.
