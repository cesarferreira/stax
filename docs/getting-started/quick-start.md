# Quick Start

Set up GitHub auth first (required for PR creation, CI checks, and review metadata).

```bash
# Option A (recommended): use GitHub CLI auth
gh auth login
st auth --from-gh

# Option B: enter a personal access token manually
st auth

# Option C: provide a stax-specific env var
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default, stax does not use ambient `GITHUB_TOKEN` unless you opt in with `auth.allow_github_token_env = true`.

```bash
# 1. Create stacked branches
st create auth-api
st create auth-ui

# 2. View your stack
st ls
# ◉  auth-ui 1↑
# ○  auth-api 1↑
# ○  main

# 3. Submit PRs for the whole stack
st ss

# 4. After auth-api PR is merged on GitHub...

# Pull trunk, detect the merge, delete auth-api, reparent auth-ui → main
st rs

# Rebase auth-ui onto updated main
st restack

# Or do both in one shot:
st rs --restack
```

## Next

- [Interactive TUI](../interface/tui.md)
- [Merge and Cascade](../workflows/merge-and-cascade.md)
