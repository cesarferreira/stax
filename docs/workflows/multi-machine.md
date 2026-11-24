# Working across multiple machines

If you run stax on more than one machine against the same stack — a laptop and a
desktop, a local checkout and a remote dev box — a stack-mutating command run on
one machine (`st refresh`, `st submit`, `st restack` + push, `st cascade`) leaves
the other machine's local branches out of date. Trunk moves, and often the
feature branches themselves get rebased and force-pushed to a new SHA.

## Recognizing the situation

On the machine you didn't just use, `st status` / `st ls` typically shows `⟳`
(needs restack) next to branches, or a branch that's simply behind its own
remote tip. Don't `git pull` a stacked branch in this state — if it was rebased
elsewhere, the local and remote histories have diverged and a plain pull will
either conflict or silently create a merge commit.

## The fix: `st rs --get --restack` (or `st refresh --get`)

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

By default, from a stack branch, `--get` reconciles only the *current* stack.
To reconcile every tracked branch in every stack, use either of:

```bash
st rs --get --all-stacks          # reconcile every tracked branch (--all-stacks requires --get)
st rs --get                       # same thing when run from trunk (no current stack exists)
st refresh --all-stacks --get     # reconcile everything, then restack and submit every stack
```

Branches are reconciled parent before child, with sibling stacks in
alphabetical order. Frozen branches and branches checked out in another
worktree are skipped and reported, in every scope. `--dry-run` previews the
same branch set.

`--all-stacks` on `st sync` widens only `--get`. `st rs --get --all-stacks
--restack` still restacks just the current stack; to restack every stack, use
`st refresh --all-stacks --get` or `st restack --all`.

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
st rs --get --restack     # or: st refresh --get   (also pushes + updates PRs)

# Everything: every stack, reconciled, restacked, pushed
st refresh --all-stacks --get
```

## Related

- [`st get` reference](../commands/reference.md)
- [Sync and refresh](../commands/core.md)
- [Multi-worktree behavior](multi-worktree.md) — for worktree-local (not
  cross-machine) staleness
