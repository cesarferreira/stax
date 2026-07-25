---
name: stax-implement
description: Implement an approved plan in the stax Rust codebase — write/edit source, add happy/error/edge tests, keep README/docs/skills.md in sync, and self-verify with cargo check + make lint-fast. Use when implementing a stax feature/bugfix, or on a repair pass fixing specific reviewer/verifier FAIL items. Optionally delegate scoped steps to the codex CLI when present.
---

# stax-implement — Implementing stax Changes

Make the plan real with the smallest correct diff. You own the correctness of the final change — including any output from a delegated `codex` CLI call.

## Workflow
1. Read `_workspace/<run_id>/01_plan.md` fully. On repair passes also read `03_review.md` and `04_verify.md`.
2. Implement step by step, touching only the files the plan names (plus strictly required neighbors).
3. Add the planned tests (see below).
4. Update flagged docs: `README.md`, `docs/`, `skills.md`.
5. Self-verify: `cargo check && make lint-fast`. Fix everything you introduced.
6. Write the change log to `_workspace/<run_id>/02_impl.md`: every file changed, why, and how to verify it.

## Optional: codex CLI delegation
If `codex` is on PATH, you may hand a tightly-scoped step to it via bash and then read, integrate, and verify the result. Never present unverified CLI output as complete. If unavailable, implement directly.

## Tests (repo Testing Policy)
- Cover happy path, error/bad path, edge cases.
- Integration tests: add `tests/<area>_tests.rs` and register it in `tests/all_tests.rs`:
  `#[path = "<name>_tests.rs"] mod <name>_tests;`. Reach shared helpers via `use crate::common;` (not `mod common;`).
- Drive the real `stax` binary in a temp repo for command behavior; unit tests for pure logic.

## stax code conventions
- Command scaffolding: `Commands` variant (`src/cli.rs`) → dispatch → `src/commands/<name>.rs` → register in `mod.rs`.
- Extend `OpKind` (`src/ops/receipt.rs`) for transaction-backed commands; keep `cascade.rs` in sync with `restack.rs`.
- `rt.block_on()` for async GitHub calls; `LiveTimer::maybe_new(!quiet, …)` for spinners.
- Borrow checker: when iterating by index and mutating a `Vec`, clone `String` values early.

## Non-negotiables
- Minimal, focused change. No unrequested refactors, speculative abstractions, or backwards-compat shims.
- Default to no comments; add one only when the *why* is non-obvious.
- Never weaken/skip/delete tests, lint, or type checks to get green — fix the root cause.
- Never add agent attribution. Never commit/push/submit/merge — that's the release stage under human approval.

## Repair mode
Fix ONLY the specific FAIL items from `03_review.md` / `04_verify.md`. No scope expansion. Note in `02_impl.md` which FAIL each edit resolves.
