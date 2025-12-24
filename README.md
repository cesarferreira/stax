# gt

> Fast stacked Git branches and PRs. A Rust rewrite of [freephite](https://github.com/bradymadden97/freephite).

**~18ms startup** vs ~300ms for the Node.js original.

## Installation

```bash
cargo install --git https://github.com/cesarferreira/gt
```

Or build from source:

```bash
git clone https://github.com/cesarferreira/gt
cd gt
cargo install --path .
```

## Quick Start

```bash
# Authenticate with GitHub (for PR creation)
gt auth

# Create your first stacked branch
gt bc feat/my-feature

# Make changes, commit, then create another branch on top
gt bc feat/another-feature

# View your stack
gt s

# Submit all branches as PRs
gt ss

# When parent branch changes, restack
gt rs
```

## Commands

### Shortcuts (freephite compatible)

| Command | Description |
|---------|-------------|
| `gt ss` | **S**ubmit **s**tack - push branches and create/update PRs |
| `gt rs` | **R**e**s**tack - rebase current branch onto its parent |
| `gt bco` | **B**ranch **c**heck**o**ut - interactive branch picker |
| `gt bc <name>` | **B**ranch **c**reate - create a new stacked branch |
| `gt bd` | **B**ranch **d**elete - delete a branch and its metadata |

### Full Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `gt status` | `s`, `l`, `log` | Show the current stack |
| `gt restack` | | Rebase current branch onto parent |
| `gt restack --all` | | Restack all branches that need it |
| `gt submit` | | Push and create/update PRs |
| `gt submit --draft` | | Create PRs as drafts |
| `gt submit --no-pr` | | Just push, skip PR creation |
| `gt checkout [branch]` | `co` | Checkout a branch (interactive if no arg) |
| `gt continue` | `cont` | Continue after resolving conflicts |
| `gt auth` | | Set GitHub personal access token |

### Branch Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `gt branch create <name>` | `b c` | Create a new stacked branch |
| `gt branch checkout` | `b co` | Interactive branch checkout |
| `gt branch track` | | Track an existing branch |
| `gt branch delete` | `b d` | Delete a branch |

### Upstack/Downstack

| Command | Alias | Description |
|---------|-------|-------------|
| `gt upstack restack` | `us restack` | Restack all branches above current |
| `gt downstack get` | `ds get` | Show branches below current |

## Example Workflow

```bash
# Start on main
git checkout main

# Create a stacked branch for your feature
gt bc feat/auth-api

# Make changes and commit
git add . && git commit -m "Add auth API"

# Create another branch on top
gt bc feat/auth-ui

# Make more changes
git add . && git commit -m "Add auth UI"

# View your stack
gt s
#   ○ main
# │
# │ ○ feat/auth-api
# │
# │ ◉ feat/auth-ui ← you are here

# Submit all PRs
gt ss
# Submitting 2 branch(es) to owner/repo...
#   Pushing feat/auth-api... ✓
#   Pushing feat/auth-ui... ✓
# Creating/updating PRs...
#   Creating PR for feat/auth-api... ✓ #123
#   Creating PR for feat/auth-ui... ✓ #124

# If main gets updated, restack
git checkout feat/auth-api
gt rs
gt us restack  # restack branches above
```

## How It Works

gt stores stack metadata in Git refs (`refs/branch-metadata/<branch>`), the same format as freephite. This means:

- No external files or databases
- Metadata travels with your repo
- You can use both `gt` and `fp` on the same repo

Each branch tracks:
- Parent branch name
- Parent branch revision (to detect when restack is needed)
- PR info (number, state)

## Configuration

Config is stored at `~/.config/gt/config.toml`:

```toml
[github]
token = "ghp_xxxx"
```

## Migrating from freephite

gt uses the same metadata format as freephite. Just install gt and your existing stacks will work:

```bash
# Your existing fp stacks just work
gt s  # shows your stack
gt rs # restacks
gt ss # submits
```

## Why Rust?

| | gt (Rust) | fp (Node.js) |
|---|---|---|
| Startup time | ~18ms | ~300ms |
| Binary size | 6.2MB | ~50MB (with node_modules) |
| Dependencies | Compiled in | npm install required |

## License

MIT
