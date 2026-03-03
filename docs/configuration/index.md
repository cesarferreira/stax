# Configuration

```bash
st config
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

[auth]
# use_gh_cli = true
# allow_github_token_env = false
# gh_hostname = "github.company.com"

[ui]
# tips = true

[ai]
# agent = "claude" # or "codex" / "gemini" / "opencode"
# model = "claude-sonnet-4-5-20250929"

[agent]
# worktrees_dir = ".stax/trees"
# default_editor = "auto"   # "auto" | "cursor" | "codex" | "code"
# post_create_hook = ""     # shell command run inside new worktree after creation
```

## Branch naming format

```toml
[branch]
format = "{user}/{date}/{message}"
user = "cesar"
date_format = "%m-%d"
```

The legacy `prefix` field still works when `format` is not set.

## GitHub auth resolution order

1. `STAX_GITHUB_TOKEN`
2. `~/.config/stax/.credentials`
3. `gh auth token` (`auth.use_gh_cli = true`)
4. `GITHUB_TOKEN` (only if `auth.allow_github_token_env = true`)

```bash
st auth status
```

The credentials file is written with `600` permissions.
