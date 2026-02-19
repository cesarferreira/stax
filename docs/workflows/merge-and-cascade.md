# Merge and Cascade

## `stax merge`

`stax merge` merges PRs from the bottom of your stack up to your current branch.

### What happens

1. Wait for CI (unless `--no-wait`)
2. Merge PR with selected strategy
3. Rebase next branch onto updated trunk
4. Update next PR base
5. Force-push updated branch
6. Repeat until done

### Common options

```bash
stax merge --dry-run
stax merge --all
stax merge --method squash
stax merge --method merge
stax merge --method rebase
stax merge --no-wait
stax merge --no-delete
stax merge --timeout 60
stax merge --yes
```

### Partial stack merge

```bash
# Stack: main <- auth <- auth-api <- auth-ui <- auth-tests
stax checkout auth-api
stax merge
```

This merges up to `auth-api` and leaves upper branches to merge later.

## `stax cascade`

`stax cascade` combines restack + push + PR create/update in one flow.

| Command | Behavior |
|---|---|
| `stax cascade` | restack -> push -> create/update PRs |
| `stax cascade --no-pr` | restack -> push |
| `stax cascade --no-submit` | restack only |
| `stax cascade --auto-stash-pop` | auto stash/pop dirty worktrees |
