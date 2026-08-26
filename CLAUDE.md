# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & Development Commands

```bash
# Build debug version
cargo build

# Build release version
cargo build --release

# Run full test suite (preferred)
make test

# Run full suite natively on host (explicit override)
make test-native

# Run a single test (filter by name; scope a former file with a module prefix, e.g. status_tests::)
cargo nextest run test_name

# Check + lint (both required before commit)
cargo check && cargo clippy -- -D warnings

# Format code
cargo fmt

# Install locally
cargo install --path .

# Run with arguments
cargo run -- <command>
```

## Test Runner Policy

- Do not run the full suite via `cargo test` in this repo.
- While iterating, scope runs to what changed: `cargo nextest run test_name` (or a module prefix, e.g. `status_tests::`) for the tests you added or touched.
- Run `make test` once before considering work done, before opening a PR, or whenever a change touches shared/core logic (e.g. `engine/`, `git/repo.rs`) that many tests depend on — CI also runs the full suite, but catching a shared-code regression locally is cheaper than a post-push CI failure.
- On macOS, this route intentionally uses Docker for speed and consistency.
- All integration tests compile into ONE binary (`tests/all_tests.rs`; `autotests = false` in `Cargo.toml`) so cargo links a single test binary instead of ~50. When you add a new `tests/<name>_tests.rs`, you MUST register it in `tests/all_tests.rs` with `#[path = "<name>_tests.rs"] mod <name>_tests;`, and reach shared helpers via `use crate::common;` (not `mod common;`). `cargo test --test <name>` only accepts `all_tests`; scope runs by module path instead.

## Architecture Overview

stax is a Rust CLI for managing stacked Git branches and PRs, compatible with freephite's metadata format.

### Module Structure

- **`src/main.rs`** - Binary entry point; delegates to `stax::cli::run()`.
- **`src/cli.rs`** - CLI definition using clap derive macros. Defines `Commands` enum with all subcommands, their flags, and the dispatch logic.
- **`src/commands/`** - Command implementations. Each file corresponds to a CLI command (status, submit, sync, restack, etc.).
 - `worktree/` - `stax worktree` subcommands: create, list, go, remove, cleanup, restack, ll
 - `worktree/ai.rs` - `stax lane` command: AI worktree lane shortcut (tmux-backed, agent launcher)
 - `shell_setup.rs` - `stax setup` (or `st setup`) for shell function installation
- **`src/engine/`** - Core domain logic:
  - `metadata.rs` - `BranchMetadata` struct stored per-branch (parent name, parent revision, PR info)
  - `stack.rs` - `Stack` struct that builds the full branch tree from metadata
- **`src/git/`** - Git operations:
  - `repo.rs` - `GitRepo` wrapper around git2 with rebase, checkout, merge operations; also hosts `WorktreeInfo` (pub), `list_worktrees()` (pub), `worktree_create()`, `worktree_remove()`, `worktrees_dir()`, `main_repo_workdir()`
  - `refs.rs` - Low-level ref operations for storing metadata in `refs/branch-metadata/<branch>`
- **`src/github/`** - GitHub API client using octocrab for PR creation/updates
- **`src/config/mod.rs`** - User configuration (`~/.config/stax/config.toml`) and credential management

### Metadata Storage

Branch metadata is stored in Git refs at `refs/branch-metadata/<branch>` as JSON blobs. This format is compatible with freephite:

```json
{
  "parentBranchName": "main",
  "parentBranchRevision": "abc123...",
  "prInfo": { "number": 42, "state": "OPEN" }
}
```

The trunk branch is stored at `refs/stax/trunk`.

## Harness: stax-dev

**Goal:** Accelerate stax feature development and bug fixing via a planner → implementer → verifier agent pipeline.

**Trigger:** For any code change in the stax codebase — new commands, bug fixes, refactors, behavior changes — use the `stax-dev` skill. Direct questions about usage or architecture don't need the harness.

**Change History:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-06-19 | Initial harness build | All | New harness |

---

### Key Patterns

- Commands that don't need a repo (auth, config, doctor) are handled before `ensure_initialized()` in cli.rs
- The `Stack::load()` method builds the complete branch tree by reading all metadata refs
- `BranchMetadata::needs_restack()` compares stored `parentBranchRevision` against current parent HEAD
- GitHub token priority: `STAX_GITHUB_TOKEN` > `GITHUB_TOKEN` > `~/.config/stax/.credentials`
- `WorktreeInfo` is public; `list_worktrees()` is public — used by both `stax worktree` and `stax agent` commands
- `stax worktree` stores managed worktrees in `.worktrees/` (repo root); `stax agent` uses `.stax/trees/` (configurable via `agent.worktrees_dir`)
- Shell integration detection uses `std::env::var("STAX_SHELL_INTEGRATION")` — the env var is exported by the shell function printed by `stax setup --print`
