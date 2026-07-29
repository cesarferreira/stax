# Configuration

```bash
st config                     # show current configuration
st config --set-ai            # interactively pick AI agent/model
st config --reset-ai          # clear saved AI defaults and re-prompt
st config --reset-ai --no-prompt
st --default-config          # print annotated config template (all options + allowed values)
```

Config is loaded as follows:

1. `STAX_CONFIG_DIR/config.toml` when `STAX_CONFIG_DIR` is set.
2. Otherwise, `~/.config/stax/config.toml` is loaded.
3. If present, `stax.toml` at the current git repository root overlays only the values it sets.

## Example config

Run `st --default-config` for the full annotated template (every section, with allowed values in comments). Copy the output to `~/.config/stax/config.toml` and uncomment only the keys you want to override.

Common overrides:

```toml
[submit]
stack_links = "body"   # "comment" | "body" | "both" | "off"
single_stack = "on"    # "on" | "off"
```

<details>
<summary>Full template (same as <code>st --default-config</code>)</summary>

```toml
[branch]
# format = "{user}/{date}/{message}"
# user = "cesar"
# date_format = "%m-%d"
# replacement = "-"
# stale_days = 30 # days without commits before `stax sweep` calls a branch stale

[git]
# rerere = true # auto-enable git rerere on `stax init`

[remote]
# name = "origin"
# base_url = "https://github.com"
# api_base_url = "https://github.company.com/api/v3"
# forge = "github" # "github" | "gitlab" | "gitea" — override auto-detection
# auto_fork = false # always fall back to submitting from a fork when the upstream push is denied
# fork_remote = "fork" # name of a pre-configured git remote for the fork (skip auto-detect)

[submit]
# stack_links = "comment" # "comment" | "body" | "both" | "off"
# single_stack = "on"     # "on" | "off" — when "off", skip stack-link sync while the stack has only one PR
# native_stack = "auto"   # "auto" | "off" | "link" — GitHub gh-stack registration; use "off" to disable entirely
# stack_links_when_native = "keep" # "keep" | "off" — stax PR links when native registration succeeds

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

[display]
# worktree_glyph = "auto" # "auto" | "tree" (Nerd Font) | "wt" (ASCII)

[restack]
# preflight_auto_repair = true # automatically use merge-base when stored parent
                               # boundary would replay a much larger range
# preflight_warn = true        # print a notice when that automatic repair happens

[skills]
# harnesses = ["claude", "codex"] # agent harnesses that receive skill files
#   valid ids: claude, codex, cursor, opencode, pi. Unset = auto (detected + already installed).

[ai]
# agent = "claude" # "codex" | "gemini" | "opencode" | "pi" — global default
# model = "claude-opus-4-8"

# Per-feature overrides — optional, fall back to [ai] above
[ai.generate]   # st create --ai, st gen / st generate, st submit --ai
# agent = "codex"
# model = "o4-mini"
# title = "Prefix PR titles with the issue key"
# body = "Include testing and rollout sections"

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
# root_dir = ".." # optional: keep lanes beside a nested trunk clone
# reuse_slots = true
#   Warm-slot recycling. Removing a clean, merged-equivalent worktree parks it
#   (reset --hard trunk + `git clean -fd`, keeping gitignored deps like
#   node_modules / .venv) instead of deleting it, and the next create/lane adopts
#   that slot instead of a cold `git worktree add`. Set false to always
#   cold-create and real-remove (no pool manifest).
# max_idle_slots = 4
#   Maximum idle slots kept parked. Parking beyond the cap does a real remove;
#   `worktree cleanup` evicts the oldest excess slots.
# reconcile = "pnpm install"
#   Optional command run (non-fatally) inside a slot after it is adopted, to
#   re-sync dependencies. A missing or failing command only warns.

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

</details>

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

### PR title and body instructions

`[ai.generate]` accepts optional `title` and `body` instruction strings for repository writing conventions:

```toml
[ai.generate]
title = "Prefix titles with the issue key and use imperative mood"
body = """
Include a Testing section.
Call out migrations and rollout risk when applicable.
"""
```

Set these in `~/.config/stax/config.toml` for global defaults or in a repo-root `stax.toml` for project rules. Repository values overlay matching global values one field at a time, so a repository can override `title` while inheriting the global `body` instruction. Missing, empty, and whitespace-only values add no instructions to prompts.

The `title` instruction applies to `st generate --pr-title` and title generation in `st submit --ai`. The `body` instruction applies to `st generate --pr-body` and body generation in `st submit --ai`. They do not affect `st generate --commit-msg`, branch names, or other AI features. Stax appends its built-in JSON-only or markdown-only output rule after custom instructions, so that output contract remains authoritative.

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

Reset clears saved global and per-feature agent/model choices. It preserves `title` and `body` instructions.

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

## Stale-branch threshold

```toml
[branch]
stale_days = 60
```

`stale_days` is the number of days without new commits before [`stax sweep`](../commands/sweep.md) classifies a branch as `stale` (default: `30`). The `stax sweep --stale-days <N>` flag overrides this per run.

## Git rerere

```toml
[git]
rerere = false
```

When `rerere` is `true` (the default), `stax init` enables Git's [rerere](https://git-scm.com/docs/git-rerere) ("reuse recorded resolution") so previously resolved merge conflicts are replayed automatically during restacks. Set it to `false` to leave your global Git configuration untouched on init.

## Stack-links placement

Where `st submit` writes the stack graph for a PR:

```toml
[submit]
stack_links = "body"   # "comment" | "body" | "both" | "off"
single_stack = "on"    # "on" | "off"
```

When body output is enabled, stax appends a managed block to the bottom of the PR body and only rewrites that managed block on future submits.

Stack-link entries use compact PR/MR references and mark the PR being rendered with `👈`. On GitHub, stax keeps native `#123` PR references so GitHub renders its standard linked issue/PR styling; other forges use direct markdown links. The intro text is relative to the PR being rendered, so an imported base PR is described as an imported reference, while a local PR calls out any imported downstack context. Imported branches remain read-only for push and PR metadata updates, but their existing PRs still receive the managed stack links when they are part of the displayed stack.

`single_stack` controls whether stack links are written when the stack contains only one PR. With the default `"on"`, links are always synced per `stack_links`. With `"off"`, stax skips link sync — and removes any stale links left over from a previous `"on"` setting — while the stack has a single PR. As soon as a second PR is submitted on the same stack, links populate on every PR (including the original) automatically.

## Native GitHub Stacked PRs (gh-stack)

```toml
[submit]
native_stack = "auto"              # "auto" | "off" | "link"
stack_links_when_native = "keep"   # "keep" | "off"
```

`native_stack = "auto"` is the default. On GitHub remotes, stax checks for the `github/gh-stack` extension and tries to register submitted multi-PR stacks with GitHub's native Stacked PRs feature. If the extension is missing, the repo does not have private-preview access, or the remote is not GitHub, submit silently keeps the existing stax behavior.

### Disable gh-stack registration

To stop stax from calling `gh stack link` on submit (while keeping normal stax submit and PR body/comment stack links):

```toml
[submit]
native_stack = "off"
```

Per run only: `st submit --no-native-stack`. To force registration once: `st submit --native-stack`.

Use `native_stack = "link"` to always attempt registration even when the repo's feature cache says the feature is disabled.

`stack_links_when_native = "keep"` preserves stax's body/comment stack links when native registration succeeds. Set it to `"off"` only if you want the GitHub native stack map without stax-managed PR body/comment links.

`st doctor` reports the installed `gh-stack` version, marks versions below v0.1.0 as out of date, and can install or upgrade the extension after confirmation with `st doctor --fix`. `native_stack = "off"` also disables `st merge --stack`'s atomic `gh stack merge` delegation (see below), falling back to the forge-API stack merge for every run.

### Atomic stack merge (`gh stack merge`)

With `github/gh-stack` v0.1.0+ and a confirmed-enabled native GitHub Stack, `st merge --stack` delegates to `gh stack merge` to land the selected range atomically instead of retargeting and merging PRs individually. This has no separate config flag — it follows `native_stack` (skipped when `"off"`) and the repo's native-stack feature cache.

→ [Native GitHub Stacks guide](../integrations/github-native-stacks.md)

## Forge type override

By default stax detects the forge from the remote hostname. If your self-hosted instance has a generic hostname like `git.mycompany.com`, override it:

```toml
[remote]
base_url = "https://git.mycompany.com"
forge = "gitlab"
```

Accepted values: `"github"`, `"gitlab"`, `"gitea"`, `"forgejo"` (Forgejo is treated as Gitea).

Auto-detection fallback: hostnames containing `gitlab` → GitLab, `gitea`/`forgejo` → Gitea, otherwise → GitHub.

## Fork fallback for submit

When `stax branch submit` pushes to an upstream you lack write access to, GitHub rejects the push. Opt in to a fork-based fallback so stax re-runs the push against your fork instead:

```toml
[remote]
auto_fork = true          # always fall back to a fork on permission-denied push
fork_remote = "fork"      # optional: reuse an existing git remote you added yourself
```

Or use `stax branch submit --fork` for a one-off. When enabled:

- stax reuses a pushable fork under your GitHub login, or creates one via the GitHub API when none exists.
- the branch is pushed with `--force-with-lease` (no bare `--force`); a diverged fork branch fails actionably rather than silently overwriting it.
- the PR is opened with head `<fork_owner>:<branch>` and `maintainer_can_modify = true`.
- an existing git remote named `fork` is never silently repointed; the run fails if its URL conflicts.

Supported forges: GitHub only. Fork fallback rejects GitLab/Gitea cleanly.
Scope: single branch only. A multi-branch stack cannot be submitted from a fork because a stacked child PR's base branch cannot live in the upstream repo.
The base branch must already exist upstream.

### Automatic CI hydration trust

The TUI and desktop app may refresh CI automatically after opening a repository.
For those credential-bearing requests, repository-local `stax.toml` may select
only `remote.name`. The following values are accepted only from global
`~/.config/stax/config.toml`: `remote.base_url`, `remote.api_base_url`,
`remote.forge`, and all `[auth]` settings.

GitHub.com, GitLab.com, and Gitea.com use built-in trusted API mappings.
Self-hosted or enterprise remotes must set a matching global
`remote.base_url`; if the API uses a different hostname, set the relationship
explicitly with global `remote.api_base_url`. For GitHub Enterprise,
`auth.gh_hostname` must match the Git remote hostname. Automatic hydration
rejects mismatches before looking up a token or making a request.

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

GitHub can return `401 Unauthorized` or `404 Not Found` when the selected token
is expired or cannot see a private repository. Stax adds actionable auth
guidance to these responses for repository list/search operations and for
review/comment reads after a PR has already been resolved. Run
`stax auth --from-gh` to refresh from GitHub CLI, or verify the token's
repository access and scopes. Direct missing-PR lookups and mutations keep their
normal errors because their 404 responses commonly describe an absent resource,
not an authentication problem.

```bash
st auth status
```
