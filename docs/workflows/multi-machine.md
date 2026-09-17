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
branches unless you also pass `--delete-merged`. Note that with
`--all-stacks`, `--get` still only reconciles the branches of the *current*
stack; other stacks are restacked from their local commits. If you're checked
out on trunk itself, there is no current stack to reconcile, so `--get` is a
silent no-op — check out a stack branch first.

`st get <branch>` remains useful for a single named branch, or for a branch
that isn't checked out locally yet at all (it creates the local tracking
branch). `--get` only reconciles branches that already exist locally in your
current stack.

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
```

## Related

- [`st get` reference](../commands/reference.md)
- [Sync and refresh](../commands/core.md)
- [Multi-worktree behavior](multi-worktree.md) — for worktree-local (not
  cross-machine) staleness
