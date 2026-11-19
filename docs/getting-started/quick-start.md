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

In a GitHub repo you can push to, start on a clean, up-to-date `main`. Create an API branch, then stack the UI and tests on top of it as siblings:

```bash
st create auth-api
echo "Token refresh API" > auth-api-demo.md
git add auth-api-demo.md
git commit -m "Document the token refresh API"

st create auth-ui
echo "Token refresh UI" > auth-ui-demo.md
git add auth-ui-demo.md
git commit -m "Document the token refresh UI"

st create auth-tests --from auth-api
echo "Token refresh test cases" > auth-tests-demo.md
git add auth-tests-demo.md
git commit -m "Outline the token refresh tests"

st ls
```

```text
◉       auth-tests 1↑
│ ○     auth-ui 1↑
○─┘     auth-api 1↑
○    ☁️ main
```

`◉` marks the checked-out branch. Read the stack from the bottom up: `auth-api` builds on `main`, and `auth-ui` and `auth-tests` both build on `auth-api`. To submit both sides of the split, run:

```bash
git switch auth-api
st upstack submit
```

On GitHub, `auth-api` targets `main`, while `auth-ui` and `auth-tests` both target `auth-api`. After `auth-api` merges, run this from each remaining branch to sync trunk, restack, and update its PR:

```bash
st refresh
```

Picked the wrong trunk? `st trunk main` or `st init --trunk <branch>` reconfigures.

## Next

- [Interactive TUI](../interface/tui.md)
- [Merge and cascade](../workflows/merge-and-cascade.md)
- [Core commands](../commands/core.md)
