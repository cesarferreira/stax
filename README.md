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

## Why stax?

- Ship stacks, not mega-PRs
- Keep rebases safe with transactional recovery (`stax undo` / `stax redo`)
- Use an interactive TUI for stack navigation, diffs, and reorder/split flows
- Submit and update PR stacks with correct parent relationships

## Install

```bash
# Homebrew (macOS/Linux)
brew tap cesarferreira/tap && brew install stax

# Or with cargo binstall
cargo binstall stax
```

Both `stax` and `st` are installed automatically.

## Quick Start

```bash
# GitHub auth (recommended)
gh auth login
stax auth --from-gh

# Create a small stack
stax create auth-api
stax create auth-ui

# Submit PRs for the stack
stax ss
```

## Documentation

Detailed documentation is now split into focused pages and powered by MkDocs Material.

- [Home docs index](docs/index.md)
- [Install](docs/getting-started/install.md)
- [Quick start](docs/getting-started/quick-start.md)
- [Interactive TUI](docs/interface/tui.md)
- [Core commands](docs/commands/core.md)
- [Merge and cascade workflows](docs/workflows/merge-and-cascade.md)
- [Undo and redo safety model](docs/safety/undo-redo.md)
- [Configuration reference](docs/configuration/index.md)
- [PR templates and AI generation](docs/integrations/pr-templates-and-ai.md)

## Run docs locally

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r docs/requirements.txt
mkdocs serve
```

Then open `http://127.0.0.1:8000`.

## License

MIT
