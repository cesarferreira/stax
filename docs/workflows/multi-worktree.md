# Multi-worktree behavior

stax is worktree-aware: when a branch in your stack is checked out in another worktree, stax runs rebase, sync, and metadata operations in the right place automatically.

This page covers repo-wide behavior across linked checkouts. For the `st worktree` command surface, see [Worktrees](../worktrees/index.md). For parallel AI lanes, see [AI worktree lanes](agent-worktrees.md).

## What's worktree-aware

- `st restack` and `st sync --restack` run `git rebase` in the target worktree when needed.
- `st cascade` fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- `st sync` updates trunk in whichever worktree currently has trunk checked out.
- `st sync` cleanup will remove a linked worktree that owned a merged or upstream-gone branch, when it's safe. Dirty/locked/in-progress lanes are kept for manual cleanup.
- Metadata (`refs/branch-metadata/*`) is shared across all worktrees automatically.

## Dirty worktrees

By default, stax fails fast when a target worktree contains uncommitted changes. Use `--auto-stash-pop` to stash before rebase and restore afterward:

```bash
st restack --auto-stash-pop
st upstack restack --auto-stash-pop
st sync --restack --auto-stash-pop
```

If a conflict occurs, the stash entry is preserved so nothing is lost.

## Related

- [Worktrees (`st wt`)](../worktrees/index.md)
- [AI worktree lanes (`st lane`)](agent-worktrees.md)
