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

## Feature Highlights

- [`st merge`](#cascade-stack-merge) - Cascade-merge your stack from bottom -> current with CI/rebase-aware safety checks.
- [`st merge --when-ready`](#cascade-stack-merge) - Merge in explicit wait-for-ready mode with configurable polling.
- [`st generate --pr-body`](#ai-powered-pr-body-generation) - Generate polished PR descriptions with AI from your branch diff and context.
- [`AI skill integrations`](#claude-code-integration) - Embed `skills.md` into Claude Code, Codex, Gemini CLI, or OpenCode so your AI can create and stack PRs.
- [`st standup`](#standup-summary) - Get a quick summary of recent PRs, pushes, and activity for daily standups.
- [`st ss`](#core-commands) - Submit or update the full PR stack with correct parent/child base relationships.
- [`st rs --restack`](#core-commands) - Sync trunk and restack descendants so your branch tree stays clean and current.
- [`Interactive TUI`](#interactive-tui) - Browse your stack tree, PR status, diffs, and reorder branches visually.
- [`st undo` / `st redo`](#safe-history-rewriting-with-undo) - Recover safely from restacks and rebases with transactional history snapshots.
- [`st demo`](#core-commands) - Interactive tutorial that walks you through stacked branches in a temp repo (no auth needed).
- [`st test`](#core-commands) - Run a command on each branch in the stack to validate before submitting.

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

- **Blazing fast** - Native Rust binary (~22ms `st ls` on a 10-branch stack)
- **Terminal UX** - Interactive TUI with tree view, PR status, diff viewer, and reorder mode
- **Ship stacks, not mega-PRs** - Submit/update a whole stack of PRs with correct bases in one command
- **Safe history rewriting** - Transactional restacks + automatic backups + `st undo` / `st redo`
- **Merge the stack for you** - Cascade merge bottom → current, with rebase/PR-base updates along the way
- **Parallel AI agents** - Isolated worktrees for Codex, Claude Code, Cursor, and others — each on its own branch, restacked together with one command
- **Drop-in compatible** - Uses freephite metadata format—existing stacks migrate instantly

## Install

```bash
# Homebrew (macOS/Linux)
brew install cesarferreira/tap/stax

# Or with cargo binstall
cargo binstall stax
```

Both `st` and `stax` are installed automatically. All examples below use `st`.

## Full Documentation

- Live docs: https://cesarferreira.github.io/stax/
- Source docs index: [docs/index.md](docs/index.md)

Run docs locally with `uv`:

```bash
uv run --with-requirements docs/requirements.txt zensical serve
```

## Quick Start

Set up GitHub auth first (required for PR creation, CI checks, and review metadata):

```bash
# Option A (recommended): use GitHub CLI auth
gh auth login
st auth --from-gh

# Option B: enter a personal access token manually
st auth

# Option C: provide a stax-specific env var
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default, stax does not use ambient `GITHUB_TOKEN` unless you opt in via `[auth].allow_github_token_env = true` in config.

```bash
# 1. Create stacked branches
st create auth-api           # First branch off main
st create auth-ui            # Second branch, stacked on first

# 2. View your stack
st ls
# ◉  auth-ui 1↑                ← you are here
# ○  auth-api 1↑
# ○  main

# 3. Submit PRs for the whole stack
st ss
# Creating PR for auth-api... ✓ #12 (targets main)
# Creating PR for auth-ui... ✓ #13 (targets auth-api)

# 4. After reviews, sync and rebase
st rs --restack
```

## Core Commands

| Command | What it does |
|---------|--------------|
| `st` | Launch interactive TUI |
| `st ls` | Show your stack with PR status and what needs rebasing |
| `st ll` | Show stack with PR URLs and full details |
| `st create <name>` | Create a new branch stacked on current |
| `st ss` | Submit stack - push all branches and create/update PRs |
| `st merge` | Merge PRs from bottom of stack up to current branch |
| `st merge --when-ready` | Merge with explicit wait-for-ready mode and configurable polling interval |
| `st rs` | Repo sync - pull trunk, clean up merged branches |
| `st rs --restack` | Sync and rebase all branches onto updated trunk |
| `st restack` | Restack current stack (ancestors + current + descendants) |
| `st restack --auto-stash-pop` | Restack even when target worktrees are dirty (auto-stash/pop) |
| `st rs --restack --auto-stash-pop` | Sync, restack, auto-stash/pop dirty worktrees |
| `st cascade` | Restack from bottom, push, and create/update PRs |
| `st cascade --no-pr` | Restack and push (skip PR creation/updates) |
| `st cascade --no-submit` | Restack only (no remote interaction) |
| `st cascade --auto-stash-pop` | Cascade even when target worktrees are dirty (auto-stash/pop) |
| `st co` | Interactive branch checkout with fuzzy search |
| `st u` / `st d` | Move up/down the stack |
| `st m` | Modify - stage all changes and amend current commit |
| `st pr` | Open current branch's PR in browser |
| `st open` | Open repository in browser |
| `st copy` | Copy branch name to clipboard |
| `st copy --pr` | Copy PR URL to clipboard |
| `st standup` | Show your recent activity for standups |
| `st changelog` | Generate changelog between two refs |
| `st undo` | Undo last operation (restack, submit, etc.) |
| `st abort` | Abort in-progress rebase/conflict resolution |
| `st detach` | Remove a branch from its stack (reparent children) |
| `st reorder` | Interactively reorder branches in a stack |
| `st validate` | Validate stack metadata health |
| `st fix` | Auto-repair broken metadata |
| `st test <cmd>` | Run a command on each branch in the stack |
| `st demo` | Interactive tutorial (no auth/repo needed) |

## Interactive Branch Creation

Run `st create` without arguments to launch the guided wizard:

```bash
$ st create

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

Use a one-liner when the branch name and commit message come from the same text:

```bash
st create -am "migrate checkout webhooks to v2"
# Creates a branch name from the message (using your branch format),
# stages all changes, and commits with the same message.
```

## AI-Powered PR Body Generation

Generate a PR description using AI, based on your diff, commit messages, and the repo's PR template:

```bash
st generate --pr-body
```

stax collects the diff, commit messages, and PR template for the current branch, sends them to an AI agent (Claude, Codex, Gemini CLI, or OpenCode), and updates the PR body on GitHub.

Prerequisites:
- Current branch must be tracked by stax
- Current branch must already have a PR (create one with `st submit` / `st ss`)

You can also generate during submit:

```bash
st submit --ai-body
```

### First Run

If no AI agent is configured, stax auto-detects what's installed and walks you through setup:

```
? Select AI agent:
> claude (default)
  codex
  gemini
  opencode

? Select model for claude:
> claude-sonnet-4-5-20250929 — Sonnet 4.5 (default, balanced)
  claude-haiku-4-5-20251001 — Haiku 4.5 (fastest, cheapest)
  claude-opus-4-6 — Opus 4.6 (most capable)

? Save choices to config? (Y/n): Y
✓ Saved ai.agent = "claude", ai.model = "claude-sonnet-4-5-20250929"
```

### Options

- `--agent <name>`: Override the configured agent for this invocation (`claude`, `codex`, `gemini`, `opencode`)
- `--model <name>`: Override the model (e.g., `claude-haiku-4-5-20251001`, `gpt-4.1-mini`, `gemini-2.5-flash`)
- `--edit`: Open $EDITOR to review/tweak the generated body before updating the PR

```bash
st generate --pr-body --agent codex                        # Use codex this time
st generate --pr-body --model claude-haiku-4-5-20251001    # Use a specific model
st generate --pr-body --agent gemini --model gemini-2.5-flash
st generate --pr-body --agent opencode
st generate --pr-body --edit                               # Review in editor first
```

## Interactive TUI

Run `st` with no arguments to launch the interactive terminal UI:

```bash
st
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
| `j/k` or `↑/↓` | Navigate branches |
| `Enter` | Checkout branch |
| `r` | Restack selected branch |
| `R` (Shift+r) | Restack all branches in stack |
| `s` | Submit stack |
| `p` | Open selected branch PR |
| `o` | Enter reorder mode (reparent branches) |
| `n` | Create new branch |
| `e` | Rename current branch |
| `d` | Delete branch |
| `/` | Search/filter branches |
| `Tab` | Toggle focus between stack and diff panes |
| `?` | Show all keybindings |
| `q/Esc` | Quit |

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
st split
```


**How it works:**
1. Run `st split` on a branch with multiple commits
2. Navigate commits with `j/k` or arrows
3. Press `s` to mark a split point and enter a branch name
4. Preview shows the resulting branch structure in real-time
5. Press `Enter` to execute - new branches are created with proper metadata

| Key | Action |
|-----|--------|
| `j/k` or `↑/↓` | Navigate commits |
| `s` | Mark split point at cursor (enter branch name) |
| `d` | Remove split point at cursor |
| `S-J/K` | Move split point down/up |
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

Split uses the transaction system, so you can `st undo` if needed.

## Standup Summary

Struggling to remember what you worked on yesterday? Run `st standup` to get a quick summary of your recent activity:

![Standup Summary](assets/standup.png)

Shows your merged PRs, opened PRs, recent pushes, and anything that needs attention - perfect for daily standups.

```bash
st standup              # Last 24 hours (default)
st standup --hours 48   # Look back further
st standup --json       # For scripting
```

## Changelog Generation

Generate a pretty changelog between two git refs - perfect for release notes or understanding what changed between versions:

```bash
st changelog v1.0.0              # From v1.0.0 to HEAD
st changelog v1.0.0 v2.0.0       # Between two tags
st changelog abc123 def456       # Between commits
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
st changelog v1.0.0 --path apps/frontend
st changelog v1.0.0 --path packages/shared-utils
```

This shows only commits that modified files within that path - ideal for generating changelogs for individual packages or services.

### JSON Output

For scripting or CI pipelines:

```bash
st changelog v1.0.0 --json
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
- **Merged middle branches (including squash merges)**: When sync reparents children off a merged branch, stax preserves the old-base boundary and uses `git rebase --onto` so child branches replay only novel commits instead of replaying already-integrated parent history.
- **Cascade**: Before restacking, stax fetches from remote and fast-forwards your local trunk — even if trunk is checked out in a different worktree. This prevents rebasing onto a stale local trunk, which would cause PRs to include commits already merged to remote.
- **Sync trunk update**: If trunk is checked out in another worktree, stax pulls it there directly.

### Dirty worktrees

By default, stax fails fast if a target worktree has uncommitted changes, showing you the branch name and worktree path.

Use `--auto-stash-pop` to let stax stash changes automatically before rebasing and restore them afterward:

```bash
st restack --auto-stash-pop
st upstack restack --auto-stash-pop
st sync --restack --auto-stash-pop
```

If the rebase results in a conflict, the stash is kept intact so your changes are not lost. Run `git stash list` to find them.

### Cascade flags

| Command | Behavior |
|---|---|
| `st cascade` | restack → push → create/update PRs |
| `st cascade --no-pr` | restack → push (skip PR creation/updates) |
| `st cascade --no-submit` | restack only (no remote interaction) |
| `st cascade --auto-stash-pop` | any of the above, auto-stash/pop dirty worktrees |

Use `--no-pr` when your remote branches should be updated (pushed) but you aren't ready to open or update PRs yet — e.g. branches still in progress. Use `--no-submit` for a pure local restack with no network activity at all. Use `--auto-stash-pop` if any branch in the stack is checked out in a dirty worktree.

> **Tip:** run `st rs` before `st cascade` to pull the latest trunk and avoid rebasing onto stale commits. If your local trunk is behind remote, `st cascade` will warn you.

## Safe History Rewriting with Undo

Stax makes rebasing and force-pushing **safe** with automatic backups and one-command recovery:

```bash
# Make a mistake while restacking? No problem.
st restack
# ✗ conflict in feature/auth
# Your repo is recoverable via: st undo

# Instantly restore to before the restack
st undo
# ✓ Undone! Restored 3 branch(es).
```

### How It Works

Every potentially-destructive operation (`restack`, `submit`, `sync --restack`, TUI reorder) is **transactional**:

1. **Snapshot** - Before touching anything, stax records the current commit SHA of each affected branch
2. **Backup refs** - Creates Git refs at `refs/stax/backups/<op-id>/<branch>` pointing to original commits
3. **Execute** - Performs the operation (rebase, force-push, etc.)
4. **Receipt** - Saves an operation receipt to `.git/stax/ops/<op-id>.json`

If anything goes wrong, `st undo` reads the receipt and restores all branches to their exact prior state.

### Undo & Redo Commands

| Command | Description |
|---------|-------------|
| `st undo` | Undo the last operation |
| `st undo <op-id>` | Undo a specific operation |
| `st redo` | Redo (re-apply) the last undone operation |

**Flags:**
- `--yes` - Auto-approve prompts (useful for scripts)
- `--no-push` - Only restore local branches, don't touch remote

### Remote Recovery

If the undone operation had force-pushed branches, stax will prompt:

```bash
st undo
# ✓ Restored 2 local branch(es)
# This operation force-pushed 2 branch(es) to remote.
# Force-push to restore remote branches too? [y/N]
```

Use `--yes` to auto-approve or `--no-push` to skip remote restoration.

## Real-World Example

You're building a payments feature. Instead of one 2000-line PR:

```bash
# Start the foundation
st create payments-models
# ... write database models, commit ...

# Stack the API layer on top
st create payments-api
# ... write API endpoints, commit ...

# Stack the UI on top of that
st create payments-ui
# ... write React components, commit ...

# View your stack
st ls
# ◉  payments-ui 1↑           ← you are here
# ○  payments-api 1↑
# ○  payments-models 1↑
# ○  main

# Submit all 3 as separate PRs (each targeting its parent)
st ss
# Creating PR for payments-models... ✓ #101 (targets main)
# Creating PR for payments-api... ✓ #102 (targets payments-models)
# Creating PR for payments-ui... ✓ #103 (targets payments-api)
```

Reviewers can now review 3 small PRs instead of one giant one. When `payments-models` is approved and merged:

```bash
st rs --restack
# ✓ Pulled latest main
# ✓ Cleaned up payments-models (merged)
# ✓ Rebased payments-api onto main
# ✓ Rebased payments-ui onto payments-api
# ✓ Updated PR #102 to target main
```

## Cascade Stack Merge

Merge your entire stack with one command! `st merge` intelligently merges PRs from the bottom of your stack up to your current branch, handling rebases and PR updates automatically.

Need strict "merge when ready" behavior with configurable polling? Use `st merge --when-ready`.
The legacy command `st merge-when-ready` (alias: `st mwr`) remains available as a compatibility alias.

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
st ls
# ◉  payments-ui 1↑           ← you are here
# ○  payments-api 1↑
# ○  payments-models 1↑
# ○  main

# Merge all 3 PRs into main
st merge
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

1. **Wait for readiness** - Polls until CI passes and approvals/mergeability are ready (or use `--no-wait` to fail fast)
2. **Merge** - Merges the PR using your chosen method (squash/merge/rebase)
3. **Rebase next** - Rebases the next PR onto updated main
4. **Update PR base** - Changes the next PR's target from the merged branch to main
5. **Push** - Force-pushes the rebased branch
6. **Repeat** - Continues until all PRs are merged
7. **Sync local repo** - Runs `st rs --force` to fast-forward trunk and finalize local cleanup (use `--no-sync` to skip)

If anything fails (CI, conflicts, permissions), the merge stops safely. Already-merged PRs remain merged, and you can fix the issue and run `st merge` again to continue (or `st merge --when-ready` if you were using that mode).

### Merge Options

```bash
# Merge with preview only (no actual merge)
st merge --dry-run

# Merge entire stack regardless of current position
st merge --all

# Choose merge strategy
st merge --method squash    # (default) Squash and merge
st merge --method merge     # Create merge commit
st merge --method rebase    # Rebase and merge

# Use explicit wait-for-ready mode (replacement for merge-when-ready)
st merge --when-ready

# Set custom polling interval for --when-ready mode (default: 15s)
st merge --when-ready --interval 10

# Skip CI polling (fail if not ready)
st merge --no-wait

# Keep branches after merge (don't delete)
st merge --no-delete

# Skip post-merge sync
st merge --no-sync

# Set custom CI timeout (default: 30 minutes)
st merge --timeout 60

# Skip confirmation prompt
st merge --yes
```

`--when-ready` cannot be combined with `--dry-run` or `--no-wait`.

### Partial Stack Merge

You can merge just part of your stack by checking out a middle branch:

```bash
# Stack: main ← auth ← auth-api ← auth-ui ← auth-tests
st checkout auth-api

# This merges only: auth, auth-api (not auth-ui or auth-tests)
st merge

# Remaining branches (auth-ui, auth-tests) are rebased onto main
# Run st merge again later to merge those too
```

## Import Your Open PRs

Already have open PRs on GitHub that aren't tracked by stax? Import them all at once:

```bash
st branch track --all-prs
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
st create auth
st create auth-login
st create auth-validation

# Teammate needs urgent bugfix reviewed - start a new stack
st co main                   # or: st t
st create hotfix-payment

# View everything
st ls
# ○  auth-validation 1↑
# ○  auth-login 1↑
# ○  auth 1↑
# │ ◉  hotfix-payment 1↑      ← you are here
# ○─┘  main
```

## Navigation

| Command | What it does |
|---------|--------------|
| `st u` | Move up to child branch |
| `st d` | Move down to parent branch |
| `st u 3` | Move up 3 branches |
| `st top` | Jump to tip of current stack |
| `st bottom` | Jump to base of stack (first branch above trunk) |
| `st t` | Jump to trunk (main/master) |
| `st prev` | Toggle to previous branch (like `git checkout -`) |
| `st co` | Interactive picker with fuzzy search |

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
st config  # Show config path and current settings
```

Config at `~/.config/stax/config.toml`:

```toml
# ~/.config/stax/config.toml
# Created automatically on first run with these defaults:

[branch]
date_format = "%m-%d"
replacement = "-"

[remote]
name = "origin"
base_url = "https://github.com"

[ui]
tips = true

[auth]
use_gh_cli = true
allow_github_token_env = false

[ai]
# agent = "claude" # or: "codex" / "gemini" / "opencode"
# model = "claude-sonnet-4-5-20250929"

# Common overrides you can enable later:
# [branch]
# format = "{user}/{date}/{message}"
# user = "cesar"
#
# [remote]
# api_base_url = "https://github.company.com/api/v3"
#
# [auth]
# gh_hostname = "github.company.com"
```

### Branch Name Format

Use `format` to template branch names with `{user}`, `{date}`, and `{message}` placeholders:

```toml
[branch]
format = "{user}/{date}/{message}"   # "cesar/02-11/add-login"
user = "cesar"                        # Optional: defaults to git config user.name
date_format = "%m-%d"                 # Optional: chrono strftime (default: "%m-%d")
```

Empty placeholders are cleaned up automatically.

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
st auth

# Option 3: Import from GitHub CLI auth (saves to credentials file)
st auth --from-gh
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
st auth status
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

## Codex Integration

Teach Codex how to use stax by installing the skill file into your Codex skills directory:

```bash
# Create skills directory if it doesn't exist
mkdir -p "${CODEX_HOME:-$HOME/.codex}/skills/stax"

# Download the stax skill file
curl -o "${CODEX_HOME:-$HOME/.codex}/skills/stax/SKILL.md" https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

This enables Codex to help you with stax workflows, create stacked branches, submit PRs, and more.

## Gemini CLI Integration

Teach Gemini CLI how to use stax by installing this repo's skill content as `GEMINI.md` in your project:

```bash
# From the stax repo root
curl -o GEMINI.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

Gemini CLI loads project instructions from `GEMINI.md`, so this gives it stack-aware workflow guidance for branch creation, submit flows, and related operations.

## OpenCode Integration

Teach OpenCode how to use stax by installing the skill file in OpenCode's skills directory:

```bash
mkdir -p ~/.config/opencode/skills/stax
curl -o ~/.config/opencode/skills/stax/SKILL.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

This enables OpenCode to help with stax workflows, stack operations, and PR generation.

## Freephite/Graphite Compatibility

stax uses the same metadata format as freephite and supports similar commands:

| freephite | st | graphite | st |
|-----------|------|----------|------|
| `fp ss` | `st ss` | `gt submit` | `st submit` |
| `fp bs` | `st branch submit` | `gt branch submit` | `st branch submit` |
| `fp us submit` | `st upstack submit` | `gt upstack submit` | `st upstack submit` |
| `fp ds submit` | `st downstack submit` | `gt downstack submit` | `st downstack submit` |
| `fp rs` | `st rs` | `gt sync` | `st sync` |
| `fp bc` | `st bc` | `gt create` | `st create` |
| `fp bco` | `st bco` | `gt checkout` | `st co` |
| `fp bu` | `st bu` | `gt up` | `st u` |
| `fp bd` | `st bd` | `gt down` | `st d` |
| `fp ls` | `st ls` | `gt log` | `st log` |

**Migration is instant** - just install stax and your existing stacks work.

## PR Templates

stax automatically discovers PR templates in your repository:

### Single Template
If you have one template at `.github/PULL_REQUEST_TEMPLATE.md`, stax uses it automatically:

```bash
st submit  # Auto-uses template, shows "Edit body?" prompt
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
st submit
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
st submit --template bugfix  # Use bugfix.md directly
st submit --no-template      # Empty body
st submit --edit             # Force editor open
```

## All Commands

<details>
<summary>Click to expand full command reference</summary>

### Stack Operations
| Command | Alias | Description |
|---------|-------|-------------|
| `st status` | `s`, `ls` | Show stack (simple view) |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack (ancestors + current + descendants) |
| `st merge` | | Merge PRs from bottom of stack to current |
| `st merge --when-ready` | | Merge with explicit wait-for-ready mode (legacy alias: `st merge-when-ready`) |
| `st sync` | `rs` | Pull trunk, delete merged branches |
| `st restack` | | Restack current stack (ancestors + current + descendants) |
| `st diff` | | Show diffs for each branch vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |

### Branch Management
| Command | Alias | Description |
|---------|-------|-------------|
| `st create <name>` | `c`, `bc` | Create stacked branch |
| `st checkout` | `co`, `bco` | Interactive branch picker |
| `st modify` | `m` | Stage all + amend current commit |
| `st rename` | `b r` | Rename branch and optionally edit commit message |
| `st branch track` | | Track an existing branch |
| `st branch track --all-prs` | | Track all your open PRs |
| `st branch untrack` | `ut` | Remove stax metadata for a branch (keep git branch) |
| `st branch reparent` | | Change parent of a branch |
| `st branch submit` | `bs` | Submit only current branch |
| `st branch delete` | | Delete a branch |
| `st branch fold` | | Fold branch into parent |
| `st branch squash` | | Squash commits on branch |
| `st detach` | | Remove branch from stack, reparent children |
| `st reorder` | | Interactively reorder branches in stack |
| `st upstack restack` | | Restack current branch + descendants |
| `st upstack submit` | | Submit current branch + descendants |
| `st downstack get` | | Show branches below current |
| `st downstack submit` | | Submit ancestors + current branch |

### Navigation
| Command | Alias | Description |
|---------|-------|-------------|
| `st up [n]` | `u`, `bu` | Move up n branches |
| `st down [n]` | `d`, `bd` | Move down n branches |
| `st top` | | Move to stack tip |
| `st bottom` | | Move to stack base |
| `st trunk` | `t` | Switch to trunk |
| `st prev` | `p` | Toggle to previous branch |

### Interactive
| Command | Description |
|---------|-------------|
| `st` | Launch interactive TUI |
| `st split` | Interactive TUI to split branch into multiple stacked branches |

### Recovery
| Command | Description |
|---------|-------------|
| `st abort` | Abort in-progress rebase/conflict resolution |
| `st undo` | Undo last operation (restack, submit, etc.) |
| `st undo <op-id>` | Undo a specific operation by ID |
| `st redo` | Re-apply the last undone operation |

### Health & Testing
| Command | Description |
|---------|-------------|
| `st validate` | Validate stack metadata (orphans, cycles, staleness) |
| `st fix` | Auto-repair broken metadata |
| `st fix --dry-run` | Preview fixes without applying |
| `st test <cmd>` | Run a command on each branch in the stack |
| `st test <cmd> --fail-fast` | Stop after first failure |
| `st test <cmd> --all` | Run on all tracked branches |

### Utilities
| Command | Description |
|---------|-------------|
| `st auth` | Set GitHub token (`--from-gh` supported) |
| `st auth status` | Show active GitHub auth source and resolution order |
| `st config` | Show configuration |
| `st doctor` | Check repo health |
| `st demo` | Interactive tutorial (no auth/repo needed) |
| `st continue` | Continue after resolving conflicts |
| `st pr` | Open PR in browser |
| `st open` | Open repository in browser |
| `st ci` | Show CI status for current branch (full table with ETA) |
| `st ci --stack` | Show CI status for all branches in current stack |
| `st ci --all` | Show CI status for all tracked branches |
| `st ci --watch` | Watch CI until completion (polls every 15s, records history) |
| `st ci --watch --interval 30` | Watch with custom polling interval in seconds |
| `st ci --verbose` | Compact summary cards instead of full per-check table |
| `st ci --json` | Output CI status as JSON |
| `st copy` | Copy branch name to clipboard |
| `st copy --pr` | Copy PR URL to clipboard |
| `st comments` | Show PR comments with rendered markdown |
| `st comments --plain` | Show PR comments as raw markdown |
| `st standup` | Show your recent activity for standups |
| `st standup --hours 48` | Look back 48 hours instead of default 24 |
| `st standup --json` | Output activity as JSON for scripting |
| `st changelog <from> [to]` | Generate changelog between two refs |
| `st changelog v1.0 --path src/` | Changelog filtered by path (monorepo) |
| `st changelog v1.0 --json` | Output changelog as JSON |
| `st generate --pr-body` | Generate PR body with AI and update the PR |
| `st generate --pr-body --edit` | Generate and review in editor before updating |

### Common Flags
- `st create -m "msg"` - Create branch with commit message
- `st create -a` - Stage all changes
- `st create -am "migrate checkout webhooks to v2"` - Create branch from message, stage all changes, and commit
- `st branch create --message "msg" --prefix feature/` - Create with explicit message and prefix
- `st branch reparent --branch feature-a --parent main` - Reparent a specific branch
- `st rename new-name` - Rename current branch
- `st rename -e` - Rename and edit commit message
- `st branch rename --push` - Rename and update remote branch in one step
- `st branch squash --message "Squashed commit"` - Squash branch commits with explicit message
- `st branch fold --keep` - Fold branch into parent but keep branch
- `st submit --draft` - Create PRs as drafts
- `st branch submit` / `st bs` - Submit current branch only
- `st upstack submit` - Submit current branch and descendants
- `st downstack submit` - Submit ancestors and current branch
- `st submit --yes` - Auto-approve prompts
- `st submit --no-pr` - Push branches only, skip PR creation/updates
- `st submit --no-fetch` - Skip `git fetch`; use cached remote-tracking refs
- `st submit --open` - Open the current branch PR in browser after submit (`st ss --open` / `st bs --open`)
- `st submit --force` - Submit even when restack check fails
- `st submit --no-prompt` - Use defaults, skip interactive prompts
- `st submit --template <name>` - Use specific template by name (skip picker)
- `st submit --no-template` - Skip template selection (no template)
- `st submit --edit` - Always open editor for PR body
- `st submit --ai-body` - Generate PR body with AI during submit
- `st submit --reviewers alice,bob` - Add reviewers
- `st submit --labels bug,urgent` - Add labels
- `st submit --assignees alice` - Assign users
- `st submit --rerequest-review` - Re-request review from existing reviewers when updating PRs
- `st submit --quiet` - Minimize submit output
- `st submit --verbose` - Show detailed submit output, including GitHub API request counts
- `st merge --all` - Merge entire stack
- `st merge --method squash` - Choose merge method (squash/merge/rebase)
- `st merge --dry-run` - Preview merge without executing
- `st merge --when-ready` - Use explicit wait-for-ready mode (legacy: `st merge-when-ready`)
- `st merge --when-ready --interval 10` - Use custom poll interval in seconds
- `st merge --no-wait` - Don't wait for CI, fail if not ready
- `st merge --no-delete` - Keep branches after merge
- `st merge --no-sync` - Skip the automatic post-merge `st rs --force`
- `st merge --timeout 60` - Wait up to 60 minutes for CI per PR
- `st merge --quiet` - Minimize merge output
- `st restack --auto-stash-pop` - Auto-stash/pop dirty target worktrees during restack
- `st restack --all` - Restack all branches in current stack
- `st restack --continue` - Continue after resolving restack conflicts
- `st restack --submit-after ask|yes|no` - After restack, ask/auto-submit/skip `st ss`
- `st restack --quiet` - Minimize restack output
- `st upstack restack --auto-stash-pop` - Auto-stash/pop when restacking descendants
- `st rs --restack --auto-stash-pop` - Sync, restack, auto-stash/pop dirty worktrees (`rs` = sync alias)
- `st sync --force` - Force sync without prompts
- `st sync --safe` - Avoid hard reset when updating trunk
- `st sync --continue` - Continue after resolving sync/restack conflicts
- `st sync --quiet` - Minimize sync output
- `st sync --verbose` - Show detailed sync output
- `st cascade --no-pr` - Restack and push branches; skip PR creation/updates
- `st cascade --no-submit` - Restack only, no remote interaction
- `st cascade --auto-stash-pop` - Auto-stash/pop dirty target worktrees during cascade restack
- `st sync --restack` - Sync and rebase all branches
- `st status --stack <branch>` - Show only one stack
- `st status --current` - Show only current stack
- `st status --compact` - Compact output
- `st status --json` - Output as JSON
- `st log --stack <branch> --current --compact --json` - Filter log output
- `st checkout --trunk` - Jump directly to trunk
- `st checkout --parent` - Jump to parent branch
- `st checkout --child 1` - Jump to first child branch
- `st ci --refresh` - Bypass CI cache
- `st undo --yes` - Undo without prompts
- `st undo --no-push` - Undo locally only, skip remote
- `st undo --quiet` - Minimize undo output
- `st redo --quiet` - Minimize redo output
- `st auth --token <token>` - Set GitHub token directly
- `st generate --pr-body --edit` - Generate and review in editor
- `st generate --pr-body --agent codex` - Use specific AI agent
- `st generate --pr-body --agent gemini` - Use Gemini CLI as the agent
- `st generate --pr-body --agent opencode` - Use OpenCode as the agent
- `st generate --pr-body --model claude-haiku-4-5-20251001` - Use specific model

**CI/Automation example:**
```bash
st submit --draft --yes --no-prompt
st merge --yes --method squash
```

</details>

## Agent Worktrees

`stax agent` lets you spin up isolated Git worktrees for parallel AI agents (Cursor, Codex, Aider, etc.) while keeping everything visible and manageable inside stax.

Each agent gets its own directory and branch. The main repo stays clean. Stax metadata, restack, undo, and the TUI all work across agent worktrees automatically.

### Quick start

```bash
# Create a worktree + stacked branch and open it in Cursor
stax agent create "Add dark mode" --open-cursor

# Reattach to a closed agent session
stax agent open add-dark-mode

# See all registered worktrees
stax agent list

# Restack all agent branches at once
stax agent sync

# Remove a finished worktree (optionally delete the branch too)
stax agent remove add-dark-mode --delete-branch

# Clean up dead entries
stax agent prune
```

### Real-world example: running 3 agents in parallel

Say you have a feature branch and want Codex, Claude Code, and OpenCode each tackling a different sub-task simultaneously — without them touching each other's files.

**Step 1 — spin up three isolated worktrees (one command each):**

```bash
stax agent create "Add dark mode" --open-codex
stax agent create "Fix auth token refresh" --open-cursor
stax agent create "Write API integration tests"
```

Each command creates an isolated directory under `.stax/trees/` with its own branch, stacks it on your current branch, and optionally opens it in the specified editor. Your main checkout is untouched.

```
main
 └── feature/my-feature                    ← your main checkout
      ├── add-dark-mode                     ← Codex working here
      ├── fix-auth-token-refresh            ← Cursor / Claude Code working here
      └── write-api-integration-tests       ← OpenCode / terminal working here
```

**Step 2 — point each agent at its directory:**

- Codex opened automatically via `--open-codex`
- For Claude Code: run `claude` inside `.stax/trees/fix-auth-token-refresh`
- For OpenCode: run `opencode` inside `.stax/trees/write-api-integration-tests`

Each agent sees only its own branch. They cannot conflict with each other.

**Step 3 — check on things while agents run:**

```bash
stax agent list   # all three worktrees, their branches, existence status
stax status       # all three branches appear in the stack tree as normal
```

**Step 4 — come back later and reattach to a session:**

```bash
stax agent open                     # fuzzy picker to choose
stax agent open fix-auth-token-refresh   # or by name
```

**Step 5 — trunk moved while agents were running:**

```bash
git pull
stax agent sync   # restacks all three branches at once — no manual rebasing
```

**Step 6 — review and submit each branch normally:**

```bash
stax checkout add-dark-mode
stax submit
```

**Step 7 — clean up when done:**

```bash
stax agent remove add-dark-mode --delete-branch
stax agent remove fix-auth-token-refresh --delete-branch
stax agent remove write-api-integration-tests --delete-branch
```

> **What stax does not do:** it doesn't talk to the agents or tell them what to work on — that's still you. What it solves is directory isolation, branch tracking, restack-after-trunk-moves, and the "where did I leave that session" problem that makes running parallel agents messy in practice.

### How it works

```
stax agent create "Add dark mode" --open-cursor
  │
  ├─ slugifies title → "add-dark-mode"
  ├─ creates branch (respects your branch.format config)
  ├─ git worktree add .stax/trees/add-dark-mode <branch>
  ├─ writes stax metadata (parent branch, revision)
  ├─ registers in .git/stax/agent-worktrees.json
  ├─ adds .stax/trees/ to .gitignore
  └─ opens cursor -n .stax/trees/add-dark-mode
```

### All subcommands

| Command | Description |
|---------|-------------|
| `stax agent create <title>` | Create worktree + branch |
| `stax agent open [name]` | Reopen in editor (fuzzy picker if no name) |
| `stax agent list` | Show all registered worktrees |
| `stax agent register` | Register current dir as an agent worktree |
| `stax agent remove <name>` | Remove worktree (add `--delete-branch` to also delete the branch) |
| `stax agent prune` | Remove dead registry entries + run `git worktree prune` |
| `stax agent sync` | Restack all registered worktrees |

### Config

Add to `~/.config/stax/config.toml` to customize:

```toml
[agent]
worktrees_dir = ".stax/trees"    # relative to repo root
default_editor = "auto"          # "auto" | "cursor" | "codex" | "code"
post_create_hook = "npm install" # optional shell command run in new worktree
```

### Flags for `create`

```
--base <branch>       Base branch (defaults to current)
--stack-on <branch>   Same as --base
--open                Open in default editor after creation
--open-cursor         Open in Cursor
--open-codex          Open in Codex
--no-hook             Skip post_create_hook for this run
```

### Editor slash-command recipes

Ready-to-import slash command recipes live in `examples/`:

| File | Editor | Command |
|------|--------|---------|
| [`examples/cursor/stax-new-agent.md`](examples/cursor/stax-new-agent.md) | Cursor | `stax agent create "{{input}}" --open-cursor` |
| [`examples/codex/stax-new-agent.md`](examples/codex/stax-new-agent.md) | Codex | `stax agent create "{{input}}" --open-codex` |
| [`examples/generic/stax-new-agent.md`](examples/generic/stax-new-agent.md) | Any (auto-detect) | `stax agent create "{{input}}" --open` |

## Benchmarks

| Command | [stax](https://github.com/cesarferreira/stax) | [freephite](https://github.com/bradymadden97/freephite) | [graphite](https://github.com/withgraphite/graphite-cli) |
|---------|------|-----------|----------|
| `ls` (10-branch stack) | 46.8ms | 1374.0ms | 506.0ms |

Raw [`hyperfine`](https://github.com/sharkdp/hyperfine) results:

```
➜ hyperfine 'st ls' 'fp ls' 'gt ls' --warmup 5
Benchmark 1: st ls
  Time (mean ± σ):      46.8 ms ±   0.5 ms    [User: 7.9 ms, System: 8.8 ms]
  Range (min … max):    45.7 ms …  48.6 ms    57 runs

Benchmark 2: fp ls
  Time (mean ± σ):      1.374 s ±  0.011 s    [User: 0.417 s, System: 0.274 s]
  Range (min … max):    1.361 s …  1.394 s    10 runs

Benchmark 3: gt ls
  Time (mean ± σ):     506.0 ms ±  18.0 ms    [User: 220.9 ms, System: 69.2 ms]
  Range (min … max):   489.8 ms … 536.3 ms    10 runs

Summary
  st ls ran
   10.81 ± 0.40 times faster than gt ls
   29.35 ± 0.41 times faster than fp ls
```

![ls benchmark](https://quickchart.io/chart?c=%7B%22type%22%3A%22bar%22%2C%22data%22%3A%7B%22labels%22%3A%5B%22freephite%22%2C%22graphite%22%2C%22stax%22%5D%2C%22datasets%22%3A%5B%7B%22label%22%3A%22Time%20(ms)%22%2C%22data%22%3A%5B1374.0%2C506.0%2C46.8%5D%2C%22backgroundColor%22%3A%5B%22%23ff0000%22%2C%22%23008000%22%2C%22%230000ff%22%5D%7D%5D%7D%2C%22options%22%3A%7B%22plugins%22%3A%7B%22datalabels%22%3A%7B%22display%22%3Atrue%2C%22color%22%3A%22white%22%2C%22align%22%3A%22center%22%2C%22anchor%22%3A%22center%22%7D%7D%2C%22title%22%3A%7B%22display%22%3Atrue%2C%22text%22%3A%22ls%20benchmark%20(lower%20is%20better)%22%7D%2C%22scales%22%3A%7B%22y%22%3A%7B%22beginAtZero%22%3Atrue%2C%22max%22%3A1500%7D%7D%7D%7D)

## License

MIT
