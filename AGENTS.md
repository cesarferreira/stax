# AGENTS.md

stax: Rust CLI for stacked Git branches and PRs. Branch metadata lives in git refs (`refs/branch-metadata/<branch>`, trunk in `refs/stax/trunk`), compatible with freephite. Everything else is discoverable from `src/`; this file only covers what isn't.

## Harness: stax-dev

**Goal:** Accelerate stax feature development and bug fixing via a git-native worktree → plan → implement → review → verify → (optional) benchmark → commit → draft-PR pipeline with a bounded repair loop.

**Trigger:** For any code change to the stax CLI (new commands, flags, bugfixes, refactors, behavior changes), run the `/stax-dev` prompt. It orchestrates the project-local `stax-planner`, `stax-implementer`, `stax-reviewer`, `stax-verifier`, `stax-benchmarker`, and `stax-release-manager` definitions under `.claude/agents/`. The former `.pi/agents/` definitions were intentionally moved to personal dotfiles and are not the project harness. Direct usage/architecture questions don't need the pipeline. Each task runs in its own worktree on a stacked branch; commit + current-branch-only draft PR submission (`stax branch submit --draft --ai --yes`) is automatic when the applicable gate is green; **merging to main is never automatic** and promoting a draft to ready-for-review is HITL-gated.

**Change History:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-07-25 | Initial harness build (5 agents, 5 skills, `/stax-dev` orchestrator) | All | New harness |
| 2026-07-25 | Git-native evolution: added `benchmarker` (optional perf gate) + `stax-benchmark` skill; worktree-per-task + stacked branch; auto commit-on-green + auto draft PR; hard "never merge to main" rule | benchmarker.md, stax-benchmark/, release-manager.md, codex.md, stax-release/, stax-dev.md | Adopt git-native workflow constraints; keep verifier as mandatory correctness gate |
| 2026-07-28 | Added `fallbackModels: claude-bridge/claude-sonnet-4-6` to the four `cursor/composer-2.5` agents so a provider outage (quota/auth/timeout) auto-routes instead of stalling the run; codex also falls back to `openai-codex/gpt-5.6-terra`. Extended benchmarker skip rule to cover pure delegation/deletion diffs with no new local hot-path work | codex.md, verifier.md, benchmarker.md, release-manager.md | Cursor was down + OpenAI-Codex usage-limited mid-run (stax-dev-03), forcing manual model overrides; benchmarker also burned time trying to bench a no-local-compute change |
| 2026-08-26 | Split verification into a fast scoped draft gate and a full CI/ready-for-review gate | AGENTS.md, verifier.md, stax-verify/, stax-dev.md | Avoid repeating the process-heavy full integration suite in every isolated worktree while retaining full validation before review or merge |
| 2026-08-27 | Reconciled the project-local harness with its documented git-native pipeline: added independent review, optional benchmark, bounded repair artifacts, and release ownership with current-branch-only draft submission | `.claude/agents/`, `.claude/skills/stax-dev/`, `skills.md`, AGENTS.md | Restore the missing commit/draft-PR path and keep full-stack submit, undraft, and merge outside automation |

## Commands

```bash
cargo nextest run <filter>   # scoped tests; a module prefix works, e.g. status_tests::
make lint-fast               # fmt check + clippy; run while iterating
make lint                    # full lint gate
make test                    # full suite; see rules below
```

- Never run the full suite with `cargo test` (process-heavy, exhausts file handles). Use `make test`.
- Use `make lint-fast` and focused nextest runs for the scoped draft gate; use `make lint` and `make test` for the full gate or high-risk changes.
- On macOS `make test` needs Docker. If the daemon is down, ask the user to start it instead of falling back to `make test-native`.
- Use the Make lint targets, not ad hoc `cargo clippy`, so flags match CI.
- State in the PR description whether evidence is a scoped run, a full local run, or CI.

## Verification Tiers

- **Draft PR gate (default):** run `cargo check`, `make lint-fast`, `git diff --check`, and focused `cargo nextest run <pattern>` coverage for the changed behavior. Reviewer and verifier must pass, but local `make lint` and `make test` are not required merely to commit and open a draft PR.
- **Full gate:** CI must run `make lint` and `make test` before a draft is promoted to ready-for-review or merged. If CI is unavailable, run both commands locally on the exact PR head.
- **High-risk exception:** run the full local gate before opening the draft when the change touches shared/core execution (`engine/`, `git/repo.rs`, `ops/`), build or test infrastructure, broad cross-cutting behavior, or security-critical behavior. The verifier may widen scope when concrete risk warrants it, and must explain why.
- PR descriptions and verification artifacts must say whether evidence came from the scoped draft gate, the full local gate, or CI.

## Test Command Policy

- **AI agents:** when full-suite validation is required, always run `make test`. On macOS this routes through Docker, which is the only sane way to run the entire integration suite — `cargo test` natively will be slow, flaky, and may exhaust file handles. Do not run a full suite for every localized draft by default; follow the verification tiers above.
- **Start Docker before running `make test`.** On macOS the Docker daemon is not always running; if `make test` fails because the Docker API is unavailable, ask the user to launch Docker Desktop instead of falling back to `make test-native`.
- Do not run the full suite via `cargo test` in this repo.
- For full-suite validation, always use `make test`.
- On macOS, `make test` intentionally routes to the Docker fast path.
- Use native paths only when explicitly needed:
  - `make test-native` (guarded nextest path; validates the file-descriptor limit)
  - `make test-local-ramdisk`
  - `make test-local-fast`
- Targeted runs via `cargo nextest run <pattern>` are required for the scoped draft gate and encouraged for tight feedback loops. Switch to `make test` when the full gate or high-risk exception applies.
- Full test runs (`make test`, including native Linux fallback, plus `make test-docker` / `make test-container`) use the `test-container` Cargo profile (no debuginfo) and shared env (`STAX_DISABLE_UPDATE_CHECK`, `RUST_MIN_STACK`, capped/sanitized `NEXTEST_TEST_THREADS`). Container runs also use the pre-baked `stax-test` image (`make test-image`) and mold linker. CI uses the same `test-container` profile and mold on `ubuntu-latest`. For tight iteration, prefer `cargo nextest run --lib --bins` or a module filter.
- All integration tests compile into a **single** binary (`tests/all_tests.rs`, with `autotests = false` in `Cargo.toml`) so cargo links one test binary instead of ~50 — this is what keeps test builds fast. Because of this, there is only one `[[test]]` target named `all_tests`: `cargo test --test status_tests` no longer works. To scope a run, filter by module path, e.g. `cargo nextest run status_tests::`. When adding a new `tests/*_tests.rs` file, register it with a `#[path = "..."] mod ...;` entry in `tests/all_tests.rs`.

## Why

- This suite is process/filesystem heavy (`git` + `stax` subprocesses), and Linux Docker is dramatically faster and more stable than native macOS for full runs.
- Native macOS performance remains sensitive to endpoint-security tooling; do not assume a warm native timing will match Docker.

## Lint Command Policy

- During implementation, use `make lint-fast` for formatting plus Clippy on library and binary targets.
- Before opening a draft PR, `make lint-fast` is sufficient when the scoped draft gate applies. Run `make lint` for the full gate or high-risk exception.
- Use these Make targets instead of ad hoc `cargo clippy` commands so local and CI lint flags stay aligned.

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

## Documentation Policy

When a change touches user-visible behaviour — new commands, changed flags, renamed concepts, removed features, or updated defaults — the following must also be updated in the same PR:

- **`README.md`** — if the change affects the quick-start, core commands table, key capabilities, or any section a first-time user would read.
- **`docs/`** — the relevant page(s) under `docs/commands/`, `docs/workflows/`, `docs/configuration/`, etc.; keep one canonical page per command.
- **`skills.md`** — the command map, high-value flags, workflow examples, best practices, or tips that reference the changed behaviour. This file is consumed by AI coding agents, so stale entries actively cause failures.

If none of these files need updating, leave a one-line note in the PR description explaining why. Check flags against `stax --help`.
