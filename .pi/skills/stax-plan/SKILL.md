---
name: stax-plan
description: Produce a file-level implementation plan for a stax Rust CLI change (new command, flag, bugfix, refactor, behavior change). Use before any implementation to decompose the work, name target files, define acceptance criteria and the test matrix, and flag docs that must change. Load when planning stax work, revising a plan after a repair loop, or scoping a feature.
---

# stax-plan — Implementation Planning for the stax CLI

A plan is only useful if it is grounded in the real codebase and leaves no discovery work for the implementer. Explore first, then plan.

## 1. Ground yourself in the code
- Prefer CodeGraph when `.codegraph/` exists: `codegraph explore "<symbols or question>"` returns verbatim source + call paths in one call. Otherwise `rg` + targeted reads.
- Confirm the exact files/functions/enums involved. Never plan against assumed locations.

## 2. Apply stax's structural patterns
- **New command:** add a variant to `Commands` in `src/cli.rs` → add dispatch → create `src/commands/<name>.rs` → register in `src/commands/mod.rs`. Commands not needing a repo (auth/config/doctor) dispatch before `ensure_initialized()`.
- **Transaction/undo support:** extend `OpKind` in `src/ops/receipt.rs`; wire `Transaction::begin()` + `snapshot()` + `finish_ok()/finish_err()`.
- **Restack-adjacent:** `cascade.rs` calls `restack::run()` directly — any signature change must update both.
- **Metadata:** per-branch JSON at `refs/branch-metadata/<branch>`; trunk at `refs/stax/trunk`. Preserve the freephite-compatible shape.
- **Async GitHub:** octocrab methods are async, wrapped with `rt.block_on()`.

## 3. Define the test matrix (repo Testing Policy)
Every non-trivial change plans tests for **happy path**, **error/bad path**, and **edge cases**. Integration tests live in `tests/<area>_tests.rs` and MUST be registered in `tests/all_tests.rs`. Prefer end-to-end tests that drive the `stax` binary in a temp repo for commands; unit tests for pure logic.

## 4. Flag documentation (repo Documentation Policy)
If behavior is user-visible, the plan lists which of `README.md`, `docs/`, `skills.md` must change. If none, state why.

## 5. Output
Write to `_workspace/<run_id>/01_plan.md` using:
```
# Plan: <goal>
## Non-goals
## Target files          (path — why)
## Steps                 (numbered; each names the file + change intent)
## Acceptance criteria
## Test matrix           (happy / error / edge — with file locations)
## Docs to update        (README / docs / skills.md, or "none because …")
## Risk & blast radius    (call out shared-code changes: engine/, git/repo.rs, ops/)
```

## Principles
- Root-cause over symptom; minimal focused scope; existing patterns over new abstractions.
- Surface ambiguity with a stated assumption + one alternative — don't silently guess.
