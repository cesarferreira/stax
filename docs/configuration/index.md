# Configuration

```bash
st config
st config --reset-ai
st config --reset-ai --no-prompt
```

Main config path: `~/.config/stax/config.toml`

## Example

```toml
[branch]
# format = "{user}/{date}/{message}"
# user = "cesar"
# date_format = "%m-%d"
# replacement = "-"

[remote]
# name = "origin"
# base_url = "https://github.com"
# api_base_url = "https://github.company.com/api/v3"
# forge = "github" # "github" | "gitlab" | "gitea" — override auto-detection

[submit]
# stack_links = "comment" # "comment" | "body" | "both" | "off"

[auth]
# use_gh_cli = true
# allow_github_token_env = false
# gh_hostname = "github.company.com"

[ui]
# tips = true

[ai]
# agent = "claude" # or "codex" / "gemini" / "opencode" — global default
# model = "claude-sonnet-4-5-20250929"  # global default model

# Per-feature overrides — each section is optional and falls back to [ai] above
[ai.generate]   # st generate --pr-body
# agent = "codex"
# model = "o4-mini"

[ai.standup]    # st standup --summary
# agent = "gemini"
# model = "gemini-2.5-pro"

[ai.resolve]    # st resolve
# agent = "claude"
# model = "claude-opus-4-5"

[ai.lane]       # st lane / st worktree create --ai
# agent = "claude"
# (model is intentionally not inherited from [ai] for interactive lanes)

[worktree]
# root_dir = "" # default: ~/.stax/worktrees/<repo>

[worktree.hooks]
# post_create = "" # blocking hook run in a new worktree before launch
# post_start = ""  # background hook run after creation
# post_go = ""     # background hook run after entering an existing worktree
# pre_remove = ""  # blocking hook run before removal
# post_remove = "" # background hook run after removal
```

## AI configuration

### Pick agent + model interactively

Run the interactive picker to choose an agent and model for any feature (or as the global default):

```bash
st config --set-ai
```

You'll be asked which feature to configure (`generate`, `standup`, `resolve`, `lane`, or global default), then prompted to pick an agent and model. The selection is written to the appropriate `[ai.*]` section in `~/.config/stax/config.toml`.

### First-use prompting

If no agent is configured for a feature the first time you run it (e.g. `st standup --summary` with no `[ai.standup]` block), stax opens the same interactive picker automatically and persists your choice for future runs — no manual config editing required.

### Resolution order

For every AI-powered command the agent and model are resolved in this order:

| Priority | Source |
|----------|--------|
| 1 | CLI flag (`--agent`, `--model`) |
| 2 | Per-feature config (`[ai.generate]`, `[ai.standup]`, etc.) |
| 3 | Global config (`[ai]`) |
| 4 | Interactive first-use prompt (persisted automatically) |

> **Note:** `[ai.lane]` intentionally does not fall back to `[ai].model`. Interactive coding agents are a different workload from one-shot generation tasks; a cheap model set for `st generate` should not silently apply to a long-running `st lane` session.

### "Using …" confirmation

Whenever stax invokes an AI agent it prints a confirmation line to stderr:

```
  Using claude with model claude-opus-4-5
  Using codex
```

### Reset saved AI defaults

Reset the saved `[ai]` defaults and immediately choose a new agent/model pair:

```bash
st config --reset-ai
```

This clears `ai.agent`, `ai.model`, and all per-feature overrides from `~/.config/stax/config.toml`, then reopens the interactive picker in a real terminal and saves the new selection.

If you only want to clear the saved pairing without prompting:

```bash
st config --reset-ai --no-prompt
```

## Branch naming format

```toml
[branch]
format = "{user}/{date}/{message}"
user = "cesar"
date_format = "%m-%d"
```

The legacy `prefix` field still works when `format` is not set.

## Submit stack links placement

```toml
[submit]
stack_links = "body"
```

`stax submit` can keep the stack links in the PR comment (`comment`), the PR body (`body`), both places (`both`), or remove stax-managed stack links entirely (`off`).

When body output is enabled, stax appends a managed block to the bottom of the PR body and only rewrites that managed block on future submits.

## Forge type override

By default stax detects the forge type (GitHub, GitLab, or Gitea/Forgejo) from the remote hostname. If your self-hosted instance has a generic hostname like `git.mycompany.com`, the auto-detection will fall back to GitHub. Override it explicitly:

```toml
[remote]
base_url = "https://git.mycompany.com"
forge = "gitlab"
```

Accepted values: `"github"`, `"gitlab"`, `"gitea"`, `"forgejo"` (`"forgejo"` is treated as Gitea).

When omitted, auto-detection is used: hostnames containing `gitlab` → GitLab, `gitea`/`forgejo` → Gitea, everything else → GitHub.

### Auth tokens by forge

| Forge  | Auth sources (checked in order)                                                    |
|--------|-------------------------------------------------------------------------------------|
| GitHub | `STAX_GITHUB_TOKEN`, credentials file, `gh` CLI, `GITHUB_TOKEN`                    |
| GitLab | `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, `STAX_FORGE_TOKEN`, credentials file          |
| Gitea  | `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, `STAX_FORGE_TOKEN`, credentials file            |

`stax auth` writes the shared credentials file at `~/.config/stax/.credentials`. That saved token is reused for GitHub, GitLab, and Gitea when forge-specific environment variables are not set.

## GitHub auth resolution order

1. `STAX_GITHUB_TOKEN`
2. `~/.config/stax/.credentials`
3. `gh auth token` (`auth.use_gh_cli = true`)
4. `GITHUB_TOKEN` (only if `auth.allow_github_token_env = true`)

```bash
st auth status
```

The credentials file is written with `600` permissions.
