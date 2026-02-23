# PR Templates and AI PR Bodies

## PR templates

stax discovers templates from your repository.

### Single template

If `.github/PULL_REQUEST_TEMPLATE.md` exists, stax uses it automatically.

### Multiple templates

Use `.github/PULL_REQUEST_TEMPLATE/` with one file per template.

```text
.github/
  PULL_REQUEST_TEMPLATE/
    feature.md
    bugfix.md
    docs.md
```

`stax submit` shows a fuzzy template picker.

### Template flags

- `--template <name>` choose template directly
- `--no-template` skip template
- `--edit` always open editor

## AI PR body generation

Generate and update PR body based on diff, commits, and template:

```bash
stax generate --pr-body
```

### Prerequisites

- Current branch must be tracked by stax
- Current branch must already have a PR (for example created via `stax submit` / `stax ss`)

If no PR exists yet, submit first:

```bash
stax ss
stax generate --pr-body
```

### Options

- `--agent <name>` override configured agent for one run
- `--model <name>` override model for one run
- `--edit` review/edit generated body before update
- Supported agents: `claude`, `codex`, `gemini`, `opencode`

You can also generate during submit:

```bash
stax submit --ai-body
```

```bash
stax generate --pr-body --agent codex
stax generate --pr-body --model claude-haiku-4-5-20251001
stax generate --pr-body --agent gemini --model gemini-2.5-flash
stax generate --pr-body --agent opencode
stax generate --pr-body --edit
```
