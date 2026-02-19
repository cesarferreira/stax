# Quick Start

Set up GitHub auth first (required for PR creation, CI checks, and review metadata).

```bash
# Option A (recommended): use GitHub CLI auth
gh auth login
stax auth --from-gh

# Option B: enter a personal access token manually
stax auth

# Option C: provide a stax-specific env var
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default, stax does not use ambient `GITHUB_TOKEN` unless you opt in with `auth.allow_github_token_env = true`.

```bash
# 1. Create stacked branches
stax create auth-api
stax create auth-ui

# 2. View your stack
stax ls
# ◉  auth-ui 1↑
# ○  auth-api 1↑
# ○  main

# 3. Submit PRs for the whole stack
stax ss

# 4. Sync and rebase after merges
stax rs --restack
```

## Next

- [Interactive TUI](../interface/tui.md)
- [Merge and Cascade](../workflows/merge-and-cascade.md)
