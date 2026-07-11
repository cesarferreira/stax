# Action-First Sync Summary Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Enrich `st sync` with a zero-extra-I/O change summary and conditional next-action guidance.

**Architecture:** Extend the existing `SyncStats` accumulator with structured outcomes gathered during the current sync phases. Keep formatting in pure helpers so output behavior and next-command priority are unit-testable, then populate the accumulator without adding Git or network operations.

**Tech Stack:** Rust, git2, existing Stax integration harness, colored terminal output.

## Global Constraints

- Add no network calls or Git subprocesses to the default reporting path.
- Preserve `--quiet` behavior and existing detailed warnings.
- Omit zero-value footer segments and print at most one contextual `Next:` command.
- Update `docs/commands/reference.md` and `skills.md`; leave the README command table unchanged because command semantics and flags do not change.

---

### Task 1: Preserve and render existing sync facts

**Files:**
- Modify: `src/commands/sync.rs`

**Interfaces:**
- Produces: `parse_diff_shortstat(&str) -> Option<(usize, usize, usize)>` returning files, additions, and deletions.
- Produces: extended `SyncStats` footer fields for imported updates and checkout changes.

- [ ] **Step 1: Write failing unit tests**

Add assertions that shortstat parsing preserves `752`, that the footer contains
`752 files`, and that updated imported branches appear only when non-zero.

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cargo nextest run --lib commands::sync::tests::parses_diff_shortstat_line_counts commands::sync::tests::render_sync_footer_is_colored_and_compact`

Expected: FAIL because the parser still returns only additions/deletions and the
footer has no file/imported fields.

- [ ] **Step 3: Implement the minimal data changes**

Extend `TrunkSummary::Pulled` with `files: usize`, preserve the first shortstat
number, add `imported_branches_updated: usize` to `SyncStats`, and render both
segments with correct singular/plural copy.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run: `cargo nextest run --lib commands::sync::tests::`

Expected: all sync unit tests pass.

### Task 2: Render conditional attention and one next command

**Files:**
- Modify: `src/commands/sync.rs`

**Interfaces:**
- Produces: `CleanupSkip { branch: String, reason: String }`.
- Produces: `render_sync_follow_up(&SyncStats) -> Vec<String>`.
- Consumes: final trunk reachability, current/final checkout, and cleanup outcomes.

- [ ] **Step 1: Write failing unit tests**

Cover skipped-cleanup reasons, checkout changes, an out-of-sync trunk, clean
output with no follow-up, and next-command priority: trunk recovery before
`st sweep`.

- [ ] **Step 2: Run the focused tests and verify RED**

Run: `cargo nextest run --lib commands::sync::tests::render_sync_follow_up`

Expected: FAIL because the follow-up renderer does not exist.

- [ ] **Step 3: Implement structured collection and rendering**

Accumulate cleanup skip reasons at existing `continue`/blocked paths, compare
the initial and final checkout, mark trunk reachability using the already
resolved fetched revision, and print the pure renderer's lines after the footer
when not quiet.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run: `cargo nextest run --lib commands::sync::tests::`

Expected: all sync unit tests pass.

### Task 3: Verify the CLI behavior end to end

**Files:**
- Modify: `tests/integration_tests.rs`

**Interfaces:**
- Consumes: the public `st sync` command output.
- Produces: regression coverage for file-count output and the absence of ambient restack guidance.

- [ ] **Step 1: Add failing integration assertions**

Extend the existing trunk-update sync test to assert a `file` count in the
completion footer. Add or extend a stacked-branch sync test to assert that
routine restack state does not produce a warning or `Next:` command when
`--restack` is absent.

- [ ] **Step 2: Run targeted integration tests and verify RED/GREEN behavior**

Run each exact module path with `cargo nextest run integration_tests::<test_name>`.

Expected: each assertion fails before its corresponding implementation and
passes afterward.

- [ ] **Step 3: Run the sync-focused integration module filters**

Run: `cargo nextest run 'integration_tests::test_sync'`

Expected: all selected sync integration tests pass.

### Task 4: Document and verify the user-visible output

**Files:**
- Modify: `docs/commands/reference.md`
- Modify: `skills.md`

**Interfaces:**
- Produces: user and agent guidance matching the implemented default output.

- [ ] **Step 1: Update documentation**

Describe the enriched completion footer, conditional attention lines, and
single next-command priority without documenting internal implementation.

- [ ] **Step 2: Run formatting and targeted validation**

Run: `cargo fmt --check`

Run: `cargo nextest run --lib commands::sync::tests::`

Expected: formatting is clean and all sync unit tests pass.

- [ ] **Step 3: Run full-suite validation**

Run: `make test`

Expected: the Docker-backed full suite passes with zero failures.

- [ ] **Step 4: Review the final diff and publish with Stax**

Run: `git diff --check && git status --short && st ss --draft --no-template`

Expected: no whitespace errors, only scoped files are changed, and Stax creates
or updates the draft PR for `cesar/improve-sync-summary`.
