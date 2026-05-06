# Configuration

```bash
st config                     # show current configuration
st config --set-ai            # interactively pick AI agent/model
st config --reset-ai          # clear saved AI defaults and re-prompt
st config --reset-ai --no-prompt
```

Config is resolved in this order:

1. `STAX_CONFIG_DIR/config.toml` when `STAX_CONFIG_DIR` is set.
2. `.config/stax/config.toml` at the current git repository root when it exists.
3. `~/.config/stax/config.toml`.

## Example config

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

[ci]
# alert = false
# success_alert_sound = "/path/to/ci-success.wav"
# error_alert_sound = "/path/to/ci-error.wav"

[auth]
# use_gh_cli = true
# allow_github_token_env = false
# gh_hostname = "github.company.com"

[ui]
# tips = true

[ai]
# agent = "claude" # "codex" | "gemini" | "opencode" — global default
# model = "claude-sonnet-4-5-20250929"

# Per-feature overrides — optional, fall back to [ai] above
[ai.generate]   # st create --ai, st gen / st generate, st submit --ai
# agent = "codex"
# model = "o4-mini"

[ai.standup]    # st standup --ai
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
# post_start  = "" # background hook after creation
# post_go     = "" # background hook after entering an existing worktree
# pre_remove  = "" # blocking hook before removal
# post_remove = "" # background hook after removal
#
# Example — keep VS Code / Cursor aware of every lane:
#   post_start = "code --add ."
#   post_go    = "code --add ."
```

## AI configuration

### Set agent + model

Pick an agent and model for any feature (or the global default):

```bash
st config --set-ai
```

You're asked which feature to configure (`generate`, `standup`, `resolve`, `lane`, or global default), then prompted for agent and model. The choice is written to the appropriate `[ai.*]` section.

### First-use prompting

The first time you run an AI-powered command without a configured agent (e.g. `st standup --ai`), stax opens the picker automatically and persists the choice for future runs — no manual config editing required.

### Resolution order

For AI-powered commands, agent and model are resolved in this order:

| Priority | Source |
|---|---|
| 1 | CLI flag (`--agent`, `--model`) where the command exposes one |
| 2 | Per-feature config (`[ai.generate]`, `[ai.standup]`, …) |
| 3 | Global config (`[ai]`) |
| 4 | Interactive first-use prompt (persisted) |

> **Note:** `[ai.lane]` intentionally does not fall back to `[ai].model`. Interactive coding agents are a different workload from one-shot generation; a cheap model set for `st generate` should not silently apply to a long-running `st lane` session.

### "Using …" confirmation

When stax invokes an AI agent it prints a confirmation line to stderr:

```text
  Using claude with model claude-opus-4-5
  Using codex
```

### Reset saved defaults

```bash
st config --reset-ai              # clear + re-prompt
st config --reset-ai --no-prompt  # clear only
```

## CI watch alerts

```toml
[ci]
alert = true
# success_alert_sound = "/path/to/ci-success.wav"
# error_alert_sound = "/path/to/ci-error.wav"
```

When `alert` is true, `st ci --watch` plays bundled success/error sounds after CI completes. Set either path to override one outcome while keeping the other bundled default.

## Branch naming format

```toml
[branch]
format = "{user}/{date}/{message}"
user = "cesar"
date_format = "%m-%d"
```

The legacy `prefix` field still works when `format` is unset.

## Stack-links placement

Where `st submit` writes the stack graph for a PR:

```toml
[submit]
stack_links = "body"   # "comment" | "body" | "both" | "off"
```

When body output is enabled, stax appends a managed block to the bottom of the PR body and only rewrites that managed block on future submits.

## Forge type override

By default stax detects the forge from the remote hostname. If your self-hosted instance has a generic hostname like `git.mycompany.com`, override it:

```toml
[remote]
base_url = "https://git.mycompany.com"
forge = "gitlab"
```

Accepted values: `"github"`, `"gitlab"`, `"gitea"`, `"forgejo"` (Forgejo is treated as Gitea).

Auto-detection fallback: hostnames containing `gitlab` → GitLab, `gitea`/`forgejo` → Gitea, otherwise → GitHub.

## Auth tokens by forge

| Forge | Auth sources (checked in order) |
|---|---|
| GitHub | `STAX_GITHUB_TOKEN`, credentials file, `gh` CLI, `GITHUB_TOKEN` |
| GitLab | `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, `STAX_FORGE_TOKEN`, credentials file |
| Gitea | `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, `STAX_FORGE_TOKEN`, credentials file |

`stax auth` writes `~/.config/stax/.credentials` (mode `600`). That shared token is reused for GitHub, GitLab, and Gitea when forge-specific env vars are not set.

### GitHub resolution order

1. `STAX_GITHUB_TOKEN`
2. `~/.config/stax/.credentials`
3. `gh auth token` (`auth.use_gh_cli = true`)
4. `GITHUB_TOKEN` (only when `auth.allow_github_token_env = true`)

```bash
st auth status
```
