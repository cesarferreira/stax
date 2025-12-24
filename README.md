# stax

> Fast stacked Git branches and PRs. A Rust rewrite of [freephite](https://github.com/bradymadden97/freephite).

**~18ms startup** vs ~300ms for the Node.js original.

## Installation

```bash
cargo install --git https://github.com/cesarferreira/stax
```

Or build from source:

```bash
git clone https://github.com/cesarferreira/stax
cd stax
cargo install --path .
```

## Quick Start

```bash
# Authenticate with GitHub (for PR creation)
stax auth

# Create your first stacked branch
stax bc feat/my-feature

# Make changes, commit, then create another branch on top
stax bc feat/another-feature

# View your stack
stax s

# Submit all branches as PRs
stax ss

# When parent branch changes, restack
stax rs
```

## Commands

### Shortcuts (freephite compatible)

| Command | Description |
|---------|-------------|
| `stax ss` | **S**ubmit **s**tack - push branches and create/update PRs |
| `stax rs` | **R**epo **s**ync - pull trunk, delete merged branches |
| `stax rs --restack` | Repo sync + restack branches |
| `stax bco` | **B**ranch **c**heck**o**ut - interactive branch picker |
| `stax bc <name>` | **B**ranch **c**reate - create a new stacked branch |
| `stax bc -m "msg"` | Create branch from message (spaces replaced) |
| `stax bu` | **B**ranch **u**p - move to child branch |
| `stax bd` | **B**ranch **d**own - move to parent branch |

### Full Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `stax status` | `s`, `ls` | Show the current stack (simple view) |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax sync` | `rs` | Pull trunk, delete merged branches |
| `stax sync --restack` | | Also restack after syncing |
| `stax restack` | | Rebase current branch onto parent |
| `stax restack --all` | | Restack all branches that need it |
| `stax submit` | `ss` | Push and create/update PRs |
| `stax submit --draft` | | Create PRs as drafts |
| `stax submit --no-pr` | | Just push, skip PR creation |
| `stax checkout [branch]` | `co`, `bco` | Checkout a branch (interactive if no arg) |
| `stax up` | `bu` | Move up the stack (to child branch) |
| `stax down` | `bd` | Move down the stack (to parent branch) |
| `stax continue` | `cont` | Continue after resolving conflicts |
| `stax auth` | | Set GitHub personal access token |

### Branch Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `stax branch create <name>` | `b c` | Create a new stacked branch |
| `stax branch checkout` | `b co` | Interactive branch checkout |
| `stax branch track` | | Track an existing branch |
| `stax branch delete` | `b d` | Delete a branch |

### Upstack/Downstack

| Command | Alias | Description |
|---------|-------|-------------|
| `stax upstack restack` | `us restack` | Restack all branches above current |
| `stax downstack get` | `ds get` | Show branches below current |

## Example Workflow

```bash
# Start on main
git checkout main

# Create a stacked branch for your feature
stax bc feat/auth-api

# Make changes and commit
git add . && git commit -m "Add auth API"

# Create another branch on top
stax bc feat/auth-ui

# Make more changes
git add . && git commit -m "Add auth UI"

# View your stack
stax s
# │ ◉  feat/auth-ui   ← you are here
# │ ○  feat/auth-api
# ○─┘  main

# Navigate the stack
stax bd  # move down to feat/auth-api
stax bu  # move back up to feat/auth-ui

# Submit all PRs
stax ss
# Submitting 2 branch(es) to owner/repo...
#   Pushing feat/auth-api... ✓
#   Pushing feat/auth-ui... ✓
# Creating/updating PRs...
#   Creating PR for feat/auth-api... ✓ #123
#   Creating PR for feat/auth-ui... ✓ #124

# If main gets updated, restack
git checkout feat/auth-api
stax rs
stax us restack  # restack branches above
```

## How It Works

stax stores stack metadata in Git refs (`refs/branch-metadata/<branch>`), the same format as freephite. This means:

- No external files or databases
- Metadata travels with your repo
- You can use both `stax` and `fp` on the same repo

Each branch tracks:
- Parent branch name
- Parent branch revision (to detect when restack is needed)
- PR info (number, state)

## Configuration

Config is stored at `~/.config/stax/config.toml` (safe to commit to dotfiles):

```toml
[branch]
prefix = "cesar/"      # Auto-prefix new branches (e.g., "my-feature" → "cesar/my-feature")
date = false           # Add date to branch names (e.g., "2024-01-15-my-feature")
replacement = "-"      # Character to replace spaces and special chars

[ui]
tips = true            # Show helpful tips
```

### Default Config

On first run, stax creates a default config:

```toml
[branch]
date = false
replacement = "-"

[ui]
tips = true
```

### GitHub Authentication

GitHub token is stored **separately** from config (not in dotfiles).

**Priority order:**
1. `STAX_GITHUB_TOKEN` env var (highest)
2. `GITHUB_TOKEN` env var
3. `~/.config/stax/.credentials` file (lowest)

```bash
# Option 1: stax-specific env var (recommended)
export STAX_GITHUB_TOKEN="ghp_xxxx"

# Option 2: Generic GitHub token
export GITHUB_TOKEN="ghp_xxxx"

# Option 3: Use stax auth command (saves to credentials file)
stax auth
```

Credentials file has `600` permissions on Unix.

## Migrating from freephite

stax uses the same metadata format as freephite. Just install stax and your existing stacks will work:

```bash
# Your existing fp stacks just work
stax s  # shows your stack
stax rs # restacks
stax ss # submits
```

## Why Rust?

| | stax (Rust) | fp (Node.js) |
|---|---|---|
| Startup time | ~18ms | ~300ms |
| Binary size | 6.2MB | ~50MB (with node_modules) |
| Dependencies | Compiled in | npm install required |

## License

MIT
