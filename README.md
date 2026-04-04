<div align="center">
  <h1>stax</h1>
  <p>
    <strong>A modern CLI for stacked Git branches and PRs.</strong>
  </p>

  <p>
    <a href="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml"><img alt="CI" src="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml/badge.svg"></a>
    <a href="https://crates.io/crates/stax"><img alt="Crates.io" src="https://img.shields.io/crates/v/stax"></a>
    <img alt="Performance" src="https://img.shields.io/badge/~24ms-startup-blue">
    <img alt="TUI" src="https://img.shields.io/badge/TUI-ratatui-5f5fff">
    <img alt="License" src="https://img.shields.io/badge/license-MIT-green">
  </p>

  <img src="assets/screenshot.png" width="900" alt="stax screenshot">
</div>

Ship small, reviewable PR stacks quickly without giving up safety.

More than stacked branches: `stax` can merge an entire stack when CI turns green, resolve in-progress rebase conflicts with AI guardrails, run parallel AI worktree lanes as normal tracked branches, and generate PR bodies or standup summaries from the work you actually shipped.

`stax` installs both binaries: `stax` and the short alias `st`. This README uses `st`.

- Live docs: [cesarferreira.github.io/stax](https://cesarferreira.github.io/stax/)
- Docs index in this repo: [docs/index.md](docs/index.md)

## Why stax

- Replace one giant PR with a clean stack of small, focused PRs
- Keep shipping while lower-stack PRs are still in review
- Merge from the bottom automatically when PRs are ready, locally or remotely (`st merge --when-ready`, `st merge --remote`)
- Resolve in-progress rebase conflicts with AI, limited to the conflicted files (`st resolve`)
- Recover from risky restacks and rewrites immediately (`st undo`, `st redo`)
- Run parallel AI worktree lanes as normal tracked branches (`st lane ...`)
- Generate PR bodies and spoken standup summaries with your preferred AI agent
- Navigate the full stack and diffs from an interactive TUI

## Install

```bash
# Homebrew (macOS/Linux)
brew install cesarferreira/tap/stax

# Or cargo-binstall
cargo binstall stax
```

### Prebuilt binaries (no package manager needed)

Download the latest binary from [GitHub Releases](https://github.com/cesarferreira/stax/releases):

```bash
# macOS (Apple Silicon)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-aarch64-apple-darwin.tar.gz | tar xz
# macOS (Intel)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-apple-darwin.tar.gz | tar xz
# Linux (x86_64)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-unknown-linux-gnu.tar.gz | tar xz

# Move both binaries to ~/.local/bin
mkdir -p ~/.local/bin
mv stax st ~/.local/bin/

# If ~/.local/bin is not on your PATH, add it:
# echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc  # or ~/.bashrc
```

**Windows (x86_64):** download `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases), extract both `stax.exe` and `st.exe`, and place them in a directory on your `PATH`. See [Windows notes](#windows-notes) for shell and worktree limitations.

Verify install:

```bash
st --version
```

<a id="quick-start"></a>
## 60-Second Quick Start

Set up GitHub auth first (required for PR creation, CI checks, and review metadata).

```bash
# Option A (recommended): import GitHub CLI auth
gh auth login
st auth --from-gh

# Option B: enter token interactively
st auth

# Option C: env var
export STAX_GITHUB_TOKEN="ghp_xxxx"
```

By default, stax does not use ambient `GITHUB_TOKEN` unless you opt in with `auth.allow_github_token_env = true`.

```bash
# 1. Create stacked branches
st create auth-api
st create auth-ui

# 2. Inspect stack
st ls
# ◉  auth-ui 1↑
# ○  auth-api 1↑
# ○  main

# 3. Submit PRs for whole stack
st ss

# 4. After auth-api PR is merged on GitHub...

# Pull trunk, detect the merge, delete auth-api, reparent auth-ui → main
st rs

# Rebase auth-ui onto updated main
st restack

# Or do both in one shot:
st rs --restack
```

Result: two stacked branches, submitted as two linked PRs. After the bottom PR is merged, sync detects it, cleans up, and restack rebases the remaining branch onto trunk.

Picked the wrong trunk branch? Run `st trunk main` to switch it, or `st init --trunk <branch>` to reconfigure from scratch.

Next steps:
- [Getting Started: Quick Start](docs/getting-started/quick-start.md)
- [Workflow: Merge and Cascade](docs/workflows/merge-and-cascade.md)

<a id="core-commands"></a>
## Core Commands

| Command | What it does |
|---|---|
| `st` | Launch interactive TUI |
| `st ls` | Show stack with PR/rebase status |
| `st ll` | Show stack with PR URLs and details |
| `st create <name>` | Create a branch stacked on current |
| `st ss` | Submit full stack and create/update PRs |
| `st merge` | Merge PRs from stack bottom to current |
| `st merge --when-ready` | Wait/poll until mergeable, then merge |
| `st merge --remote` | Merge the stack remotely on GitHub while you keep working locally |
| `st rs` | Sync trunk and clean merged branches (no rebasing; confirmed cleanup can remove safe linked worktrees) |
| `st rs --restack` | Sync trunk, clean merged branches, then rebase stack |
| `st restack` | Rebase current stack onto parents locally (`--stop-here` skips descendants) |
| `st cascade` | Restack, push, and create/update PRs |
| `st split` | Split branch into stacked branches (by commit or `--hunk`) |
| `st undo` / `st redo` | Recover or re-apply risky operations |
| `st resolve` | Resolve an in-progress rebase conflict with AI and continue automatically |
| `st standup` | Summarize recent engineering activity |
| `st pr` | Open the current branch PR in the browser |
| `st pr list` | Show open pull requests in the current repo |
| `st issue list` | Show open issues in the current repo |
| `st generate --pr-body [--no-prompt]` | Generate PR body with AI |
| `st run <cmd>` (alias: `st test <cmd>`) | Run a command on each branch in stack |

For complete command and flag reference: [docs/commands/core.md](docs/commands/core.md) and [docs/commands/reference.md](docs/commands/reference.md).

## Key Capabilities

<a id="cascade-stack-merge"></a>
### Cascade Stack Merge

Merge from stack bottom up to your current branch with safety checks for CI/readiness.

```bash
# Merge from bottom -> current branch
st merge

# Wait for readiness explicitly before merging
st merge --when-ready

# Merge full stack regardless of current position
st merge --all
```

Read more: [docs/workflows/merge-and-cascade.md](docs/workflows/merge-and-cascade.md)

### Safe History Rewriting (Undo/Redo)

`stax` snapshots branch state before destructive operations (`restack`, `submit`, `reorder`) so recovery is immediate when something goes wrong.

```bash
st restack
st undo
st redo
```

Read more: [docs/safety/undo-redo.md](docs/safety/undo-redo.md)

### AI Conflict Resolution

When a restack or merge stops on a rebase conflict, `st resolve` sends only the currently conflicted text files to your configured AI agent, applies the returned resolutions, and continues the rebase automatically.

```bash
# Resolve the current rebase conflict with your configured AI agent
st resolve

# Or override the agent/model for one run
st resolve --agent codex --model gpt-5.3-codex
```

If the AI returns invalid output, touches a non-conflicted file, or leaves more conflicts behind than allowed, stax stops and keeps the rebase in progress so you can inspect or continue manually.

Read more: [docs/commands/core.md](docs/commands/core.md) and [docs/commands/reference.md](docs/commands/reference.md)

### Interactive TUI

Launch with no arguments to browse stacks, inspect the selected branch summary, scroll patches, and run common operations without leaving the terminal.

```bash
st
```

<p align="center">
  <img alt="stax TUI" src="assets/tui.png" width="800">
</p>

Read more: [docs/interface/tui.md](docs/interface/tui.md)

<a id="developer-worktrees"></a>
### Developer Worktrees

Work on multiple stacks in parallel without losing context. `st worktree` (alias `st wt`) creates and manages Git worktree lanes for existing or new branches, with shell integration for transparent `cd`.

```bash
# Open the worktree dashboard (interactive terminals only)
st wt

# One-time shell integration setup
st shell-setup --install   # writes ~/.config/stax/shell-setup.sh and sources it from ~/.zshrc
# Later stax upgrades refresh the generated shell-setup file automatically

# Create a fresh random lane or a named lane
st worktree create
st worktree create payments-api

# List all worktrees (* = current)
st worktree list

# Jump to a worktree (transparent cd via shell function)
st worktree go payments-api
# or the quick alias:
sw payments-api

# Remove when done
st worktree remove payments-api
```

Shortcuts: `st w` (list), `st wtc [branch]` (create), `st wtgo <name>` (go), `st wtrm <name>` (remove). In an interactive terminal, bare `st wt` opens the worktree dashboard and uses tmux-backed re-entry for lanes.

Read more: [docs/workflows/multi-worktree.md](docs/workflows/multi-worktree.md)

<a id="worktree-lanes"></a>
### Worktree Lanes For AI

Run 2, 3, or 8 AI coding sessions in parallel without sharing one working directory.

Each lane is an isolated Git worktree with a real branch behind it. When stax creates the branch for a lane, it also writes normal stax metadata, so that lane shows up in `st ls`, participates in restack/sync/undo, and can be reopened instantly with `st wt go`.

```bash
# Spin up three lanes in parallel
st lane auth-refresh "fix token refresh edge cases"
st lane flaky-tests "stabilize the flaky test suite"
st wt c ui-polish --run "cursor ."
st lane review-pass "address the open PR comments"

# They are normal stax branches, not hidden scratch dirs
st ls

# Trunk moved while they were running? Restack every managed lane
st wt rs

# Re-enter any lane explicitly or open the lane picker
st lane flaky-tests
st lane

# Rich status + cleanup
st wt ll
st wt cleanup --dry-run
st wt cleanup
st wt prune
st wt rm auth-refresh --delete-branch
```

Read more: [docs/workflows/agent-worktrees.md](docs/workflows/agent-worktrees.md)

### AI PR Body + Standup Summary

Use your configured AI agent to draft PR bodies and generate daily standup summaries.

```bash
# Generate/update PR body from branch diff + context
st generate --pr-body

# Generate/update PR body without the review prompt
st generate --pr-body --no-prompt

# Spoken-style standup summary
st standup --summary
```

Read more: [docs/integrations/pr-templates-and-ai.md](docs/integrations/pr-templates-and-ai.md) and [docs/workflows/reporting.md](docs/workflows/reporting.md)

## Docs Map

If you want to...

- Install and configure quickly: [docs/getting-started/install.md](docs/getting-started/install.md)
- Learn stacked branch concepts: [docs/concepts/stacked-branches.md](docs/concepts/stacked-branches.md)
- Use day-to-day commands: [docs/commands/core.md](docs/commands/core.md)
- Explore full command/flag reference: [docs/commands/reference.md](docs/commands/reference.md)
- Navigate branches efficiently: [docs/commands/navigation.md](docs/commands/navigation.md)
- Merge, cascade, and keep stacks healthy: [docs/workflows/merge-and-cascade.md](docs/workflows/merge-and-cascade.md)
- Work across multiple worktrees: [docs/workflows/multi-worktree.md](docs/workflows/multi-worktree.md)
- Use developer worktrees (`st worktree`): [docs/workflows/multi-worktree.md](docs/workflows/multi-worktree.md)
- Run several AI worktree lanes in parallel: [docs/workflows/agent-worktrees.md](docs/workflows/agent-worktrees.md)
- Configure auth/branch naming/remote behavior: [docs/configuration/index.md](docs/configuration/index.md)
- Validate and repair metadata health: [docs/commands/stack-health.md](docs/commands/stack-health.md)

## Integrations

AI/editor integration guides:

- Claude Code: [docs/integrations/claude-code.md](docs/integrations/claude-code.md)
- Codex: [docs/integrations/codex.md](docs/integrations/codex.md)
- Gemini CLI: [docs/integrations/gemini-cli.md](docs/integrations/gemini-cli.md)
- OpenCode: [docs/integrations/opencode.md](docs/integrations/opencode.md)
- PR templates + AI generation: [docs/integrations/pr-templates-and-ai.md](docs/integrations/pr-templates-and-ai.md)

Shared skill/instruction file used across agents: [skills.md](skills.md)

## Performance & Compatibility

Absolute times vary by repo and machine. In the current `hyperfine` sample for this repo:

- `st ls` ran about 16.25x faster than [freephite](https://github.com/bradymadden97/freephite) and 10.05x faster than [Graphite](https://github.com/withgraphite/graphite-cli).
- `st rs` ran about 2.41x faster than [freephite](https://github.com/bradymadden97/freephite).
- stax is freephite/graphite compatible for common stacked-branch workflows.

Details:
- Benchmarks: [docs/reference/benchmarks.md](docs/reference/benchmarks.md)
- Compatibility: [docs/compatibility/freephite-graphite.md](docs/compatibility/freephite-graphite.md)

<a id="configuration"></a>
## Configuration

```bash
st config
st config --reset-ai
st config --reset-ai --no-prompt
```

Config file location:

```text
~/.config/stax/config.toml
```

Common settings include branch naming format, submit stack-links placement, auth source preferences, and enterprise GitHub API host overrides.

Example:

```toml
[submit]
stack_links = "body" # "comment" | "body" | "both" | "off"
```

If you want stax to reset and immediately re-prompt for the AI agent/model, run:

```bash
st config --reset-ai
```

Use `st config --reset-ai --no-prompt` to only clear the saved pairing without opening the picker.

Read full config reference: [docs/configuration/index.md](docs/configuration/index.md)

<a id="windows-notes"></a>
## Windows Notes

stax builds and runs on Windows (x86_64) with pre-built binaries available from [GitHub Releases](https://github.com/cesarferreira/stax/releases). Most commands work identically, with the following limitations:

**Shell integration is not available.** `st shell-setup` supports bash, zsh, and fish only. On Windows this means:

- `st wt c` and `st wt go` create/navigate worktrees but cannot auto-`cd` the parent shell. After running these commands, manually `cd` to the printed path.
- The `sw` quick alias is not available.
- `st wt rm` (with no argument, to remove the current worktree) cannot relocate the shell automatically. Specify the worktree name explicitly: `st wt rm <name>`.

**Worktree commands still work.** `st wt c`, `st wt go`, `st wt ls`, `st wt ll`, `st wt cleanup`, `st wt rm <name>`, `st wt prune`, and `st wt restack` all function normally — only the shell-level directory change is missing.

**tmux integration requires WSL or a Unix-like environment.** The `--tmux` flag and the worktree dashboard's tmux session management assume a Unix tmux binary is available.

All other stax features — stacked branches, PRs, restack, sync, undo/redo, TUI, AI generation — work on Windows without limitations.

## Contributing & License

- License: MIT
- Before opening a PR, run the repo test command policy from [AGENTS.md](AGENTS.md):

```bash
make test
# or
just test
```

For project docs and architecture, start at [docs/index.md](docs/index.md).
