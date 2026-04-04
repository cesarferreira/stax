# Multi-Worktree Support

stax is worktree-aware. If a branch in your stack is checked out in another worktree, stax runs rebase, sync, and metadata operations in the right place automatically.

This page is about repo-wide behavior across linked checkouts. For the `st worktree` / `st wt` command itself, see [Worktrees](../worktrees/index.md).

## Worktree-safe operations

- `st restack` and `st sync --restack` run `git rebase` in the target worktree when needed.
- `st cascade` fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- `st sync` updates trunk in whichever worktree currently has trunk checked out.
- If a merged or upstream-gone branch is still checked out in another worktree, `st sync` removes that linked worktree when cleanup is confirmed and the lane is safe to remove; dirty/locked/in-progress lanes are kept with follow-up cleanup commands instead.
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

## Using `st wt` For Lanes

Use the dedicated [Worktrees](../worktrees/index.md) guide for the command surface itself:

- `st wt` dashboard behavior
- `create`, `go`, `ls`, `ll`, `path`, `rm`, `cleanup`, `prune`, and `restack`
- shell integration and `sw`
- managed vs unmanaged lane behavior
- tmux, hooks, and configuration

For the parallel AI version of this flow, see [AI Worktree Lanes](agent-worktrees.md).
