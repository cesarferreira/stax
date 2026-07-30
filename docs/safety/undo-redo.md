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

- **Trunk head** — fast-forwarded (or reset) local trunk is rolled back, regardless of
  whether trunk was checked out in the main worktree or in a linked worktree.
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
