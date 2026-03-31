# Standup and Changelog

## Standup summary

```bash
st standup                   # Last 24 hours (default)
st standup --hours 48        # Look back further
st standup --all             # Include all stacks, not just current
st standup --json            # Raw activity data as JSON
```

![Standup summary](../assets/standup.png)

Shows merged PRs, opened PRs, recent pushes, and items that need attention.
Works with GitHub, GitLab, and Gitea repositories.

> **Note:** "Reviews given" data is not yet available on any forge. Efficiently
> querying reviews authored by a user requires GraphQL (GitHub) or iterating
> every open PR (GitLab/Gitea), which is too slow for large repositories.

## AI standup summary

Generate a concise spoken-style summary of your activity using an AI agent:

```bash
st standup --summary
st standup --summary --hours 48
st standup --summary --agent claude
st standup --summary --agent gemini
st standup --summary --jit
```

Uses the AI agent configured under `[ai]` in `~/.config/stax/config.toml` (same agent as `st generate --pr-body`). Override for a single run with `--agent`.

When `--jit` is enabled, standup also inspects your current Jira sprint via the `jit` CLI and feeds the AI two extra signals:
- tickets that already have PRs in flight
- likely next backlog tickets without PRs yet

Install or learn more about `jit`: <https://github.com/cesarferreira/jit>

The summary is word-wrapped and displayed in a card that fits your terminal width:

```
  ✓ Generating standup summary with codex        4.1s

  ╭──────────────────────────────────────────────────────────────────╮
  │                                                                  │
  │  Yesterday I finished the billing webhook retry fix and split    │
  │  the reporting dashboard cleanup into two PRs so review stays    │
  │  small. I also opened a third PR to speed up integration tests   │
  │  by caching seed data, and all three are now in review. Today    │
  │  I'm handling review feedback and preparing the next analytics   │
  │  task.                                                           │
  │                                                                  │
  ╰──────────────────────────────────────────────────────────────────╯
```

Key phrases are highlighted: completed work in green, new work in cyan, reviews in blue, and upcoming tasks in yellow.

### Output formats

```bash
st standup --summary                   # Spinner + colored card (default)
st standup --summary --plain-text      # Raw text, no colors — pipe-friendly
st standup --summary --json            # {"summary": "..."} JSON
st standup --summary --jit             # Add Jira context via jit
```

### Prerequisites

- An AI agent installed and on `PATH`: `claude`, `codex`, `gemini`, or `opencode`
- For `--jit`, install [`jit`](https://github.com/cesarferreira/jit) and make sure it is on `PATH`
- Agent configured in `~/.config/stax/config.toml`:

```toml
[ai]
agent = "claude"   # or "codex", "gemini", "opencode"
```

Or pass `--agent` directly to skip config.

## Changelog generation

```bash
st changelog                      # Auto-detect last tag → HEAD
st changelog v1.0.0               # Explicit from ref → HEAD
st changelog v1.0.0 v2.0.0       # Between two explicit refs
st changelog abc123 def456        # Between two commits
```

### Tag prefix (monorepo releases)

In a monorepo with tags like `release/ios/v1.2.0` and `release/android/v3.0.0`,
use `--tag-prefix` to pick the latest tag matching a platform:

```bash
st changelog --tag-prefix release/ios
st changelog --tag-prefix release/android
st changelog --tag-prefix release/android --json
```

### Path filtering

```bash
st changelog v1.0.0 --path apps/frontend
st changelog v1.0.0 --path packages/shared-utils
```

### JSON output

```bash
st changelog v1.0.0 --json
st changelog --json               # Auto-resolved tag appears in "resolved_from"
```

PR numbers are extracted from squash-merge commit messages like `(#123)`.
