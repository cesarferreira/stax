<div align="center">
  <h1>stax</h1>
  <p>
    <strong>A modern CLI for stacked Git branches and PRs.</strong>
  </p>
  <p>
    Built in Rust for speed, inspired by <a href="https://github.com/bradymadden97/freephite">freephite</a> but reimagined with a cleaner UX, better error messages, and new features.
  </p>

  <p>
    <img alt="Startup" src="https://img.shields.io/badge/startup-~18ms-brightgreen">
    <img alt="Git" src="https://img.shields.io/badge/git-git2-f34f29">
    <img alt="Async" src="https://img.shields.io/badge/async-tokio-2f74c0">
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
  </p>
</div>

## Why stax?

- **Fast** - Native Rust binary starts in ~18ms (vs ~300ms for Node.js tools)
- **Modern UX** - Clear error messages with actionable suggestions
- **Visual stack view** - Beautiful tree rendering with colors and PR status
- **Flexible** - Force flags, detailed logs, and smart defaults
- **Compatible** - Uses same metadata format as freephite (migrate instantly)

## Install

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

# When parent branch changes, sync and restack
stax rs --restack
```

## Initialization

On first run, stax will initialize the repository by selecting a trunk branch (usually `main` or `master`). In non-interactive mode, it auto-detects the trunk if possible.

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

### Full Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `stax status` | `s`, `ls` | Show the current stack (simple view) |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax diff` | | Show diffs for each branch vs parent + aggregate stack diff |
| `stax range-diff` | | Show range-diff for branches that need restack |
| `stax sync` | `rs` | Pull trunk, delete merged branches |
| `stax sync --restack` | | Also restack after syncing |
| `stax restack` | | Rebase current branch onto parent |
| `stax restack --all` | | Restack all branches that need it |
| `stax submit` | `ss` | Push and create/update PRs |
| `stax submit --draft` | | Create PRs as drafts |
| `stax submit --no-pr` | | Just push, skip PR creation |
| `stax checkout [branch]` | `co`, `bco` | Checkout a branch (interactive if no arg) |
| `stax continue` | `cont` | Continue after resolving conflicts |
| `stax auth` | | Set GitHub personal access token |
| `stax config` | | Show config file path and contents |
| `stax doctor` | | Check stax configuration and repo health |

### Notable Flags and Behavior

#### Output and scripting

- `stax status --json --compact --stack <branch> --all --quiet`
- `stax log --json --compact --stack <branch> --all --quiet`
- Status/log output includes PR state, CI status, and ahead/behind counts.

#### Submit

- Prefills PR title/body from branch names, commit messages, and PR templates.
- `stax submit --reviewers alice,bob --labels bug --assignees alice --yes --no-prompt`
- Updates a single "stack summary" comment with PR links.

#### Sync/Restack

- Detects dirty working tree and offers to stash before restack/sync.
- `stax sync --safe` avoids `reset --hard` when updating trunk.
- `stax sync --continue` and `stax restack --continue` resume after conflicts.

#### Branching and navigation

- `stax bc --from <branch>` or `stax branch create --from <branch>` choose a base branch.
- `stax bc --prefix feature -m "auth"` overrides the configured prefix for this branch.
- `stax branch reparent --branch <name> --parent <name>` reattach branches.
- Parent selection is interactive when ambiguous; warnings when parent is missing on remote.
- `stax checkout --trunk`, `--parent`, `--child <n>` quick jumps; picker shows commits/PR info/restack status.

#### Diffs

- `stax diff` shows each branch vs parent plus an aggregate stack diff.
- `stax range-diff` highlights restack effects.

#### Doctor

- `stax doctor` checks repo health, remotes, and provider configuration.

### Branch Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `stax branch create <name>` | `b c` | Create a new stacked branch |
| `stax branch checkout` | `b co` | Interactive branch checkout |
| `stax branch track` | | Track an existing branch |
| `stax branch reparent` | | Change the parent of a tracked branch |
| `stax branch delete` | `b d` | Delete a branch |
| `stax branch fold` | `b f` | Fold current branch into its parent |
| `stax branch squash` | `b sq` | Squash commits on current branch |
| `stax branch up` | `b u` | Move up the stack (to child branch) |
| `stax branch down` | | Move down the stack (to parent branch) |

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
stax branch down  # move down to feat/auth-api
stax branch up    # move back up to feat/auth-ui

# Submit all PRs
stax ss
# Submitting 2 branch(es) to owner/repo...
#   Pushing feat/auth-api... ✓
#   Pushing feat/auth-ui... ✓
# Creating/updating PRs...
#   Creating PR for feat/auth-api... ✓ #123
#   Creating PR for feat/auth-ui... ✓ #124

# If main gets updated, sync and restack
stax rs --restack
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

Config is stored at `~/.config/stax/config.toml`:

```toml
[branch]
prefix = "cesar/"      # Auto-prefix new branches (e.g., "my-feature" → "cesar/my-feature")
date = false           # Add date to branch names (e.g., "2024-01-15-my-feature")
replacement = "-"      # Character to replace spaces and special chars

[remote]
name = "origin"        # Remote name to use
provider = "github"    # github, gitlab, gitea
base_url = "https://github.com" # Web base URL
api_base_url = "https://api.github.com" # Optional (GitHub Enterprise)
```

View your config with `stax config`.

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

## Migrating from freephite

stax uses the same metadata format as freephite, so migration is instant - just install stax and your existing stacks work:

```bash
# Your existing fp stacks just work
stax s  # shows your stack
stax rs # syncs repo
stax ss # submits PRs
```

## stax vs freephite

| | stax | freephite |
|---|---|---|
| Language | Rust | Node.js |
| Startup time | ~18ms | ~300ms |
| Binary size | ~6MB | ~50MB (with node_modules) |
| Install | Single binary | npm install |
| Status | Active | Unmaintained |
| Error messages | Detailed with suggestions | Basic |
| Visual tree | Colored, multi-level nesting | Basic |
| Force submit | `--force` flag | Not available |

## License

MIT
