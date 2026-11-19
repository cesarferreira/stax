# Quick start

## 1. Onboard

`st setup` configures GitHub auth, shell integration, and optional AI agent skills.

```bash
st setup
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
For token selection and troubleshooting, see the [configuration reference](../configuration/index.md).

</details>

## 2. Ship a stack end-to-end

In a GitHub repo you can push to, start on a clean, up-to-date `main`. Use two new files to give each branch its own commit:

```bash
st create auth-api
printf 'Token refresh API\n' > auth-api-demo.md
git add auth-api-demo.md
git commit -m "Document the token refresh API"

st create auth-ui
printf 'Token refresh UI\n' > auth-ui-demo.md
git add auth-ui-demo.md
git commit -m "Document the token refresh UI"

st ls  # auth-ui sits on auth-api, which sits on main
st ss  # open or update both PRs
```

On GitHub, `auth-api` targets `main`, and `auth-ui` targets `auth-api`. After `auth-api` merges, run this from `auth-ui` to sync trunk, restack, and update the remaining PR:

```bash
st refresh
```

Picked the wrong trunk? `st trunk main` or `st init --trunk <branch>` reconfigures.

## Next

- [Interactive TUI](../interface/tui.md)
- [Merge and cascade](../workflows/merge-and-cascade.md)
- [Core commands](../commands/core.md)
