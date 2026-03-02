# Merge and Cascade

## `stax merge`

`stax merge` merges PRs from the bottom of your stack up to your current branch.
Use `stax merge --when-ready` for the explicit wait-for-ready mode (legacy alias: `stax merge-when-ready` / `stax mwr`).

### What happens

1. Wait for PR readiness (CI + approvals + mergeability) unless `--no-wait`
2. Merge PR with selected strategy
3. Rebase next branch onto updated trunk
4. Update next PR base
5. Force-push updated branch
6. Repeat until done
7. Run post-merge sync (`stax rs --force`) unless `--no-sync`

### Common options

```bash
stax merge --dry-run
stax merge --all
stax merge --method squash
stax merge --method merge
stax merge --method rebase
stax merge --when-ready
stax merge --when-ready --interval 10
stax merge --no-wait
stax merge --no-delete
stax merge --no-sync
stax merge --timeout 60
stax merge --yes
```

`--when-ready` cannot be combined with `--dry-run` or `--no-wait`.

### Partial stack merge

```bash
# Stack: main <- auth <- auth-api <- auth-ui <- auth-tests
stax checkout auth-api
stax merge
```

This merges up to `auth-api` and leaves upper branches to merge later.

When merged branches are removed from the middle of a stack, follow-up restacks use a provenance-aware `rebase --onto` boundary so descendants replay only their novel commits (works for merge, rebase, and common squash-merge flows).

## `stax cascade`

`stax cascade` combines restack + push + PR create/update in one flow.

| Command | Behavior |
|---|---|
| `stax cascade` | restack -> push -> create/update PRs |
| `stax cascade --no-pr` | restack -> push |
| `stax cascade --no-submit` | restack only |
| `stax cascade --auto-stash-pop` | auto stash/pop dirty worktrees |
