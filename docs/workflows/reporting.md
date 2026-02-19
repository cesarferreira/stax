# Standup and Changelog

## Standup summary

```bash
stax standup
stax standup --hours 48
stax standup --json
```

![Standup summary](../assets/standup.png)

Shows merged PRs, opened PRs, recent pushes, and items that need attention.

## Changelog generation

```bash
stax changelog v1.0.0
stax changelog v1.0.0 v2.0.0
stax changelog abc123 def456
```

### Monorepo filtering

```bash
stax changelog v1.0.0 --path apps/frontend
stax changelog v1.0.0 --path packages/shared-utils
```

### JSON output

```bash
stax changelog v1.0.0 --json
```

PR numbers are extracted from squash-merge commit messages like `(#123)`.
