<div align="center">
  <h1>stax</h1>
  <p>
    <strong>Stacked branches without the bookkeeping.</strong>
  </p>
  <p>
    Create linked PR stacks, restack after merges, merge bottom-up safely, and recover instantly when a rewrite goes sideways.
  </p>

  <p>
    <a href="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml"><img alt="CI" src="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml/badge.svg"></a>
    <a href="https://crates.io/crates/stax"><img alt="Crates.io" src="https://img.shields.io/crates/v/stax"></a>
    <a href="https://cesarferreira.github.io/stax/"><img alt="Docs" src="https://img.shields.io/badge/docs-live-blue"></a>
    <a href="LICENSE"><img alt="License" src="https://img.shields.io/badge/license-MIT-green"></a>
  </p>

  <img src="assets/screenshot.png" width="900" alt="stax TUI">
</div>

`stax` is a fast Rust CLI for shipping small, reviewable PRs. It gives you an interactive terminal UI, one-command stack submission, safe undo/redo, AI-assisted conflict resolution, and worktree lanes for parallel coding agents.

Install gives you both binaries: `stax` and the short alias `st`. This README uses `st`.

The core workflow looks like this:

```bash
st create auth-api
st create auth-ui
st ss
st rs --restack
```

Create the next branch, submit the stack, then clean up and rebase after the bottom PR lands.

## Why stax

- Keep PRs small without hand-editing PR bases or babysitting branch metadata
- Submit or update the whole stack in one command with `st ss`
- Sync merged branches and restack what is left with `st rs --restack`
- Merge from the bottom safely when PRs are ready with `st merge --when-ready` or `st merge --remote`
- Recover immediately from risky rewrites with `st undo` and `st redo`
- Resolve rebase conflicts, generate PR bodies, and write standups with AI
- Run parallel AI lanes on isolated worktrees that still behave like normal Git branches

## Install

```bash
# macOS / Linux
brew install cesarferreira/tap/stax

# Or with cargo-binstall
cargo binstall stax

# Verify
st --version
```

<details>
<summary>Manual binaries, Windows, and source builds</summary>

See the [full install guide](docs/getting-started/install.md) for GitHub Releases, Windows notes, and source installation.

</details>

## 60-Second Quick Start

```bash
# 1. Authenticate once for PR metadata and CI checks
gh auth login
st auth --from-gh

# 2. Create a stack
st create auth-api
st create auth-ui

# 3. Submit linked PRs
st ss

# 4. After the bottom PR merges on GitHub
st rs --restack
```

That is the common loop: create small branches, submit them as a linked stack, then sync and restack after merges.

## Key Workflows

### Cascade merge

Merge from the stack bottom up with readiness checks built in.

```bash
st merge
st merge --when-ready
st merge --remote
```

### AI conflict resolution

When a restack or merge stops on a rebase conflict, `st resolve` sends only the conflicted text files to your configured AI agent, applies the resolutions, and continues the rebase.

```bash
st resolve
st resolve --agent codex --model gpt-5.3-codex
```

### Worktree lanes

Run multiple AI or human coding lanes in isolated worktrees, all tracked as normal Git branches.

```bash
st lane fix-auth-refresh "Fix the token refresh edge case from issue #142"
st lane stabilize-ci "Stabilize the 3 flaky checkout tests"
st wt
```

## Core Commands

| Command | What it does |
|---|---|
| `st` | Launch the interactive TUI |
| `st ls` / `st ll` | Show stack status, PR details, and URLs |
| `st create <name>` | Create a branch stacked on the current branch |
| `st ss` | Submit the full stack and create or update linked PRs |
| `st rs --restack` | Sync trunk, clean merged branches, then rebase what is left |
| `st merge` | Merge from the stack bottom toward the current branch |
| `st resolve` | Resolve an in-progress rebase conflict with AI |
| `st undo` / `st redo` | Recover or re-apply risky history operations |
| `st lane` | Start or re-enter an AI worktree lane |
| `st generate --pr-body` | Generate a PR body from the current branch diff |

## Documentation

- Live docs: [cesarferreira.github.io/stax](https://cesarferreira.github.io/stax/)
- Docs index in this repo: [docs/index.md](docs/index.md)
- Quick start: [docs/getting-started/quick-start.md](docs/getting-started/quick-start.md)
- Command reference: [docs/commands/reference.md](docs/commands/reference.md)

## Contributing

Before opening a PR, follow the repo test policy from [AGENTS.md](AGENTS.md):

```bash
make test
# or
just test
```

## License

[MIT](LICENSE)
