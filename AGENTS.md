# AGENTS.md

stax is a Rust CLI (edition 2024, MSRV in `Cargo.toml`) for stacked Git branches and PRs. Metadata is compatible with freephite.

## Commands

```bash
cargo build                         # debug build
cargo run -- <command>              # run stax
cargo nextest run <filter>          # scoped tests; module prefix works, e.g. status_tests::
cargo nextest run --lib --bins      # unit tests only
make lint-fast                      # fmt check + clippy (lib/bins); use while iterating
make lint                           # full lint, same as CI
make test                           # full suite; run before PR or after touching shared code
```

Never run the full suite with `cargo test`: it is slow and flaky here (process-heavy, exhausts file handles). Use `make test`.

- `make test` uses Docker on macOS. If the daemon is down, ask the user to start it. Do not silently fall back to `make test-native`.
- Use `make lint` rather than ad hoc `cargo clippy` so local and CI flags match.
- Run `make test` when a change touches `engine/`, `git/repo.rs`, `ops/`, build/test infra, or other cross-cutting behavior. Otherwise scoped nextest runs plus `make lint-fast` are enough for a draft PR; CI runs the full gate.
- Say in the PR description which evidence you have: scoped run, local full gate, or CI.

## Architecture

- `src/cli/` - clap definitions (`args.rs`) and dispatch (`mod.rs`). Commands that must work outside a repo (`setup`, `auth`, `config`, `doctor`, `web`) are handled before `ensure_initialized()`; otherwise they trigger `init` and break onboarding.
- `src/commands/` - one file per command; `worktree/` holds `stax wt` and `stax lane`.
- `src/application/` - presentation-neutral operations shared by the CLI, TUI, and web. It must not use terminal I/O, `println!`, `commands/`, `tui/`, or UI crates (`dialoguer`, `ratatui`, `console`, ...). `scripts/application-boundary-lint.py` enforces this in `make lint`.
- `src/engine/` - `Stack::load()` builds the branch tree from metadata; `BranchMetadata::needs_restack()` compares the stored parent revision to the parent's HEAD.
- `src/git/` - `repo.rs` (libgit2 `GitRepo` wrapper, worktree helpers) and `refs.rs` (metadata refs).
- `src/forge/` and `src/github/` - GitHub, GitLab, and Gitea clients behind `ForgeClient`.
- `src/ops/` - transactions and receipts that back undo/redo.
- `src/tui/`, `src/web/` - the interactive dashboard and the web workspace.
- `src/config/mod.rs` - `~/.config/stax/config.toml`. A new option needs an entry in `default_config.toml`.

Metadata lives in refs, not files: `refs/branch-metadata/<branch>` (JSON: `parentBranchName`, `parentBranchRevision`, `prInfo`), trunk in `refs/stax/trunk`.

Token priority: `STAX_GITHUB_TOKEN` > `GITHUB_TOKEN` > `~/.config/stax/.credentials`.

## Testing

- Every non-trivial change needs tests for the happy path, the error path, and edge cases.
- Prefer integration tests that run the real `stax` binary in a temp repo. Use unit tests for pure logic. A new command or flag needs at least one end-to-end test.
- All integration tests compile into ONE binary, `tests/all_tests.rs` (`autotests = false`). A new `tests/<name>_tests.rs` must be registered there with `#[path = "<name>_tests.rs"] mod <name>_tests;` and reach helpers via `use crate::common;`. `cargo test --test <name>` does not work; filter by module path.
- Tests must be hermetic: no GitHub tokens, no user `STAX_*` env, null `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`. Never call `env::set_var`/`remove_var` in tests; configure the child command instead (lint enforces this).

## Docs

A user-visible change (command, flag, rename, default) must update, in the same PR:

- `README.md` if a first-time user would see it
- the relevant page under `docs/`; keep one canonical page per command and have workflow pages link to it
- `skills.md`, which AI agents consume; stale entries cause failures

If none apply, say why in the PR description. Verify command/flag docs against `stax --help`.

## Gotchas

`learnings.md` has the accumulated lessons (TUI/picker rendering, shell integration, worktree removal, restack provenance, test hermeticity). Read the relevant section before changing those areas. The rules that bite most often:

- Descendant rebases (`restack`, `merge`, `sync --restack`) must keep `parent_branch_revision` and use provenance-aware `git rebase --onto`. A plain `git rebase <trunk>` replays already-squashed history.
- When merging stacks, retarget and rebase children before deleting a base branch, or GitHub auto-closes their PRs.
- Stack lane colors come from `src/commands/stack_palette.rs`. Do not duplicate palettes.
- Progress and prompts go to stderr, so `--json` and piped stdout stay clean.
- Graph traversal over metadata must be iterative and cycle-safe, since metadata can be corrupted.

## Claude Code

For any code change (new command, bug fix, refactor), use the `stax-dev` skill (`.claude/skills/stax-dev`), which runs a planner, implementer, and verifier pipeline. Plain usage or architecture questions don't need it.
