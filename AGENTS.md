# AGENTS.md

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

## Verification Tiers

- **Draft PR gate (default):** run `cargo check`, `make lint-fast`, `git diff --check`, and focused `cargo nextest run <pattern>` coverage for the changed behavior. Reviewer and verifier must pass, but local `make lint` and `make test` are not required merely to commit and open a draft PR.
- **Full gate:** CI must run `make lint` and `make test` before a draft is promoted to ready-for-review or merged. If CI is unavailable, run both commands locally on the exact PR head.
- **High-risk exception:** run the full local gate before opening the draft when the change touches shared/core execution (`engine/`, `git/repo.rs`, `ops/`), build or test infrastructure, broad cross-cutting behavior, or security-critical behavior. The verifier may widen scope when concrete risk warrants it, and must explain why.
- PR descriptions and verification artifacts must say whether evidence came from the scoped draft gate, the full local gate, or CI.

## Test Command Policy

- **AI agents:** when full-suite validation is required, always run `make test`. On macOS this routes through Docker, which is the only sane way to run the entire integration suite — `cargo test` natively will be slow, flaky, and may exhaust file handles. Do not run a full suite for every localized draft by default; follow the verification tiers above.
- **Start Docker before running `make test`.** On macOS the Docker daemon is not always running; if `make test` fails with `failed to connect to the docker API at unix:///.../docker.sock`, ask the user to launch Docker Desktop (or run `open -a Docker`) and retry — do not silently fall back to `make test-native`.
- Do not run the full suite via `cargo test` in this repo.
- For full-suite validation, always use `make test`.
- On macOS, `make test` intentionally routes to the Docker fast path.
- Use native paths only when explicitly needed:
  - `make test-native` (guarded nextest path; validates the file-descriptor limit)
  - `make test-local-ramdisk`
  - `make test-local-fast`
- Targeted runs via `cargo nextest run <pattern>` are required for the scoped draft gate and encouraged for tight feedback loops. Switch to `make test` when the full gate or high-risk exception applies.
- Full test runs (`make test`, including native Linux fallback, plus `make test-docker` / `make test-container`) use the `test-container` Cargo profile (no debuginfo) and shared env (`STAX_DISABLE_UPDATE_CHECK`, `RUST_MIN_STACK`, capped/sanitized `NEXTEST_TEST_THREADS`). Container runs also use the pre-baked `stax-test` image (`make test-image`) and mold linker. CI uses the same `test-container` profile and mold on `ubuntu-latest`. For tight iteration, prefer `cargo nextest run --lib --bins` or a module filter before a full run.
- All integration tests compile into a **single** binary (`tests/all_tests.rs`, with `autotests = false` in `Cargo.toml`) so cargo links one test binary instead of ~50 — this is what keeps test builds fast. Because of this, there is only one `[[test]]` target named `all_tests`: `cargo test --test status_tests` no longer works. To scope a run, filter by module path instead, e.g. `cargo nextest run status_tests::` (one former file) or `cargo nextest run status_tests::status_json_output` (one test). When adding a new `tests/*_tests.rs` file, register it with a `#[path = "..."] mod ...;` entry in `tests/all_tests.rs`.

## Why

- This suite is process/filesystem heavy (`git` + `stax` subprocesses), and Linux Docker is dramatically faster and more stable than native macOS for full runs.
- Native macOS performance remains sensitive to endpoint-security tooling; do not assume a warm native timing will match Docker.

## Lint Command Policy

- During implementation, use `make lint-fast` for formatting plus Clippy on library and binary targets.
- Before opening a draft PR, `make lint-fast` is sufficient when the scoped draft gate applies. Run `make lint` for the full gate or high-risk exception.
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
