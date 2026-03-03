# Merge and Cascade

## `st merge`

`st merge` merges PRs from the bottom of your stack up to your current branch.
Use `st merge --when-ready` for the explicit wait-for-ready mode (legacy alias: `st merge-when-ready` / `st mwr`).

### What happens

1. Wait for PR readiness (CI + approvals + mergeability) unless `--no-wait`
2. Merge PR with selected strategy
3. Rebase next branch onto updated trunk
4. Update next PR base
5. Force-push updated branch
6. Repeat until done
7. Run post-merge sync (`st rs --force`) unless `--no-sync`

### Common options

```bash
st merge --dry-run
st merge --all
st merge --method squash
st merge --method merge
st merge --method rebase
st merge --when-ready
st merge --when-ready --interval 10
st merge --no-wait
st merge --no-delete
st merge --no-sync
st merge --timeout 60
st merge --yes
```

`--when-ready` cannot be combined with `--dry-run` or `--no-wait`.

### Partial stack merge

```bash
# Stack: main <- auth <- auth-api <- auth-ui <- auth-tests
st checkout auth-api
st merge
```

This merges up to `auth-api` and leaves upper branches to merge later.

During merge flows, descendant branches are rebased with provenance-aware boundaries so already-integrated parent commits are not replayed after squash merges. Follow-up restacks also auto-normalize missing/merged-equivalent parents and keep old boundaries so descendants replay only novel commits.

## `st cascade`

`st cascade` combines restack + push + PR create/update in one flow.

| Command | Behavior |
|---|---|
| `st cascade` | restack -> push -> create/update PRs |
| `st cascade --no-pr` | restack -> push |
| `st cascade --no-submit` | restack only |
| `st cascade --auto-stash-pop` | auto stash/pop dirty worktrees |
