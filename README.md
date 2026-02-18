<div align="center">
  <h1>stax</h1>
  <p>
    <strong>A modern CLI for stacked Git branches and PRs.</strong>
  </p>

  <p>
    <a href="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml"><img alt="CI" src="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml/badge.svg"></a>
    <a href="https://crates.io/crates/stax"><img alt="Crates.io" src="https://img.shields.io/crates/v/stax"></a>
    <img alt="Performance" src="https://img.shields.io/badge/~21ms-startup-blue">
    <img alt="TUI" src="https://img.shields.io/badge/TUI-ratatui-5f5fff">
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
  </p>

  <img src="assets/screenshot.png" width="900" alt="stax screenshot">
</div>

## What are Stacked Branches?

Instead of one massive PR with 50 files, stacked branches let you split work into small, reviewable pieces that build on each other (and visualize it as a tree).

**Why this is great:**
- **Smaller reviews** - Each PR is focused, so reviewers move faster and catch more issues
- **Parallel progress** - Keep building on top while lower PRs are still in review
- **Safer shipping** - Merge foundations first; reduce the risk of “one giant PR” landing at once
- **Cleaner history** - Each logical change lands independently (easier to understand, revert, and `git blame`)

<details>
<summary>Example stack</summary>

```text
◉  feature/auth-ui 1↑
○  feature/auth-api 1↑
○  main
```
</details>

Each branch is a focused PR. Reviewers see small diffs. You ship faster.

## Why stax?

stax is a modern stacked-branch workflow that keeps PRs small, rebases safe, and the whole stack easy to reason about.

- **Blazing fast** - Native Rust binary (~22ms `stax ls` on a 10-branch stack)
- **Terminal UX** - Interactive TUI with tree view, PR status, diff viewer, and reorder mode
- **Ship stacks, not mega-PRs** - Submit/update a whole stack of PRs with correct bases in one command
- **Safe history rewriting** - Transactional restacks + automatic backups + `stax undo` / `stax redo`
- **Merge the stack for you** - Cascade merge bottom → current, with rebase/PR-base updates along the way
- **Drop-in compatible** - Uses freephite metadata format—existing stacks migrate instantly

## Install

```bash
# Homebrew (macOS/Linux)
brew tap cesarferreira/tap && brew install stax

# Or with cargo binstall
cargo binstall stax
```

Both `stax` and `st` (short alias) are installed automatically. All examples below use `stax`, but `st` works identically.

## Quick Start

Set up GitHub auth first (required for PR creation, CI checks, and review metadata):

```bash
# Option A (recommended): use GitHub CLI auth
gh auth login
stax auth --from-gh

# Option B: enter a personal access token manually
stax auth

# Option C: provide a stax-specific env var
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default, stax does not use ambient `GITHUB_TOKEN` unless you opt in via `[auth].allow_github_token_env = true` in config.

```bash
# 1. Create stacked branches
stax create auth-api           # First branch off main
stax create auth-ui            # Second branch, stacked on first

# 2. View your stack
stax ls
# ◉  auth-ui 1↑                ← you are here
# ○  auth-api 1↑
# ○  main

# 3. Submit PRs for the whole stack
stax ss
# Creating PR for auth-api... ✓ #12 (targets main)
# Creating PR for auth-ui... ✓ #13 (targets auth-api)

# 4. After reviews, sync and rebase
stax rs --restack
```

## Interactive Branch Creation

Run `stax create` without arguments to launch the guided wizard:

```bash
$ stax create

╭─ Create Stacked Branch ─────────────────────────────╮
│ Parent: feature/auth (current branch)               │
╰─────────────────────────────────────────────────────╯

? Branch name: auth-validation

? What to include:
  ● Stage all changes (3 files modified)
  ○ Empty branch (no changes)

? Commit message (Enter to skip): Validate auth tokens

✓ Created cesar/auth-validation
  → Stacked on feature/auth
```

## Interactive TUI

Run `stax` with no arguments to launch the interactive terminal UI:

```bash
stax
```

<p align="center">
  <img alt="stax TUI" src="assets/tui.png" width="800">
</p>

**TUI Features:**
- Visual stack tree with PR status, sync indicators, and commit counts
- Full diff viewer for each branch
- Keyboard-driven: checkout, restack, submit PRs, create/rename/delete branches
- **Reorder mode**: Rearrange branches in your stack with `o` then `Shift+↑/↓`

| Key | Action |
|-----|--------|
| `↑/↓` | Navigate branches |
| `Enter` | Checkout branch |
| `r` | Restack selected branch |
| `s` | Submit stack |
| `o` | Enter reorder mode (reparent branches) |
| `n` | Create new branch |
| `d` | Delete branch |
| `?` | Show all keybindings |

### Reorder Mode

Rearrange branches within your stack without manually running reparent commands:

<p align="center">
  <img alt="stax reorder mode" src="assets/reordering-stacks.png" width="800">
</p>

1. Select a branch and press `o` to enter reorder mode
2. Use `Shift+↑/↓` to move the branch up or down in the stack
3. Preview shows which reparent operations will happen
4. Press `Enter` to apply changes and automatically restack

### Split Mode

Split a branch with many commits into multiple stacked branches:

```bash
stax split
```


**How it works:**
1. Run `stax split` on a branch with multiple commits
2. Navigate commits with `j/k` or arrows
3. Press `s` to mark a split point and enter a branch name
4. Preview shows the resulting branch structure in real-time
5. Press `Enter` to execute - new branches are created with proper metadata

| Key | Action |
|-----|--------|
| `j/k` or `↑/↓` | Navigate commits |
| `s` | Mark split point at cursor (enter branch name) |
| `d` | Remove split point at cursor |
| `Enter` | Execute split |
| `?` | Show help |
| `q/Esc` | Cancel and quit |

**Example:** You have a branch with commits A→B→C→D→E. Mark splits after B ("part1") and D ("part2"):

```
Before:                    After:
main                       main
  └─ my-feature (A-E)        └─ part1 (A, B)
                                 └─ part2 (C, D)
                                      └─ my-feature (E)
```

Split uses the transaction system, so you can `stax undo` if needed.

## Core Commands

| Command | What it does |
|---------|--------------|
| `stax` | Launch interactive TUI |
| `stax ls` | Show your stack with PR status and what needs rebasing |
| `stax create <name>` | Create a new branch stacked on current |
| `stax ss` | Submit stack - push all branches and create/update PRs |
| `stax merge` | Merge PRs from bottom of stack up to current branch |
| `stax rs` | Repo sync - pull trunk, clean up merged branches |
| `stax rs --restack` | Sync and rebase all branches onto updated trunk |
| `stax restack --auto-stash-pop` | Restack even when target worktrees are dirty (auto-stash/pop) |
| `stax rs --restack --auto-stash-pop` | Sync, restack, auto-stash/pop dirty worktrees |
| `stax cascade` | Restack from bottom, push, and create/update PRs |
| `stax cascade --no-pr` | Restack and push (skip PR creation/updates) |
| `stax cascade --no-submit` | Restack only (no remote interaction) |
| `stax cascade --auto-stash-pop` | Cascade even when target worktrees are dirty (auto-stash/pop) |
| `stax co` | Interactive branch checkout with fuzzy search |
| `stax u` / `stax d` | Move up/down the stack |
| `stax m` | Modify - stage all changes and amend current commit |
| `stax pr` | Open current branch's PR in browser |
| `stax copy` | Copy branch name to clipboard |
| `stax copy --pr` | Copy PR URL to clipboard |
| `stax standup` | Show your recent activity for standups |
| `stax changelog` | Generate changelog between two refs |
| `stax undo` | Undo last operation (restack, submit, etc.) |

## Standup Summary

Struggling to remember what you worked on yesterday? Run `stax standup` to get a quick summary of your recent activity:

![Standup Summary](assets/standup.png)

Shows your merged PRs, opened PRs, recent pushes, and anything that needs attention - perfect for daily standups.

```bash
stax standup              # Last 24 hours (default)
stax standup --hours 48   # Look back further
stax standup --json       # For scripting
```

## Changelog Generation

Generate a pretty changelog between two git refs - perfect for release notes or understanding what changed between versions:

```bash
stax changelog v1.0.0              # From v1.0.0 to HEAD
stax changelog v1.0.0 v2.0.0       # Between two tags
stax changelog abc123 def456       # Between commits
```

Example output:

```
Changelog: v1.0.0 → HEAD (5 commits)
──────────────────────────────────────────────────

  abc1234 #42 feat: implement user auth (@johndoe)
  def5678 #38 fix: resolve cache issue (@janesmith)
  ghi9012     chore: update deps (@bob)
```

### Monorepo Support

Working in a monorepo? Filter commits to only those touching a specific folder:

```bash
stax changelog v1.0.0 --path apps/frontend
stax changelog v1.0.0 --path packages/shared-utils
```

This shows only commits that modified files within that path - ideal for generating changelogs for individual packages or services.

### JSON Output

For scripting or CI pipelines:

```bash
stax changelog v1.0.0 --json
```

```json
{
  "from": "v1.0.0",
  "to": "HEAD",
  "path": null,
  "commit_count": 3,
  "commits": [
    {
      "hash": "abc1234567890",
      "short_hash": "abc1234",
      "message": "feat: add feature (#42)",
      "author": "johndoe",
      "pr_number": 42
    }
  ]
}
```

PR numbers are automatically extracted from commit messages (GitHub's squash merge format: `(#123)`).

## Multi-Worktree Support

stax is worktree-aware. When you have branches checked out across multiple worktrees, restack, sync, and cascade all work correctly without requiring you to switch worktrees manually.

### How it works

- **Restack / upstack restack / sync `--restack`**: When a branch to be rebased is checked out in another worktree, stax runs `git rebase` inside that worktree instead of checking it out in the current one.
- **Cascade**: Before restacking, stax fetches from remote and fast-forwards your local trunk — even if trunk is checked out in a different worktree. This prevents rebasing onto a stale local trunk, which would cause PRs to include commits already merged to remote.
- **Sync trunk update**: If trunk is checked out in another worktree, stax pulls it there directly.

### Dirty worktrees

By default, stax fails fast if a target worktree has uncommitted changes, showing you the branch name and worktree path.

Use `--auto-stash-pop` to let stax stash changes automatically before rebasing and restore them afterward:

```bash
stax restack --auto-stash-pop
stax upstack restack --auto-stash-pop
stax sync --restack --auto-stash-pop
```

If the rebase results in a conflict, the stash is kept intact so your changes are not lost. Run `git stash list` to find them.

### Cascade flags

| Command | Behavior |
|---|---|
| `stax cascade` | restack → push → create/update PRs |
| `stax cascade --no-pr` | restack → push (skip PR creation/updates) |
| `stax cascade --no-submit` | restack only (no remote interaction) |
| `stax cascade --auto-stash-pop` | any of the above, auto-stash/pop dirty worktrees |

Use `--no-pr` when your remote branches should be updated (pushed) but you aren't ready to open or update PRs yet — e.g. branches still in progress. Use `--no-submit` for a pure local restack with no network activity at all. Use `--auto-stash-pop` if any branch in the stack is checked out in a dirty worktree.

> **Tip:** run `stax rs` before `stax cascade` to pull the latest trunk and avoid rebasing onto stale commits. If your local trunk is behind remote, `stax cascade` will warn you.

## Safe History Rewriting with Undo

Stax makes rebasing and force-pushing **safe** with automatic backups and one-command recovery:

```bash
# Make a mistake while restacking? No problem.
stax restack
# ✗ conflict in feature/auth
# Your repo is recoverable via: stax undo

# Instantly restore to before the restack
stax undo
# ✓ Undone! Restored 3 branch(es).
```

### How It Works

Every potentially-destructive operation (`restack`, `submit`, `sync --restack`, TUI reorder) is **transactional**:

1. **Snapshot** - Before touching anything, stax records the current commit SHA of each affected branch
2. **Backup refs** - Creates Git refs at `refs/stax/backups/<op-id>/<branch>` pointing to original commits
3. **Execute** - Performs the operation (rebase, force-push, etc.)
4. **Receipt** - Saves an operation receipt to `.git/stax/ops/<op-id>.json`

If anything goes wrong, `stax undo` reads the receipt and restores all branches to their exact prior state.

### Undo & Redo Commands

| Command | Description |
|---------|-------------|
| `stax undo` | Undo the last operation |
| `stax undo <op-id>` | Undo a specific operation |
| `stax redo` | Redo (re-apply) the last undone operation |

**Flags:**
- `--yes` - Auto-approve prompts (useful for scripts)
- `--no-push` - Only restore local branches, don't touch remote

### Remote Recovery

If the undone operation had force-pushed branches, stax will prompt:

```bash
stax undo
# ✓ Restored 2 local branch(es)
# This operation force-pushed 2 branch(es) to remote.
# Force-push to restore remote branches too? [y/N]
```

Use `--yes` to auto-approve or `--no-push` to skip remote restoration.

## Real-World Example

You're building a payments feature. Instead of one 2000-line PR:

```bash
# Start the foundation
stax create payments-models
# ... write database models, commit ...

# Stack the API layer on top
stax create payments-api
# ... write API endpoints, commit ...

# Stack the UI on top of that
stax create payments-ui
# ... write React components, commit ...

# View your stack
stax ls
# ◉  payments-ui 1↑           ← you are here
# ○  payments-api 1↑
# ○  payments-models 1↑
# ○  main

# Submit all 3 as separate PRs (each targeting its parent)
stax ss
# Creating PR for payments-models... ✓ #101 (targets main)
# Creating PR for payments-api... ✓ #102 (targets payments-models)
# Creating PR for payments-ui... ✓ #103 (targets payments-api)
```

Reviewers can now review 3 small PRs instead of one giant one. When `payments-models` is approved and merged:

```bash
stax rs --restack
# ✓ Pulled latest main
# ✓ Cleaned up payments-models (merged)
# ✓ Rebased payments-api onto main
# ✓ Rebased payments-ui onto payments-api
# ✓ Updated PR #102 to target main
```

## Cascade Stack Merge

Merge your entire stack with one command! `stax merge` intelligently merges PRs from the bottom of your stack up to your current branch, handling rebases and PR updates automatically.

### How It Works

```
Stack:  main ← PR-A ← PR-B ← PR-C ← PR-D

Position        │ What gets merged
────────────────┼─────────────────────────────
On PR-A         │ Just PR-A (1 PR)
On PR-B         │ PR-A, then PR-B (2 PRs)
On PR-C         │ PR-A → PR-B → PR-C (3 PRs)
On PR-D (top)   │ Entire stack (4 PRs)
```

The merge scope depends on your current branch:
- **Bottom of stack**: Merges just that one PR
- **Middle of stack**: Merges all PRs from bottom up to current
- **Top of stack**: Merges the entire stack

### Example Usage

```bash
# View your stack
stax ls
# ◉  payments-ui 1↑           ← you are here
# ○  payments-api 1↑
# ○  payments-models 1↑
# ○  main

# Merge all 3 PRs into main
stax merge
```

You'll see an interactive preview before merging:

```
╭──────────────────────────────────────────────────────╮
│                    Stack Merge                       │
╰──────────────────────────────────────────────────────╯

You are on: payments-ui (PR #103)

This will merge 3 PRs from bottom → current:

  ┌─────────────────────────────────────────────────┐
  │  1. payments-models (#101)       ✓ Ready        │
  │     ├─ CI: ✓ passed                             │
  │     ├─ Reviews: ✓ 2/2 approved                  │
  │     └─ Merges into: main                        │
  ├─────────────────────────────────────────────────┤
  │  2. payments-api (#102)          ✓ Ready        │
  │     ├─ CI: ✓ passed                             │
  │     ├─ Reviews: ✓ 1/1 approved                  │
  │     └─ Merges into: main (after rebase)         │
  ├─────────────────────────────────────────────────┤
  │  3. payments-ui (#103)           ✓ Ready        │  ← you are here
  │     ├─ CI: ✓ passed                             │
  │     ├─ Reviews: ✓ 1/1 approved                  │
  │     └─ Merges into: main (after rebase)         │
  └─────────────────────────────────────────────────┘

Merge method: squash (change with --method)

? Proceed with merge? [y/N]
```

### What Happens During Merge

For each PR in the stack (bottom to top):

1. **Wait for CI** - Polls until CI passes (or use `--no-wait` to skip)
2. **Merge** - Merges the PR using your chosen method (squash/merge/rebase)
3. **Rebase next** - Rebases the next PR onto updated main
4. **Update PR base** - Changes the next PR's target from the merged branch to main
5. **Push** - Force-pushes the rebased branch
6. **Repeat** - Continues until all PRs are merged

If anything fails (CI, conflicts, permissions), the merge stops safely. Already-merged PRs remain merged, and you can fix the issue and run `stax merge` again to continue.

### Merge Options

```bash
# Merge with preview only (no actual merge)
stax merge --dry-run

# Merge entire stack regardless of current position
stax merge --all

# Choose merge strategy
stax merge --method squash    # (default) Squash and merge
stax merge --method merge     # Create merge commit
stax merge --method rebase    # Rebase and merge

# Skip CI polling (fail if not ready)
stax merge --no-wait

# Keep branches after merge (don't delete)
stax merge --no-delete

# Set custom CI timeout (default: 30 minutes)
stax merge --timeout 60

# Skip confirmation prompt
stax merge --yes
```

### Partial Stack Merge

You can merge just part of your stack by checking out a middle branch:

```bash
# Stack: main ← auth ← auth-api ← auth-ui ← auth-tests
stax checkout auth-api

# This merges only: auth, auth-api (not auth-ui or auth-tests)
stax merge

# Remaining branches (auth-ui, auth-tests) are rebased onto main
# Run stax merge again later to merge those too
```

## Import Your Open PRs

Already have open PRs on GitHub that aren't tracked by stax? Import them all at once:

```bash
stax branch track --all-prs
```

This command:
- Fetches all your open PRs from GitHub
- Downloads any missing branches from remote
- Sets up tracking with the correct parent (based on each PR's target branch)
- Stores PR metadata for each branch

Perfect for onboarding an existing repository or after cloning a fresh copy.

## Working with Multiple Stacks

You can have multiple independent stacks at once:

```bash
# You're working on auth...
stax create auth
stax create auth-login
stax create auth-validation

# Teammate needs urgent bugfix reviewed - start a new stack
stax co main                   # or: stax t
stax create hotfix-payment

# View everything
stax ls
# ○  auth-validation 1↑
# ○  auth-login 1↑
# ○  auth 1↑
# │ ◉  hotfix-payment 1↑      ← you are here
# ○─┘  main
```

## Navigation

| Command | What it does |
|---------|--------------|
| `stax u` | Move up to child branch |
| `stax d` | Move down to parent branch |
| `stax u 3` | Move up 3 branches |
| `stax top` | Jump to tip of current stack |
| `stax bottom` | Jump to base of stack (first branch above trunk) |
| `stax t` | Jump to trunk (main/master) |
| `stax prev` | Toggle to previous branch (like `git checkout -`) |
| `stax co` | Interactive picker with fuzzy search |

## Reading the Stack View

```
○        feature/validation 1↑
◉        feature/auth 1↓ 2↑ ⟳
│ ○    ☁ feature/payments PR #42
○─┘    ☁ main
```

| Symbol | Meaning |
|--------|---------|
| `◉` | Current branch |
| `○` | Other branch |
| `☁` | Has remote tracking |
| `1↑` | 1 commit ahead of parent |
| `1↓` | 1 commit behind parent |
| `⟳` | Needs restacking (parent changed) |
| `PR #42` | Has open PR |

## Configuration

```bash
stax config  # Show config path and current settings
```

Config at `~/.config/stax/config.toml`:

```toml
# ~/.config/stax/config.toml — full reference with defaults

[branch]
# DEPRECATED: Use `format` instead. Auto-prefix for branches.
# prefix = "cesar/"

# Branch name format template. Placeholders: {user}, {date}, {message}
# format = "{user}/{date}/{message}"

# Username for branch naming (default: git config user.name)
# user = "cesar"

# Date format for {date} placeholder (default: "%m-%d")
# Uses chrono strftime: %Y=year, %m=month, %d=day
# date_format = "%m-%d"

# Character to replace spaces and special chars (default: "-")
# replacement = "-"

[remote]
# Git remote name (default: "origin")
# name = "origin"

# Base web URL for GitHub (default: "https://github.com")
# base_url = "https://github.com"

# API base URL for GitHub Enterprise
# api_base_url = "https://github.company.com/api/v3"

[auth]
# Use `gh auth token` as a fallback auth source (default: true)
# use_gh_cli = true

# Allow ambient GITHUB_TOKEN env var (default: false)
# allow_github_token_env = false

# Optional hostname for gh auth token (GitHub Enterprise)
# gh_hostname = "github.company.com"

[ui]
# Show contextual tips/suggestions (default: true)
# tips = true

[ai]
# AI agent for PR body generation: "claude" or "codex"
# If not set, stax auto-detects installed agents and prompts on first use
# agent = "claude"

# Model to use with the AI agent (default: agent's own default)
# model = "claude-sonnet-4-5-20250929"
```

### Branch Name Format

Use `format` to template branch names with `{user}`, `{date}`, and `{message}` placeholders:

```toml
[branch]
format = "{user}/{date}/{message}"   # "cesar/02-11/add-login"
user = "cesar"                        # Optional: defaults to git config user.name
date_format = "%m-%d"                 # Optional: chrono strftime (default: "%m-%d")
```

Empty placeholders are cleaned up automatically. The legacy `prefix` field still works if `format` is not set.

### GitHub Authentication

stax looks for a GitHub token in the following order (first found wins):

1. `STAX_GITHUB_TOKEN` environment variable
2. Credentials file (`~/.config/stax/.credentials`)
3. `gh auth token` (when `auth.use_gh_cli = true`, default)
4. `GITHUB_TOKEN` environment variable (only when `auth.allow_github_token_env = true`)

```bash
# Option 1: stax-specific env var (highest priority)
export STAX_GITHUB_TOKEN="ghp_xxxx"

# Option 2: Interactive setup (saves to credentials file)
stax auth

# Option 3: Import from GitHub CLI auth (saves to credentials file)
stax auth --from-gh
```

To use `GITHUB_TOKEN` as a fallback, opt in explicitly:

```toml
[auth]
allow_github_token_env = true
```

```bash
export GITHUB_TOKEN="ghp_xxxx"
```

The credentials file is created with `600` permissions (read/write for owner only).

Check which source stax is actively using:

```bash
stax auth status
```

## Claude Code Integration

Teach Claude Code how to use stax by installing the skills file:

```bash
# Create skills directory if it doesn't exist
mkdir -p ~/.claude/skills

# Download the stax skills file
curl -o ~/.claude/skills/stax.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

This enables Claude Code to help you with stax workflows, create stacked branches, submit PRs, and more.

## Freephite/Graphite Compatibility

stax uses the same metadata format as freephite and supports similar commands:

| freephite | stax | graphite | stax |
|-----------|------|----------|------|
| `fp ss` | `stax ss` | `gt submit` | `stax submit` |
| `fp bs` | `stax branch submit` | `gt branch submit` | `stax branch submit` |
| `fp us submit` | `stax upstack submit` | `gt upstack submit` | `stax upstack submit` |
| `fp ds submit` | `stax downstack submit` | `gt downstack submit` | `stax downstack submit` |
| `fp rs` | `stax rs` | `gt sync` | `stax sync` |
| `fp bc` | `stax bc` | `gt create` | `stax create` |
| `fp bco` | `stax bco` | `gt checkout` | `stax co` |
| `fp bu` | `stax bu` | `gt up` | `stax u` |
| `fp bd` | `stax bd` | `gt down` | `stax d` |
| `fp ls` | `stax ls` | `gt log` | `stax log` |

**Migration is instant** - just install stax and your existing stacks work.

## PR Templates

stax automatically discovers PR templates in your repository:

### Single Template
If you have one template at `.github/PULL_REQUEST_TEMPLATE.md`, stax uses it automatically:

```bash
stax submit  # Auto-uses template, shows "Edit body?" prompt
```

### Multiple Templates
Place templates in `.github/PULL_REQUEST_TEMPLATE/` directory:

```
.github/
  └── PULL_REQUEST_TEMPLATE/
      ├── feature.md
      ├── bugfix.md
      └── docs.md
```

stax shows an interactive fuzzy-search picker:

```bash
stax submit
# ? Select PR template
#   > No template
#     bugfix
#     feature
#     docs
```

### Template Control Flags

- `--template <name>`: Skip picker, use specific template
- `--no-template`: Don't use any template
- `--edit`: Always open $EDITOR for body (regardless of template)

```bash
stax submit --template bugfix  # Use bugfix.md directly
stax submit --no-template      # Empty body
stax submit --edit             # Force editor open
```

## AI-Powered PR Body Generation

Generate a PR description using AI, based on your diff, commit messages, and the repo's PR template:

```bash
stax generate --pr-body
```

stax collects the diff, commit messages, and PR template for the current branch, sends them to an AI agent (Claude or Codex CLI), and updates the PR body on GitHub.

### First Run

If no AI agent is configured, stax auto-detects what's installed and walks you through setup:

```
? Select AI agent:
> claude (default)
  codex

? Select model for claude:
> claude-sonnet-4-5-20250929 — Sonnet 4.5 (default, balanced)
  claude-haiku-4-5-20251001 — Haiku 4.5 (fastest, cheapest)
  claude-opus-4-6 — Opus 4.6 (most capable)

? Save choices to config? (Y/n): Y
✓ Saved ai.agent = "claude", ai.model = "claude-sonnet-4-5-20250929"
```

### Options

- `--agent <name>`: Override the configured agent for this invocation
- `--model <name>`: Override the model (e.g., `claude-haiku-4-5-20251001`, `gpt-4.1-mini`)
- `--edit`: Open $EDITOR to review/tweak the generated body before updating the PR

```bash
stax generate --pr-body --agent codex                        # Use codex this time
stax generate --pr-body --model claude-haiku-4-5-20251001    # Use a specific model
stax generate --pr-body --edit                               # Review in editor first
```

## All Commands

<details>
<summary>Click to expand full command reference</summary>

### Stack Operations
| Command | Alias | Description |
|---------|-------|-------------|
| `stax status` | `s`, `ls` | Show stack (simple view) |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax submit` | `ss` | Submit full current stack (ancestors + current + descendants) |
| `stax merge` | | Merge PRs from bottom of stack to current |
| `stax sync` | `rs` | Pull trunk, delete merged branches |
| `stax restack` | | Rebase current branch onto parent |
| `stax diff` | | Show diffs for each branch vs parent |
| `stax range-diff` | | Show range-diff for branches needing restack |

### Branch Management
| Command | Alias | Description |
|---------|-------|-------------|
| `stax create <name>` | `c`, `bc` | Create stacked branch |
| `stax checkout` | `co`, `bco` | Interactive branch picker |
| `stax modify` | `m` | Stage all + amend current commit |
| `stax rename` | `b r` | Rename branch and optionally edit commit message |
| `stax branch track` | | Track an existing branch |
| `stax branch track --all-prs` | | Track all your open PRs |
| `stax branch untrack` | `ut` | Remove stax metadata for a branch (keep git branch) |
| `stax branch reparent` | | Change parent of a branch |
| `stax branch submit` | `bs` | Submit only current branch |
| `stax branch delete` | | Delete a branch |
| `stax branch fold` | | Fold branch into parent |
| `stax branch squash` | | Squash commits on branch |
| `stax upstack restack` | | Restack current branch + descendants |
| `stax upstack submit` | | Submit current branch + descendants |
| `stax downstack submit` | | Submit ancestors + current branch |

### Navigation
| Command | Alias | Description |
|---------|-------|-------------|
| `stax up [n]` | `u`, `bu` | Move up n branches |
| `stax down [n]` | `d`, `bd` | Move down n branches |
| `stax top` | | Move to stack tip |
| `stax bottom` | | Move to stack base |
| `stax trunk` | `t` | Switch to trunk |
| `stax prev` | `p` | Toggle to previous branch |

### Interactive
| Command | Description |
|---------|-------------|
| `stax` | Launch interactive TUI |
| `stax split` | Interactive TUI to split branch into multiple stacked branches |

### Recovery
| Command | Description |
|---------|-------------|
| `stax undo` | Undo last operation (restack, submit, etc.) |
| `stax undo <op-id>` | Undo a specific operation by ID |
| `stax redo` | Re-apply the last undone operation |

### Utilities
| Command | Description |
|---------|-------------|
| `stax auth` | Set GitHub token (`--from-gh` supported) |
| `stax auth status` | Show active GitHub auth source and resolution order |
| `stax config` | Show configuration |
| `stax doctor` | Check repo health |
| `stax continue` | Continue after resolving conflicts |
| `stax pr` | Open PR in browser |
| `stax ci` | Show CI status for branches in current stack |
| `stax ci --all` | Show CI status for all tracked branches |
| `stax ci --watch` | Watch CI until completion (polls every 15s, records history) |
| `stax ci --watch --interval 30` | Watch with custom polling interval in seconds |
| `stax ci --json` | Output CI status as JSON |
| `stax copy` | Copy branch name to clipboard |
| `stax copy --pr` | Copy PR URL to clipboard |
| `stax comments` | Show PR comments with rendered markdown |
| `stax comments --plain` | Show PR comments as raw markdown |
| `stax standup` | Show your recent activity for standups |
| `stax standup --hours 48` | Look back 48 hours instead of default 24 |
| `stax standup --json` | Output activity as JSON for scripting |
| `stax changelog <from> [to]` | Generate changelog between two refs |
| `stax changelog v1.0 --path src/` | Changelog filtered by path (monorepo) |
| `stax changelog v1.0 --json` | Output changelog as JSON |
| `stax generate --pr-body` | Generate PR body with AI and update the PR |
| `stax generate --pr-body --edit` | Generate and review in editor before updating |

### Common Flags
- `stax create -m "msg"` - Create branch with commit message
- `stax create -a` - Stage all changes
- `stax create -am "msg"` - Stage all and commit
- `stax rename new-name` - Rename current branch
- `stax rename -e` - Rename and edit commit message
- `stax submit --draft` - Create PRs as drafts
- `stax branch submit` / `stax bs` - Submit current branch only
- `stax upstack submit` - Submit current branch and descendants
- `stax downstack submit` - Submit ancestors and current branch
- `stax submit --yes` - Auto-approve prompts
- `stax submit --no-prompt` - Use defaults, skip interactive prompts
- `stax submit --template <name>` - Use specific template by name (skip picker)
- `stax submit --no-template` - Skip template selection (no template)
- `stax submit --edit` - Always open editor for PR body
- `stax submit --reviewers alice,bob` - Add reviewers
- `stax submit --labels bug,urgent` - Add labels
- `stax submit --assignees alice` - Assign users
- `stax merge --all` - Merge entire stack
- `stax merge --method squash` - Choose merge method (squash/merge/rebase)
- `stax merge --dry-run` - Preview merge without executing
- `stax merge --no-wait` - Don't wait for CI, fail if not ready
- `stax restack --auto-stash-pop` - Auto-stash/pop dirty target worktrees during restack
- `stax upstack restack --auto-stash-pop` - Auto-stash/pop when restacking descendants
- `stax rs --restack --auto-stash-pop` - Sync, restack, auto-stash/pop dirty worktrees (`rs` = sync alias)
- `stax cascade --no-pr` - Restack and push branches; skip PR creation/updates
- `stax cascade --no-submit` - Restack only, no remote interaction
- `stax cascade --auto-stash-pop` - Auto-stash/pop dirty target worktrees during cascade restack
- `stax sync --restack` - Sync and rebase all branches
- `stax status --json` - Output as JSON
- `stax undo --yes` - Undo without prompts
- `stax undo --no-push` - Undo locally only, skip remote
- `stax generate --pr-body --edit` - Generate and review in editor
- `stax generate --pr-body --agent codex` - Use specific AI agent
- `stax generate --pr-body --model claude-haiku-4-5-20251001` - Use specific model

**CI/Automation example:**
```bash
stax submit --draft --yes --no-prompt
stax merge --yes --method squash
```

</details>

## Benchmarks

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---------|------|-----------|----------|
| `ls` (10-branch stack) | 22.8ms | 369.5ms | 209.1ms |

Raw [`hyperfine`](https://github.com/sharkdp/hyperfine) results:

```
➜ hyperfine 'stax ls' 'fp ls' 'gt ls' --warmup 3
Benchmark 1: stax ls
  Time (mean ± σ):      22.8 ms ±   1.0 ms    [User: 9.0 ms, System: 11.3 ms]
  Range (min … max):    21.1 ms …  26.9 ms    112 runs

Benchmark 2: fp ls
  Time (mean ± σ):     369.5 ms ±   7.0 ms    [User: 268.8 ms, System: 184.2 ms]
  Range (min … max):   360.7 ms … 380.4 ms    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):     209.1 ms ±   2.8 ms    [User: 152.5 ms, System: 52.6 ms]
  Range (min … max):   205.9 ms … 215.7 ms    13 runs

Summary
  stax ls ran
   9.18 ± 0.43 times faster than gt ls
   16.23 ± 0.79 times faster than fp ls
```

![ls benchmark](https://quickchart.io/chart?c=%7B%22type%22%3A%22bar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22freephite%22%2C%22graphite%22%2C%22stax%22%5D%2C%22datasets%22%3A%5B%7B%22label%22%3A%22Time%20(ms)%22%2C%22data%22%3A%5B369.5%2C209.1%2C22.8%5D%2C%22backgroundColor%22%3A%5B%22%23ff0000%22%2C%22%23008000%22%2C%22%230000ff%22%5D%7D%5D%7D%2C%22options%22%3A%7B%22plugins%22%3A%7B%22datalabels%22%3A%7B%22display%22%3Atrue%2C%22color%22%3A%22white%22%2C%22align%22%3A%22center%22%2C%22anchor%22%3A%22center%22%7D%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22text%22%3A%22ls%20benchmark%20(lower%20is%20better)%22%7D%2C%22scales%22%3A%7B%22y%22%3A%7B%22beginAtZero%22%3Atrue%2C%22max%22%3A400%7D%7D%7D%7D)

## License

MIT
