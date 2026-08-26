---
name: stax-verify
description: Run the mechanical quality gate for a stax change — build, lint, and tests via the repo's canonical Make targets — and report PASS/FAIL/BLOCKED with concrete command output. Use after review to objectively confirm a change works before release; a FAIL routes back to the implementer. Enforces the repo Test Command Policy (make test over cargo test; Docker on macOS).
---

# stax-verify — Verifying stax Changes

Produce the objective verdict. No claim without command output.

## Command policy (repo Test Command Policy — follow exactly)
- **Build & lint:** `cargo check`, `make lint-fast`, and `git diff --check` form the scoped draft gate. Run `make lint` when the full gate applies.
- **Tests:**
  - Fast feedback on changed areas: `cargo nextest run <pattern>` or a module prefix (e.g. `status_tests::`, `status_tests::status_json_output`).
  - Localized changes may PASS for draft PR creation after focused tests; a full suite is not required merely because the next step opens a draft.
  - Run `make test` for shared/core code (`engine/`, `git/repo.rs`, `ops/`), build/test infrastructure, broad cross-cutting behavior, security-critical behavior, or an explicitly requested full gate.
  - Do **NOT** run the full suite via `cargo test`. `cargo test --test <name>` no longer works — scope by module path instead. On macOS, `make test` intentionally routes through Docker.
- **Docker down:** if `make test` fails with `failed to connect to the docker API`, mark **BLOCKED** and tell the user to start Docker Desktop (`open -a Docker`) and retry. Do not silently fall back to `make test-native`.

## How to verify
1. Read `01_plan.md` (intended test matrix) and `02_impl.md` (changed files → scope).
2. Run scoped tests for changed areas. Run the full lint and test suite only when the full-gate risk criteria apply.
3. For each failure, capture the failing test name, the assertion, and the decisive output lines.

## Output
Write `_workspace/<run_id>/04_verify.md`:
```
# Verify — verdict: PASS | FAIL | BLOCKED
## Commands run (with decisive output)   (`cmd` → result / pass-fail counts)
## Failures                              (test::name — assertion — cause)
## Coverage note                         (draft/full gate? happy/error/edge covered? scoped vs full make test?)
```
Return **PASS** / **FAIL** / **BLOCKED** + a one-line reason.

## Non-negotiables
- A green build is not a PASS — tests decide.
- Never weaken/skip/delete tests or lint to get green. Report flakiness as flaky, with evidence.
- Environment failures are **BLOCKED**, not FAIL. Genuine test/lint failures are **FAIL** with per-item evidence so the fix can be precise.
- Read-only on source.
