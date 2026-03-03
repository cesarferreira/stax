# Standup and Changelog

## Standup summary

```bash
stax standup                   # Last 24 hours (default)
stax standup --hours 48        # Look back further
stax standup --all             # Include all stacks, not just current
stax standup --json            # Raw activity data as JSON
```

![Standup summary](../assets/standup.png)

Shows merged PRs, opened PRs, recent pushes, and items that need attention.

## AI standup summary

Generate a concise spoken-style summary of your activity using an AI agent:

```bash
stax standup --summary
stax standup --summary --hours 48
stax standup --summary --agent claude
stax standup --summary --agent gemini
stax standup --summary --jit
```

Uses the AI agent configured under `[ai]` in `~/.config/stax/config.toml` (same agent as `stax generate --pr-body`). Override for a single run with `--agent`.

When `--jit` is enabled, standup also inspects your current Jira sprint via the `jit` CLI and feeds the AI two extra signals:
- tickets that already have PRs in flight
- likely next backlog tickets without PRs yet

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
stax standup --summary                   # Spinner + colored card (default)
stax standup --summary --plain-text      # Raw text, no colors — pipe-friendly
stax standup --summary --json            # {"summary": "..."} JSON
stax standup --summary --jit             # Add Jira context via jit
```

### Prerequisites

- An AI agent installed and on `PATH`: `claude`, `codex`, `gemini`, or `opencode`
- Agent configured in `~/.config/stax/config.toml`:

```toml
[ai]
agent = "claude"   # or "codex", "gemini", "opencode"
```

Or pass `--agent` directly to skip config.

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
