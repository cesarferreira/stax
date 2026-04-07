# stax ⚡

**The fastest stacked-branch workflow for Git**

Interactive TUI • Smart PR stacks • Safe undo/redo • AI guardrails • Written in Rust

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-%23000000.svg?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![CI](https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml/badge.svg)](https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml)

![stax TUI](assets/tui.png)

**Ship small, reviewable PRs faster — without giving up safety.**

stax gives you a beautiful interactive terminal UI, one-command stack submission, automatic cascade merging, transactional undo/redo, and AI-powered conflict resolution and summaries.

`stax` installs both binaries: `stax` and the short alias `st`. This README uses `st`.

## ✨ Features

- **Blazing-fast TUI** — tree view, diffs, reordering, splitting, and PR status at a glance
- **One-command PR stacks** — `st ss` pushes your entire stack and creates or updates linked PRs
- **Smart sync & cascade** — `st rs --restack` detects merged PRs, cleans up, and rebases automatically
- **Safe by default** — full undo/redo history with `st undo` / `st redo` for risky operations
- **AI superpowers** — resolve rebase conflicts automatically with `st resolve`, generate PR bodies, and create standup summaries
- **Parallel AI lanes** — run multiple AI agents on isolated, tracked branches with `st lane`
- **Works with your existing stack** — compatible with Graphite / freephite workflows

## 🚀 Quick Install

### Homebrew (macOS / Linux)
```bash
brew install cesarferreira/tap/stax
```

### Cargo binstall (fastest)
```bash
cargo binstall stax
```

### Prebuilt binaries
Download the latest release from [GitHub Releases](https://github.com/cesarferreira/stax/releases) and place `stax` + `st` in your `PATH`.

Then verify:
```bash
st --version
```

Need manual binaries, Windows notes, or source builds? See the [full install guide](docs/getting-started/install.md).

## 60-Second Quick Start

```bash
# 1. Authenticate (one time)
gh auth login
st auth --from-gh

# 2. Create a stacked branch
st create auth-api
st create auth-ui

# 3. Submit the whole stack as linked PRs
st ss

# 4. After the bottom PR merges on GitHub
st rs --restack
```

Done. Your stack is clean, rebased, and ready for the next feature.

**Full quick start & workflows -> [Live Docs](https://cesarferreira.github.io/stax/)**

## 📸 Screenshots

![TUI](assets/tui.png)
![Reordering stacks](assets/reordering-stacks.png)
![Standup summary](assets/standup.png)
![Screenshot](assets/screenshot.png)

## 📖 Documentation

- [Live Documentation](https://cesarferreira.github.io/stax/)
- [Commands Reference](https://cesarferreira.github.io/stax/commands/)
- [Getting Started](https://cesarferreira.github.io/stax/getting-started/quick-start/)

## Core Commands (most used)

| Command | What it does |
|----------------------|-------------------------------------------|
| `st` | Launch the interactive TUI |
| `st ls` | Show your stack with PR status |
| `st create <name>` | Create a new stacked branch |
| `st ss` | Submit the full stack as PRs |
| `st rs --restack` | Sync + restack after merges |
| `st resolve` | AI-powered conflict resolution |
| `st undo` / `st redo` | Safe rollback or re-apply |

All commands and flags are documented in the [full reference](https://cesarferreira.github.io/stax/commands/reference/).

---

**stax** is fully open source and local-first.<br>
Made with ❤️ by [César Ferreira](https://github.com/cesarferreira).

**License** — [MIT](LICENSE)
