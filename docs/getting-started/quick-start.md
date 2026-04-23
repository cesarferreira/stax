# Quick start

## 1. Onboard

`st setup` is the recommended one-shot: shell integration, AI agent skills, and GitHub auth.

```bash
st setup --yes
```

<details>
<summary>Alternative auth paths</summary>

```bash
# Import from the GitHub CLI
gh auth login && st auth --from-gh

# Enter a personal access token interactively
st auth

# Or via env var (stax-specific)
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default stax ignores ambient `GITHUB_TOKEN`. Opt in with `auth.allow_github_token_env = true`.

</details>

## 2. Ship a stack end-to-end

```bash
# Stack two branches on trunk
st create auth-api
st create auth-ui

# See the stack
st ls
# ◉  auth-ui 1↑
# ○  auth-api 1↑
# ○  main

# Submit the whole stack as linked PRs
st ss

# After the bottom PR merges on GitHub, catch up in one shot:
st rs --restack    # pull trunk, clean merged, rebase the rest
# or: st refresh    # sync + restack + push/update PRs in one command
# scripts: st refresh --force --yes --no-prompt
```

Picked the wrong trunk? `st trunk main` or `st init --trunk <branch>` reconfigures.

## Next

- [Interactive TUI](../interface/tui.md)
- [Merge and cascade](../workflows/merge-and-cascade.md)
- [Core commands](../commands/core.md)
