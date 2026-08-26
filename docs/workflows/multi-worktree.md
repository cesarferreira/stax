# Multi-worktree behavior

stax is worktree-aware: when a branch in your stack is checked out in another worktree, stax runs rebase, sync, and metadata operations in the right place automatically.

This page covers repo-wide behavior across linked checkouts. For the `st worktree` command surface, see [Worktrees](../worktrees/index.md). For parallel AI lanes, see [AI worktree lanes](agent-worktrees.md).

## What's worktree-aware

- `st restack` and `st sync --restack` run `git rebase` in the target worktree when needed.
- `st cascade` fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- `st sync` updates trunk in whichever worktree currently has trunk checked out.
- `st sync` cleanup preserves a linked worktree that owns a merged or upstream-gone branch by switching it to trunk or detaching `HEAD` before deleting the branch.
- Metadata (`refs/branch-metadata/*`) is shared across all worktrees automatically.

## Sync cleanup

When a cleanup candidate is checked out in another worktree, interactive sync offers to keep the worktree (the default), remove it, or skip the branch. Keeping it preserves ignored, untracked, and modified files. If switching the worktree to trunk would overwrite local changes, stax leaves it on a detached `HEAD` at its existing commit instead.

`st sync --force` also keeps the worktree. In this command, `--force` skips prompts but never authorizes worktree removal. If stax cannot switch or detach the worktree safely—for example, because a merge is in progress—it keeps the worktree, branch, remote branch, and metadata and reports the commands needed for manual recovery.

## Dirty worktrees

By default, stax fails fast when a target worktree contains uncommitted changes. Use `--auto-stash-pop` to stash before rebase and restore afterward:

```bash
st restack --auto-stash-pop
st upstack restack --auto-stash-pop
st sync --restack --auto-stash-pop
```

`--stash` and `--no-stash` control the **current** working tree before sync starts, not the linked target worktrees. `--stash` auto-stashes the current tree without prompting (works with `--quiet`/`--json`; does NOT auto-confirm branch deletions); `--no-stash` makes sync fail immediately on a dirty current tree, overriding `--force`. In contrast, `--auto-stash-pop` acts during the restack phase on each **linked target** worktree as branches are rebased. The two mechanisms are orthogonal: you can combine `--stash --restack --auto-stash-pop` to handle both the current tree and any linked worktrees in a single `st rs` invocation.

If a conflict occurs, the stash entry is preserved so nothing is lost. When `st sync` auto-stashes your working tree and fails on an unrecoverable error path (for example, when restoring the stash itself conflicts with an updated trunk), it prints a warning to stderr naming the stash "stax auto-stash" with instructions to run `git stash pop` to restore your changes.

## Related

- [Worktrees (`st wt`)](../worktrees/index.md)
- [AI worktree lanes (`st lane`)](agent-worktrees.md)
