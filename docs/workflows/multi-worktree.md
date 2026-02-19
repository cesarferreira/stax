# Multi-Worktree Support

stax is worktree-aware. If a branch in your stack is checked out in another worktree, stax runs operations in the right worktree automatically.

## Behavior

- Restack and sync `--restack` run `git rebase` in the target worktree when needed.
- Cascade fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- Sync updates trunk in whichever worktree currently has trunk checked out.

## Dirty worktrees

By default, stax fails fast when target worktrees contain uncommitted changes.

Use `--auto-stash-pop` to stash before rebase and restore afterward:

```bash
stax restack --auto-stash-pop
stax upstack restack --auto-stash-pop
stax sync --restack --auto-stash-pop
```

If conflicts occur, stax preserves the stash entry so changes are not lost.
