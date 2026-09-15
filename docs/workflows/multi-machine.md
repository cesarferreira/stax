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

## The fix: `st get <branch>`, not bare `st get`

```bash
st get <top-branch-of-your-stack>
```

`st get` with an explicit branch name fetches that branch **and its local
upstack chain** from the remote, and for each one:

- fast-forwards the local branch if the remote is simply ahead,
- rebases local-only commits onto the fetched remote tip if histories
  diverged (e.g. the branch was rebased and re-pushed elsewhere),
- or resets it entirely with `--force` if you want the remote to win outright.

This reconciles your local machine with whatever the other machine already
pushed, instead of re-deriving a second, divergent rebase locally.

### Why not bare `st get` / `st rs`

With **no argument**, `st get` is equivalent to `st sync` (`st rs`): it fetches
**trunk only**, then restacks your local branches onto their local parents
using your *local* commits. It does not re-fetch the SHAs your feature
branches already have on the remote. If those branches were rebased and pushed
from another machine, `st rs`/bare `st get` will rebase your stale local
commits onto the new trunk independently — producing a *second*, different
rebase of the same content, which then needs a force-push (and can conflict
with what's already on the remote).

Use `st sync`/`st rs` to keep trunk current and clean up merged branches; use
`st get <branch>` to pull down branch-level history another machine already
rewrote.

## Typical flow

```bash
# On the machine where you did the work
st refresh          # or: st submit / st restack + push

# On the other machine
st get <top-branch-of-your-stack>
```

If you only remember the bottom branch name, that's fine too — `st get`
resolves the trunk-to-target chain and syncs local upstack branches by
default (`--downstack` opts out).

## Related

- [`st get` reference](../commands/reference.md)
- [Sync and refresh](../commands/core.md)
- [Multi-worktree behavior](multi-worktree.md) — for worktree-local (not
  cross-machine) staleness
