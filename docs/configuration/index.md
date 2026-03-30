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
# agent = "claude" # or "codex" / "gemini" / "opencode"
# model = "claude-sonnet-4-5-20250929"

[worktree]
# root_dir = "" # default: ~/.stax/worktrees/<repo>

[worktree.hooks]
# post_create = "" # blocking hook run in a new worktree before launch
# post_start = ""  # background hook run after creation
# post_go = ""     # background hook run after entering an existing worktree
# pre_remove = ""  # blocking hook run before removal
# post_remove = "" # background hook run after removal
```

## Reset saved AI defaults

Reset the saved `[ai]` defaults and immediately choose a new agent/model pair:

```bash
st config --reset-ai
```

This clears `ai.agent` and `ai.model` from `~/.config/stax/config.toml`, then reopens the interactive picker in a real terminal and saves the new selection.

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

| Forge  | Environment variables (checked in order)                        |
|--------|-----------------------------------------------------------------|
| GitHub | `STAX_GITHUB_TOKEN`, credentials file, `gh` CLI, `GITHUB_TOKEN`|
| GitLab | `STAX_GITLAB_TOKEN`, `GITLAB_TOKEN`, `STAX_FORGE_TOKEN`        |
| Gitea  | `STAX_GITEA_TOKEN`, `GITEA_TOKEN`, `STAX_FORGE_TOKEN`          |

## GitHub auth resolution order

1. `STAX_GITHUB_TOKEN`
2. `~/.config/stax/.credentials`
3. `gh auth token` (`auth.use_gh_cli = true`)
4. `GITHUB_TOKEN` (only if `auth.allow_github_token_env = true`)

```bash
st auth status
```

The credentials file is written with `600` permissions.
