# Working across multiple machines

If you run stax on more than one machine against the same stack — a laptop and a
desktop, a local checkout and a remote dev box — a stack-mutating command run on
one machine (`st refresh`, `st submit`, `st restack` + push, `st cascade`) leaves
the other machine's local branches out of date. Trunk moves, and often the
feature branches themselves get rebased and force-pushed to a new SHA.

## Recognizing the situation

On the machine you didn't just use, run:

```bash
st status --remote
```

It fetches the remote tips of the branches it shows (only `refs/remotes/*`, never
your local branches) and labels each one against its own remote ref:

```text
○ feature-b [origin: 2↓ behind]
○ feature-a [origin: diverged 1↑/3↓]
○ main      [origin: in sync]

↓ 2 branches behind or diverged from origin. Run st pull to reconcile.
```

Use `--no-fetch` to compare against cached remote-tracking refs when you are
offline (a failed fetch also falls back to cached refs with a warning).
Plain `st status` / `st ls` compares each branch with its parent, so it may show `⟳`
(needs restack) but not whether a branch is behind its own remote tip; use
`st status --remote` for that comparison. Don't `git pull` a stacked branch in this state — if it was rebased
elsewhere, the local and remote histories have diverged and a plain pull will
either conflict or silently create a merge commit.

## The fix: `st pull`

```bash
st pull
```

`st pull` reconciles every tracked branch in every stack with its own remote
ref, then restacks every stack onto its updated parents. It does not push. It
is `st sync --get --all-stacks --restack` followed by a restack of each stack,
so it also works from trunk. Frozen branches are skipped.

- A clean working tree is required unless you pass `--auto-stash-pop`, which
  stashes and pops dirty worktrees (including the current one) while restacking.
- `st pull --force` lets the remote win outright: diverged branches are hard-reset
  to their remote tip, and prompts are skipped.
- `st pull --no-restack` only reconciles branches (same as
  `st sync --get --all-stacks`).
- `st pull --dry-run` (alias `--plan`) previews the reconciliation and lists the
  stacks it would restack. It makes no changes and does not predict conflicts.
- It stops at the first conflict. Resolve it, run `st continue`, then re-run
  `st pull`. Finished stacks are no-ops on the re-run.
- It records one undo receipt per operation; `st undo` reverts only the most
  recent one.
- There is no `--json`. Use `st sync --get --all-stacks --json` for machine output.

To also push and update PRs, use `st refresh --all-stacks --get`.

## Narrower: `st rs --get --restack` (or `st refresh --get`)

```bash
st rs --get --restack
```

`--get` fetches each branch of your **current stack** from its own remote ref
(not just trunk) and reconciles it before the restack phase runs:

- fast-forwards the local branch if the remote is simply ahead,
- rebases local-only commits onto the fetched remote tip if histories
  diverged (e.g. the branch was rebased and re-pushed elsewhere),
- or resets it entirely (with `--force`) if you want the remote to win
  outright.

One command converges your whole current stack with whatever another machine
already pushed, then restacks — instead of re-deriving a second, divergent
rebase locally.

`st refresh --get` runs the same reconciliation as part of the full refresh
flow (sync trunk → reconcile each stack branch against its own remote →
restack → push and update PRs). `st refresh --get --force` is equivalent to
`st rs --get --restack --force`, except that refresh does not delete merged
branches unless you also pass `--delete-merged`.

### Every stack at once

Start with `st pull`. The lower-level forms below are for finer control. By
default, from a stack branch, `--get` reconciles only the *current* stack.
To reconcile every tracked branch in every stack, use any of:

```bash
st pull                          # reconcile + restack every stack, no push
st rs --get --all-stacks          # reconcile every tracked branch (--all-stacks requires --get)
st rs --get                       # same thing when run from trunk (no current stack exists)
st refresh --all-stacks --get     # reconcile everything, then restack and submit every stack
```

Branches are reconciled parent before child, with sibling stacks in
alphabetical order. Frozen branches and branches checked out in another
worktree are skipped and reported, in every scope. `--dry-run` previews the
same branch set.

`st pull` does both halves for every stack. `--all-stacks` on `st sync` widens
only `--get`: `st rs --get --all-stacks --restack` still restacks just the
current stack. To restack every stack and also push and update PRs, use
`st refresh --all-stacks --get`; `st restack --all` restacks without
reconciling.

`st get <branch>` remains useful for a single named branch, or for a branch
that isn't checked out locally yet at all (it creates the local tracking
branch). `--get` only reconciles branches that already exist locally in the
reconciled scope.

### Why not bare `st rs` / bare `st get`

With **no `--get`**, `st sync`/`st rs` only fetches **trunk**, then restacks
your local branches onto their local parents using your *local* commits. It
does not re-fetch the SHAs your feature branches already have on the remote.
Likewise, bare `st get` (no branch argument) is equivalent to bare `st sync`.
If your feature branches were rebased and pushed from another machine, either
form will rebase your stale local commits onto the new trunk independently —
producing a *second*, different rebase of the same content, which then needs
a force-push (and can conflict with what's already on the remote).

## Typical flow

```bash
# On the machine where you did the work
st refresh          # or: st submit / st restack + push

# On the other machine
st status --remote  # see drift
st pull             # reconcile + restack every stack (add --force to let the remote win)

# Everything, including push and PR updates
st refresh --all-stacks --get
```

## Related

- [`st get` reference](../commands/reference.md)
- [Sync and refresh](../commands/core.md)
- [Multi-worktree behavior](multi-worktree.md) — for worktree-local (not
  cross-machine) staleness
