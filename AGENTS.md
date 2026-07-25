# AGENTS.md

## Harness: stax-dev

**Goal:** Accelerate stax feature development and bug fixing via a git-native worktree → plan → implement → review → verify → (optional) benchmark → commit → draft-PR pipeline with a bounded repair loop.

**Trigger:** For any code change to the stax CLI (new commands, flags, bugfixes, refactors, behavior changes), run the `/stax-dev` prompt. It orchestrates the `planner`, `codex`, `claude-reviewer`, `verifier`, `benchmarker`, and `release-manager` agents (`.pi/agents/`) via the `subagent` tool. Direct usage/architecture questions don't need the pipeline. Each task runs in its own worktree on a stacked branch; commit + draft PR are automatic on green; **merging to main is never automatic** and promoting a draft to ready-for-review is HITL-gated.

**Change History:**
| Date | Change | Target | Reason |
|------|--------|--------|--------|
| 2026-07-25 | Initial harness build (5 agents, 5 skills, `/stax-dev` orchestrator) | All | New harness |
| 2026-07-25 | Git-native evolution: added `benchmarker` (optional perf gate) + `stax-benchmark` skill; worktree-per-task + stacked branch; auto commit-on-green + auto draft PR; hard "never merge to main" rule | benchmarker.md, stax-benchmark/, release-manager.md, codex.md, stax-release/, stax-dev.md | Adopt git-native workflow constraints; keep verifier as mandatory correctness gate | 

## Test Command Policy

- **AI agents:** for full-suite validation always run `make test`. On macOS this routes through Docker, which is the only sane way to run the entire integration suite — `cargo test` natively will be slow, flaky, and may exhaust file handles. Default to `make test` and only fall back to native runners when explicitly told to.
- **Start Docker before running `make test`.** On macOS the Docker daemon is not always running; if `make test` fails with `failed to connect to the docker API at unix:///.../docker.sock`, ask the user to launch Docker Desktop (or run `open -a Docker`) and retry — do not silently fall back to `make test-native`.
- Do not run the full suite via `cargo test` in this repo.
- For full-suite validation, always use `make test`.
- On macOS, `make test` intentionally routes to the Docker fast path.
- Use native paths only when explicitly needed:
  - `make test-native` (guarded nextest path; validates the file-descriptor limit)
  - `make test-local-ramdisk`
  - `make test-local-fast`
- Targeted single-test runs via `cargo nextest run <pattern>` are fine and encouraged for tight feedback loops; switch to `make test` once changes are ready for verification.
- Full test runs (`make test`, including native Linux fallback, plus `make test-docker` / `make test-container`) use the `test-container` Cargo profile (no debuginfo) and shared env (`STAX_DISABLE_UPDATE_CHECK`, `RUST_MIN_STACK`, capped/sanitized `NEXTEST_TEST_THREADS`). Container runs also use the pre-baked `stax-test` image (`make test-image`) and mold linker. CI uses the same `test-container` profile and mold on `ubuntu-latest`. For tight iteration, prefer `cargo nextest run --lib --bins` or a module filter before a full run.
- All integration tests compile into a **single** binary (`tests/all_tests.rs`, with `autotests = false` in `Cargo.toml`) so cargo links one test binary instead of ~50 — this is what keeps test builds fast. Because of this, there is only one `[[test]]` target named `all_tests`: `cargo test --test status_tests` no longer works. To scope a run, filter by module path instead, e.g. `cargo nextest run status_tests::` (one former file) or `cargo nextest run status_tests::status_json_output` (one test). When adding a new `tests/*_tests.rs` file, register it with a `#[path = "..."] mod ...;` entry in `tests/all_tests.rs`.

## Why

- This suite is process/filesystem heavy (`git` + `stax` subprocesses), and Linux Docker is dramatically faster and more stable than native macOS for full runs.
- Native macOS performance remains sensitive to endpoint-security tooling; do not assume a warm native timing will match Docker.

## Lint Command Policy

- During implementation, use `make lint-fast` for formatting plus Clippy on library and binary targets.
- Before completion or submission, run `make lint` once to cover all targets and features.
- Use these Make targets instead of ad hoc `cargo clippy` commands so local and CI lint flags stay aligned.

## Documentation Policy

When a change touches user-visible behaviour — new commands, changed flags, renamed concepts, removed features, or updated defaults — the following must also be updated in the same PR:

- **`README.md`** — if the change affects the quick-start, core commands table, key capabilities, or any section a first-time user would read.
- **`docs/`** — the relevant page(s) under `docs/commands/`, `docs/workflows/`, `docs/configuration/`, etc.
- **`skills.md`** — the command map, high-value flags, workflow examples, best practices, or tips that reference the changed behaviour. This file is consumed by AI coding agents, so stale entries actively cause failures.

If none of these files need updating, leave a one-line note in the PR description explaining why.

## Testing Policy

Every non-trivial code change must include tests that cover:

- **Happy path** — the new or changed behaviour works correctly under normal inputs.
- **Error / bad path** — invalid inputs, missing preconditions, or failure modes return the expected error or graceful degradation.
- **Edge cases** — boundary conditions, empty inputs, and any known tricky states.

Prefer integration tests (under `tests/`) that exercise the full `stax` binary via subprocess for commands; use unit tests for pure logic. When adding a new command or flag, add at least one integration test that runs the command end-to-end in a temporary repo.
