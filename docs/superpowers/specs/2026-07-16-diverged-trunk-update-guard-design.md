# Diverged Trunk Update Guard Design

## Problem

`stax update` promises to sync trunk before it restacks and submits the current stack. When the local trunk and its remote-tracking branch have diverged, sync correctly refuses to reset the local trunk, but the command currently continues into restack. The feature branch is then rebased onto a trunk that did not sync, and a later submit can publish that incorrect history.

The same unsafe path is available through `stax sync --restack` because update delegates its sync and restack work to that command.

## Desired Behavior

- A command that requests restacking must fail closed when the local trunk does not reach the fetched remote trunk.
- A failed fetch must also block restacking; cached remote-tracking refs are not proof that the remote state is fresh.
- The failure must occur before imported-branch refresh, merged-branch cleanup, or restacking can rewrite or delete feature refs.
- `stax update` must not enter its submit phase after the sync failure.
- The error must name both trunk refs and tell the user to inspect and reconcile them before retrying.
- Plain `stax sync` without `--restack` keeps its existing behavior: fetch what it can, preserve local trunk commits, finish without rewriting feature branches, and print the existing follow-up warning.
- Stax must never reset or discard divergent local trunk commits automatically.

## Design

Keep the safety boundary in `commands::sync::run`, where it protects every caller that requests restacking. There are three checks:

1. Immediately after fetch, a non-successful fetch blocks restacking before any ref-update phase. This check does not trust an existing remote-tracking ref because it may be stale.
2. Immediately after the trunk-update attempt, compare the fixed fetched remote OID with the current local trunk OID. This runs before imported-branch refresh and merged-branch cleanup.
3. Repeat the OID comparison immediately before the optional restack phase, in case an intervening cleanup path changed trunk state.

The OID checks use:

- the fetched remote trunk OID captured after fetch; and
- the current local trunk OID resolved at each OID guard.

When a guard fails, restore any automatic stash and return an error. Because `commands::refresh::run` already propagates sync errors with `?`, `stax update` will stop and will not call submit.

The guard belongs in sync rather than only in update so direct `stax sync --restack` calls receive the same protection. Plain sync calls skip the guard because they did not request history rewriting.

## Error Handling

The failure should be explicit and actionable, for example:

```text
Cannot restack because main did not reach origin/main.
Inspect and reconcile main with origin/main, then retry.
```

If sync stashed a dirty worktree, it must pop that stash before returning the error. A stash-pop failure remains an error and must not be hidden.

## Regression Coverage

Add an integration test that creates this history:

1. local and remote trunk share a baseline;
2. local trunk gains one local-only commit;
3. remote trunk gains a different commit;
4. a tracked feature branch remains based on the original baseline.

Run `stax update` through a non-interactive local-remote fixture and assert:

- the command exits unsuccessfully with the actionable divergence message;
- the feature branch OID is unchanged;
- the feature branch remote OID is unchanged;
- no rebase remains in progress; and
- local trunk retains its local-only commit.

Existing update and sync tests must continue to pass for fast-forwardable and already-current trunks.

Add two edge-case regressions:

- a squash-merged parent with a surviving child under default cleanup, proving the child OID and parent ref remain untouched when trunk diverges; and
- a failed fetch while cached `origin/main` still equals local `main`, proving stale cached state cannot authorize restacking and an auto-stash is restored.

## Non-Goals

- Automatically choosing between local and remote trunk history.
- Resetting, rebasing, merging, or pushing the trunk on the user's behalf.
- Changing cleanup behavior for plain `stax sync`.
- Redesigning the sync command's reporting model.
