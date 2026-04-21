# Undo and redo

stax makes history rewriting safer with transactional operations and built-in recovery.

```bash
st restack
# ... conflict or unwanted outcome
st undo
```

## Transaction model

For potentially destructive operations (`restack`, `submit`, `sync --restack`, TUI reorder, `split`, `fix`), stax:

1. Snapshots affected branch SHAs
2. Creates backup refs at `refs/stax/backups/<op-id>/<branch>`
3. Executes the operation
4. Writes a receipt to `.git/stax/ops/<op-id>.json`

`st undo` restores branches to their exact pre-operation commits.

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
