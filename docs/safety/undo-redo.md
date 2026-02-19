# Undo and Redo

stax makes history rewriting safer with transactional operations and built-in recovery.

```bash
stax restack
# ... conflict or bad outcome
stax undo
```

## Transaction model

For potentially destructive operations (`restack`, `submit`, `sync --restack`, TUI reorder), stax:

1. Snapshots affected branch SHAs
2. Creates backup refs at `refs/stax/backups/<op-id>/<branch>`
3. Executes operation
4. Writes operation receipt to `.git/stax/ops/<op-id>.json`

If needed, `stax undo` restores branches to exact pre-operation commits.

## Commands

| Command | Description |
|---|---|
| `stax undo` | Undo the last operation |
| `stax undo <op-id>` | Undo a specific operation |
| `stax redo` | Re-apply the last undone operation |

## Useful flags

- `--yes` auto-approves prompts
- `--no-push` restores local branches only

If remote branches were force-pushed by the operation, stax offers to restore them too.
