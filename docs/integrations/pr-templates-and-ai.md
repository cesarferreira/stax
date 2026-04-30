# PR templates and AI PR details

## PR templates

stax discovers templates from your repository automatically.

### Single template

If `.github/PULL_REQUEST_TEMPLATE.md` exists, stax uses it.

### Multiple templates

Use `.github/PULL_REQUEST_TEMPLATE/` with one file per template:

```text
.github/
  PULL_REQUEST_TEMPLATE/
    feature.md
    bugfix.md
    docs.md
```

`st submit` shows a fuzzy template picker.

### Template flags

| Flag | Behavior |
|---|---|
| `--template <name>` | Use a specific template |
| `--no-template` | Skip template entirely |
| `--edit` | Always open the editor |

## AI PR details during submit

Generate PR titles and bodies while submitting:

```bash
st ss --ai                  # suggest title/body and prompt before updating
st bs --ai --body           # current branch only, body generation only
st ss --ai --yes            # accept generated details for new PRs
st ss --ai --body --yes     # refresh existing PR bodies automatically
```

`--title` and `--body` narrow what AI generates. Without either modifier, `--ai` targets both title and body.

For existing PRs, interactive `--ai` asks whether to update title, body, both, or skip. With `--yes`, plain `--ai` leaves existing PR content alone; explicit `--title` and/or `--body` updates those fields automatically.

## AI PR body refresh

Generate or update a PR body using diff, commits, and template:

```bash
st generate --pr-body
st generate --pr-body --no-prompt   # skip final review prompt
```

### Prerequisites

- Current branch is tracked by stax
- Current branch already has a PR (e.g. created via `st submit` / `st ss`)

If no PR exists yet:

```bash
st ss
st generate --pr-body
```

### Template behavior for `generate`

`generate --pr-body` uses the same template logic as `submit`:

| Scenario | Behavior |
|---|---|
| `--no-template` | Skip template entirely |
| `--template <name>` | Use the named template (warns + falls back if not found) |
| `--no-prompt` + single template | Auto-select the single template |
| `--no-prompt` + multiple templates | No template (avoids silent arbitrary pick) |
| Interactive + single template | Auto-select the single template |
| Interactive + multiple templates | Fuzzy picker |

```bash
st generate --pr-body --template feature
st generate --pr-body --no-template
st generate --pr-body --no-prompt
```

### Options

| Flag | Behavior |
|---|---|
| `--agent <name>` | Override configured agent for one run |
| `--model <name>` | Override model for one run |
| `--no-prompt` | Skip picker/review prompts, use defaults |
| `--edit` | Review/edit generated body before update |
| `--template <name>` | Use a specific PR template |
| `--no-template` | Skip PR template |

Supported agents: `claude`, `codex`, `gemini`, `opencode`. When `codex` is selected, stax tries OpenAI's live Models API first (using `OPENAI_API_KEY`) before falling back to local Codex defaults.

To forget the saved AI pairing and re-prompt:

```bash
st config --reset-ai
st config --reset-ai --no-prompt   # clear without opening picker
```

### Stack graph placement

Put the stack graph in the PR body instead of the default stax comment:

```toml
[submit]
stack_links = "body"   # or "both"
```

### More examples

```bash
st generate --pr-body --agent codex
st generate --pr-body --model claude-haiku-4-5-20251001
st generate --pr-body --agent gemini --model gemini-2.5-flash
st generate --pr-body --agent opencode
st generate --pr-body --edit
st generate --pr-body --template feature
```
