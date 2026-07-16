# Preserve Worktrees During Sync Cleanup

## Context

Issue [#636](https://github.com/cesarferreira/stax/issues/636) describes a destructive surprise in `stax sync` / `stax rs`: when a merged branch is checked out in another worktree, confirming branch cleanup can remove the entire worktree. That also removes worktree-local ignored and untracked files such as `.env` files.

The same cleanup machinery is used for upstream-gone branches, so the safe behavior must apply consistently to both merged and upstream-gone cleanup candidates.

## Goals

- Preserve a branch-owning worktree by default while still deleting the cleaned branch.
- Preserve ignored, untracked, and modified files in that worktree.
- Keep worktree removal available only as an explicit interactive action.
- Make `--force` select preservation without prompting; it must not imply permission to remove a worktree.
- Fail closed: if Stax cannot free the branch from its worktree safely, keep the worktree and branch and report recovery guidance.

## Non-goals

- Adding new command-line flags for this behavior.
- Building an arbitrary branch picker inside sync cleanup.
- Changing standalone `stax worktree remove` behavior or warm-slot recycling.
- Automatically resolving a rebase, merge, conflicts, or other in-progress Git operation in the affected worktree.

## User Experience

For a cleanup candidate checked out in a non-current worktree, interactive sync presents an action menu:

1. **Keep worktree and delete branch** (default)
2. **Remove worktree and delete branch**
3. **Skip**

For the main worktree, the removal action is omitted because the main worktree cannot be removed. When removal is unsafe because the worktree is locked or has an in-progress Git operation, the removal action is also omitted rather than being attempted implicitly.

The preserve action switches the affected worktree to trunk when trunk is available. If trunk is already checked out elsewhere, or switching to trunk would overwrite local changes, Stax detaches `HEAD` at the worktree's current commit. Detaching preserves the working directory while freeing the merged branch for deletion.

`stax sync --force` automatically chooses the preserve action. Quiet non-forced operation retains its existing fail-closed behavior and skips decisions that would require interaction.

## Design

### Cleanup decision

Introduce a small sync-local decision enum representing `PreserveWorktree`, `RemoveWorktree`, and `Skip`. Ordinary branches and the current branch keep their existing confirmation and parent-checkout flows. A branch owned by another worktree uses the action menu instead of encoding worktree removal inside a yes/no confirmation.

Decision collection remains separate from execution so parent resolution and child reparenting can continue to use the final set of confirmed deletions.

### Preserving the worktree

Reuse `BranchDeleteResolution` to locate the owning worktree and choose an initial switch target. Add a repository helper that performs the switch inside that worktree:

1. Try the resolved branch target when one is available.
2. If that switch fails, try `git switch --detach` at the current commit.
3. Return the Git error if detaching also fails.

No reset, clean, stash, or file deletion is part of preservation. After the worktree no longer owns the branch, normal local branch deletion proceeds.

### Explicit removal

The remove action reuses the existing worktree removal hooks, blocker checks, and `RemovalMode::AllowParking` behavior. When dirtiness is the only blocker, the action is labelled **Force-remove dirty worktree and delete branch**. The action is omitted for all other blockers. Dirty removal therefore remains explicitly destructive and requires the user to select it. `--force` never selects it.

### Failure handling

If preservation or explicit removal fails, sync records the cleanup as skipped and does not delete the local branch, remote branch, or Stax metadata for that candidate. Output names the affected worktree and explains whether switching or detaching failed, followed by the existing manual recovery command where useful.

There is no fallback from preservation to removal.

## Testing

Use test-driven development with integration coverage through the `stax` binary and focused unit tests for decision/prompt rendering.

- Happy path: `sync --force` deletes a merged branch, leaves its linked worktree present, frees the branch by switching or detaching, and preserves a gitignored `.env` file.
- Dirty path: tracked and untracked modifications survive; a conflicting trunk switch falls back to detached `HEAD`.
- Explicit removal: selecting remove retains the existing linked-worktree removal behavior.
- Skip path: selecting skip leaves the worktree, local branch, remote branch, and metadata unchanged.
- Error path: failed switch plus failed detach leaves all branch/worktree state intact and reports recovery guidance.
- Edge cases: main worktree ownership, trunk already checked out elsewhere, and upstream-gone cleanup use the same safe decision model.

Targeted iteration uses `cargo nextest run integration_tests::test_name` and unit-test filters. Final verification uses `make lint` and `make test` per repository policy.

## Documentation

- Update `docs/workflows/multi-worktree.md` to describe preservation as the default and document the interactive choices.
- Update `skills.md` so agents know that `sync --force` preserves affected worktrees.
- `README.md` does not require a change because this is a detailed cleanup safety behavior rather than a quick-start command or core capability change.
