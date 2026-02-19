# Configuration

```bash
stax config
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
# agent = "claude"
# model = "claude-sonnet-4-5-20250929"
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
stax auth status
```

The credentials file is written with `600` permissions.
