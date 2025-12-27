<div align="center">
  <h1>stax</h1>
  <p>
    <strong>A modern CLI for stacked Git branches and PRs.</strong>
  </p>

  <p>
    <img alt="Version" src="https://img.shields.io/badge/version-0.2.1-blue">
    <a href="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml">
      <img alt="CI" src="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml/badge.svg">
    </a>
    <img alt="Performance" src="https://img.shields.io/badge/~21ms-startup-brightgreen">
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
  </p>
  <p>
    <img alt="stax screenshot" src="assets/screenshot.png">
  </p>
</div>

## What are Stacked Branches?

Instead of one massive PR with 50 files, stacked branches let you split work into small, reviewable pieces that build on each other:

```
○      bugfix/auth-validation-edge-case 1↑
○      feature/auth-validation 1↑
◉      feature/auth-login 1↑           ← you are here
○      feature/auth 1↑ 1↓ ⟳
│ ○    bugfix/payments-retries 1↑ 1↓ ⟳
│ ○    feature/payments-api 2↑
│ ○    feature/payments 1↑ 1↓ ⟳
│ │ ○  feature/profile-edit 1↑
│ │ ○  ☁ feature/profile 1↑ PR #42
○─┴─┘  ☁ main
```

Each branch is a focused PR. Reviewers see small diffs. You ship faster.

## Why stax?

- **Fast** - Native Rust binary, runs in ~21ms (17x faster than alternatives)
- **Visual** - Beautiful tree rendering showing your entire stack at a glance
- **Smart** - Tracks what needs rebasing, shows PR status, handles conflicts gracefully
- **Compatible** - Uses same metadata format as freephite (migrate instantly)

## Install

```bash
# Homebrew (macOS/Linux)
brew tap cesarferreira/tap && brew install stax

# Or with cargo
cargo install --git https://github.com/cesarferreira/stax
```

## Quick Start

```bash
# 1. Authenticate with GitHub
stax auth

# 2. Create stacked branches
stax create auth-api           # First branch off main
stax create auth-ui            # Second branch, stacked on first

# 3. View your stack
stax ls

# 4. Submit PRs for the whole stack
stax ss

# 5. After reviews, sync and rebase
stax rs --restack
```

## Core Commands

| Command | What it does |
|---------|--------------|
| `stax ls` | Show your stack with PR status and what needs rebasing |
| `stax create <name>` | Create a new branch stacked on current |
| `stax ss` | Submit stack - push all branches and create/update PRs |
| `stax rs` | Repo sync - pull trunk, clean up merged branches |
| `stax rs --restack` | Sync and rebase all branches onto updated trunk |
| `stax co` | Interactive branch checkout with fuzzy search |
| `stax u` / `stax d` | Move up/down the stack |
| `stax m` | Modify - stage all changes and amend current commit |
| `stax pr` | Open current branch's PR in browser |

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
| `stax co` | Interactive picker with fuzzy search |

## Reading the Stack View

```
○      feature/validation 1↑
◉      feature/auth 2↑ 1↓ ⟳
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
[branch]
prefix = "cesar/"      # Auto-prefix branches: "auth" → "cesar/auth"

[remote]
name = "origin"
provider = "github"    # github, gitlab, gitea
```

### GitHub Authentication

```bash
# Option 1: Environment variable (recommended)
export STAX_GITHUB_TOKEN="ghp_xxxx"

# Option 2: Interactive setup
stax auth
```

## Freephite/Graphite Compatibility

stax uses the same metadata format as freephite and supports similar commands:

| freephite | stax | graphite | stax |
|-----------|------|----------|------|
| `fp ss` | `stax ss` | `gt submit` | `stax submit` |
| `fp rs` | `stax rs` | `gt sync` | `stax sync` |
| `fp bc` | `stax bc` | `gt create` | `stax create` |
| `fp bco` | `stax bco` | `gt checkout` | `stax co` |
| `fp bu` | `stax bu` | `gt up` | `stax u` |
| `fp bd` | `stax bd` | `gt down` | `stax d` |
| `fp ls` | `stax ls` | `gt log` | `stax log` |

**Migration is instant** - just install stax and your existing stacks work.

## All Commands

<details>
<summary>Click to expand full command reference</summary>

### Stack Operations
| Command | Alias | Description |
|---------|-------|-------------|
| `stax status` | `s`, `ls` | Show stack (simple view) |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax submit` | `ss` | Push and create/update PRs |
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
| `stax branch track` | | Track an existing branch |
| `stax branch reparent` | | Change parent of a branch |
| `stax branch delete` | | Delete a branch |
| `stax branch fold` | | Fold branch into parent |
| `stax branch squash` | | Squash commits on branch |

### Navigation
| Command | Alias | Description |
|---------|-------|-------------|
| `stax up [n]` | `u`, `bu` | Move up n branches |
| `stax down [n]` | `d`, `bd` | Move down n branches |
| `stax top` | | Move to stack tip |
| `stax bottom` | | Move to stack base |
| `stax trunk` | `t` | Switch to trunk |

### Utilities
| Command | Description |
|---------|-------------|
| `stax auth` | Set GitHub token |
| `stax config` | Show configuration |
| `stax doctor` | Check repo health |
| `stax continue` | Continue after resolving conflicts |
| `stax pr` | Open PR in browser |

### Common Flags
- `stax create -m "msg"` - Create branch with commit message
- `stax create -a` - Stage all changes
- `stax create -am "msg"` - Stage all and commit
- `stax submit --draft` - Create PRs as drafts
- `stax submit --reviewers alice,bob` - Add reviewers
- `stax sync --restack` - Sync and rebase all branches
- `stax status --json` - Output as JSON

</details>

## Benchmarks

| Command | stax | freephite |
|---------|------|-----------|
| `ls` (10-branch stack) | 21ms | 363ms |

## License

MIT
