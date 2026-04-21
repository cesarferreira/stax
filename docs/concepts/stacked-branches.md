# Stacked branches

Instead of one giant PR, split work into a chain of small branches that build on each other. Each branch is its own focused PR.

## Why it works

- **Smaller reviews** with clearer scope
- **Parallel progress** — keep stacking while lower PRs are in review
- **Safer shipping** — merge foundations first, derive the rest
- **Cleaner history** for reading and rollback

## The shape of a stack

```text
◉  feature/auth-ui    1↑
○  feature/auth-api   1↑
○  main
```

Three focused PRs — each depending on the branch below it — instead of one 2,000-line monolith.

## A real flow

```bash
# Build bottom-up
st create payments-models
st create payments-api
st create payments-ui

# Submit as three linked PRs
st ss
```

After the bottom PR merges on GitHub:

```bash
st rs --restack
```

`rs` pulls trunk and deletes the merged branch; `--restack` rebases the rest onto updated trunk and updates PR bases.

## Related

- [Multiple stacks](multiple-stacks.md)
- [Merge and cascade](../workflows/merge-and-cascade.md)
- [Quick start](../getting-started/quick-start.md)
