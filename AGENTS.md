# AGENTS.md

stax: Rust CLI for stacked Git branches and PRs. Branch metadata lives in git refs (`refs/branch-metadata/<branch>`, trunk in `refs/stax/trunk`), compatible with freephite. Everything else is discoverable from `src/`; this file only covers what isn't.

## Commands

```bash
cargo nextest run <filter>   # scoped tests; a module prefix works, e.g. status_tests::
make lint-fast               # fmt check + clippy; run while iterating
make lint                    # full lint, same as CI; run before a PR
make test                    # full suite; see rules below
```

- Never run the full suite with `cargo test` (process-heavy, exhausts file handles). Use `make test`.
- Run `make test` before a PR only if you touched `src/engine/`, `src/git/repo.rs`, `src/ops/`, build/test infra, or other cross-cutting code. Otherwise scoped nextest + `make lint-fast` is enough; CI runs the full gate.
- On macOS `make test` needs Docker. If the daemon is down, ask the user to start it instead of falling back to `make test-native`.
- Use the Make lint targets, not ad hoc `cargo clippy`, so flags match CI.
- State in the PR description whether evidence is a scoped run, a full local run, or CI.

## Rules that aren't obvious from the code

- `src/application/` is presentation-neutral: no `println!`, stdin/stdout, `commands/`, `tui/`, or UI crates (`dialoguer`, `ratatui`, `console`). `make lint` enforces it. Put UI in `commands/` or `tui/`.
- Commands that must work outside a repo (`setup`, `auth`, `config`, `doctor`, `web`) are dispatched before `ensure_initialized()` in `src/cli/mod.rs`. Add new repo-less commands there, or they trigger `init`.
- Rebasing descendants (`restack`, `merge`, `sync --restack`) must preserve `parent_branch_revision` and use `git rebase --onto <new> <old>`. A plain `git rebase <trunk>` replays squash-merged history.
- When merging stacks, rebase and retarget children before deleting the base branch, or GitHub auto-closes their PRs.
- Walk the branch graph iteratively with cycle detection. Metadata can be corrupted, so never recurse over it.
- Stack colors come from `src/commands/stack_palette.rs`; don't add per-command palettes.
- Send progress and prompts to stderr so `--json` and piped stdout stay clean.
- A new config option needs an entry in `src/config/default_config.toml`.

`learnings.md` has more lessons on TUI/picker rendering, shell integration, worktree removal, and CI timing. Read the relevant part before touching those areas.

## Tests

- Cover the happy path, the error path, and edge cases. Prefer integration tests that run the real `stax` binary in a temp repo; a new command or flag needs at least one.
- All integration tests compile into ONE binary, `tests/all_tests.rs`. Register a new `tests/<name>_tests.rs` there with `#[path = "<name>_tests.rs"] mod <name>_tests;` and import helpers with `use crate::common;`. `cargo test --test <name>` won't work; filter by module path.
- Tests must be hermetic: no GitHub tokens, no user `STAX_*` env, null `GIT_CONFIG_GLOBAL`/`GIT_CONFIG_SYSTEM`. Never call `env::set_var`/`remove_var`; configure the child command (lint enforces this).

## Docs

A user-visible change (command, flag, rename, default) must update in the same PR: `README.md` if a new user would see it, the page under `docs/` (one canonical page per command), and `skills.md` (AI agents consume it, so stale entries cause failures). If none apply, say why in the PR description. Check flags against `stax --help`.
