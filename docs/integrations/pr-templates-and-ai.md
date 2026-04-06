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

`st submit` shows a fuzzy template picker.

### Template flags

- `--template <name>` choose template directly
- `--no-template` skip template
- `--edit` always open editor

## AI PR body generation

Generate and update PR body based on diff, commits, and template:

```bash
st generate --pr-body
```

If you want stax to generate the body and update the PR without the final review prompt:

```bash
st generate --pr-body --no-prompt
```

### Prerequisites

- Current branch must be tracked by stax
- Current branch must already have a PR (for example created via `st submit` / `st ss`)

If no PR exists yet, submit first:

```bash
st ss
st generate --pr-body
```

### Template flags for `generate`

`generate --pr-body` uses the same template selection logic as `submit`:

| Scenario | Behavior |
|---|---|
| `--no-template` | Skip template entirely |
| `--template <name>` | Use the named template; warns and falls back to no template if not found |
| `--no-prompt` + single template | Auto-selects the single available template |
| `--no-prompt` + multiple templates | No template used (avoids silent arbitrary pick) |
| Interactive (default) + single template | Auto-selects the single available template |
| Interactive (default) + multiple templates | Fuzzy picker to choose template |

```bash
st generate --pr-body --template feature
st generate --pr-body --no-template
st generate --pr-body --no-prompt   # auto-selects single template, or no template
```

### Options

- `--agent <name>` override configured agent for one run
- `--model <name>` override model for one run
- `--no-prompt` skip AI picker/review prompts and use defaults
- `--edit` review/edit generated body before update
- `--template <name>` use a specific PR template by name
- `--no-template` skip PR template entirely
- Supported agents: `claude`, `codex`, `gemini`, `opencode`

When `codex` is selected, stax will try OpenAI's live Models API first (using `OPENAI_API_KEY`) before falling back to its local Codex defaults.

If you want stax to forget the saved AI pairing and immediately ask again:

```bash
st config --reset-ai
```

Use `st config --reset-ai --no-prompt` to clear the saved pairing without reopening the picker.

You can also generate during submit:

```bash
st submit --ai-body
```

If you want the stack graph to live in the PR body instead of the usual stax comment, set:

```toml
[submit]
stack_links = "body" # or "both"
```

```bash
st generate --pr-body --agent codex
st generate --pr-body --model claude-haiku-4-5-20251001
st generate --pr-body --agent gemini --model gemini-2.5-flash
st generate --pr-body --agent opencode
st generate --pr-body --no-prompt
st generate --pr-body --edit
st generate --pr-body --template feature
st generate --pr-body --no-template
```
