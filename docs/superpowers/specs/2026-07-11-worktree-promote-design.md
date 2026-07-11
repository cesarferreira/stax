# Worktree Promote

**Date:** 2026-07-11
**Status:** Approved

## Problem

A branch checked out in a linked worktree is already a normal Git branch, but
moving that branch back to the repository's main worktree currently requires a
manual sequence: leave or remove the linked worktree, check out its branch in
the main worktree, and relocate the shell. The sequence is easy to get wrong,
especially because Git prevents one branch from being checked out in two
worktrees at once.

Stax should provide one safe command for turning the current linked-worktree
lane into the branch checked out in the main worktree.

## Goals

- Add `st wt promote` / `st worktree promote` for the current linked worktree.
- Preserve the branch, commits, upstream, PR linkage, and Stax metadata.
- Retire the linked worktree using the configured remove-or-park behavior.
- Check out the promoted branch in the main worktree.
- Move the parent shell to the main worktree when shell integration is active.
- Fail without changing either worktree when known safety preconditions are not
  met.
- Roll back checkout changes when an unexpected Git failure occurs during the
  handoff.

## Non-goals

- Promoting a named worktree from another checkout.
- Automatically stashing or moving uncommitted changes.
- Adding a `--stash` or `--force` mode.
- Deleting or untracking the promoted branch.
- Adding the action to the worktree dashboard in the first version.
- Moving ignored dependency or build artifacts into the main worktree.

## Command Surface

```text
st worktree promote
st wt promote
```

The command accepts no worktree selector in the first version. It always acts
on the current checkout, which keeps the command's meaning and shell relocation
unambiguous.

Running it from the main worktree is an error because there is no linked
worktree to promote. Running it from a detached HEAD is also an error because
there is no branch to hand off.

## Preconditions

Before running hooks or changing Git state, Stax resolves the main worktree and
the current linked worktree, then verifies all of the following:

- the current checkout is a linked worktree, not the main worktree;
- the linked worktree is on a local branch;
- both the linked worktree and main worktree are clean;
- neither checkout is locked or has an in-progress merge, rebase, or conflict;
- the main worktree path exists and is usable; and
- the branch is not checked out in any worktree other than the current one.

If a check fails, Stax exits with a specific explanation and suggested manual
recovery command where useful. It does not offer to stash automatically.

## Handoff Flow

Stax records the source branch and the branch currently checked out in the main
worktree, then performs the handoff:

1. Run the existing blocking `pre_remove` hook for the linked worktree.
2. Detach HEAD in the linked worktree, releasing the source branch.
3. Switch the main worktree to the source branch.
4. Retire the linked worktree through the existing removal infrastructure,
   allowing configured warm-slot parking when eligible.
5. Run the existing background `post_remove` hook.
6. Emit the main worktree path for shell integration and print a concise success
   message naming the branch and destination.

The command reuses existing worktree discovery, cleanliness checks, hooks,
parking, removal, and shell-message mechanisms rather than duplicating their
policies.

## Rollback

All predictable failures are caught during preflight. For unexpected failures:

- If switching the main worktree fails after the linked worktree is detached,
  Stax switches the linked worktree back to the source branch.
- If retiring the linked worktree fails after the main worktree has switched,
  Stax switches the main worktree back to its original branch, then switches
  the linked worktree back to the source branch.
- If rollback itself fails, Stax reports the exact resulting checkout state and
  manual commands needed to recover. It never claims the promotion succeeded.

The linked worktree is not removed before the main checkout succeeds, so the
most destructive operation occurs last.

## Shell Behavior

With Stax shell integration, a successful `st wt promote` changes the parent
shell's directory to the main worktree after the command completes. The shell
wrapper must not move the shell on failure.

Without shell integration, Stax cannot change the parent process's directory.
It prints the main worktree path and a copyable `cd` command, consistent with
the existing worktree create/go behavior.

## User-facing Output

Success output should state that the branch was promoted and identify the main
worktree path. Errors should identify which checkout blocks the operation and
why, for example:

```text
Cannot promote: the main worktree has uncommitted changes at <path>.
Commit or stash those changes, then retry `st wt promote`.
```

## Documentation

The implementation updates:

- `README.md` if its worktree command overview lists individual actions;
- `docs/worktrees/index.md`;
- `docs/commands/reference.md`; and
- `skills.md`.

## Testing

Integration tests exercise the real `stax` binary in temporary repositories.
Coverage includes:

- successful promotion of a clean linked worktree;
- preservation of Stax metadata and the branch's commit;
- warm-slot parking and real-removal configurations;
- refusal when the linked worktree is dirty;
- refusal when the main worktree is dirty;
- refusal from detached HEAD;
- refusal from the main worktree;
- rollback when switching the main worktree fails;
- rollback when linked-worktree retirement fails; and
- shell integration relocating only after success.

Targeted worktree tests provide the tight feedback loop. Final full-suite
verification uses `make test` as required by the repository test policy.
