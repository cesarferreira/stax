# What Are Stacked Branches?

Instead of one massive PR, stacked branches split work into small reviewable pieces that build on each other.

## Why this works well

- Smaller reviews with clearer scope
- Parallel progress while lower PRs are being reviewed
- Safer shipping by merging foundations first
- Cleaner history for understanding and rollback

## Example stack

```text
◉  feature/auth-ui 1↑
○  feature/auth-api 1↑
○  main
```

Each branch is a focused PR. Reviewers see smaller diffs, and your stack keeps moving.

## Real-world flow

```bash
# Start the foundation
stax create payments-models

# Stack the API layer
stax create payments-api

# Stack the UI layer
stax create payments-ui

# Submit as separate PRs
stax ss
```

After the bottom PR merges:

```bash
stax rs --restack
```

stax rebases the rest of the stack and updates PR bases.
