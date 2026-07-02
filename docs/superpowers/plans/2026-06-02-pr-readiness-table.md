# PR Readiness Table Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the read-only `st pr list --ready` / `st ready` PR readiness table described in `docs/superpowers/specs/2026-06-02-pr-readiness-table-design.md`.

**Architecture:** Add a focused `src/commands/ready.rs` module that owns readiness row data, action classification, scope collection, live forge fetching, JSON output, and table rendering. Keep `src/commands/pr.rs` responsible for existing repo PR listing, and only route `--ready` to the new module. Wire CLI aliases through `src/cli/args.rs` and `src/cli/mod.rs`.

**Tech Stack:** Rust, clap, serde, tokio, existing `ForgeClient`, existing `github_list` table helpers, existing stax integration test harness.

---

### Task 1: Readiness Core

**Files:**
- Create: `src/commands/ready.rs`
- Modify: `src/commands/mod.rs`

- [x] Write failing unit tests for action classification, CI summaries, and action sorting in `src/commands/ready.rs`.
- [x] Run `cargo test ready::tests --lib` and confirm failures are from the missing module/API.
- [x] Implement `ReadyAction`, `ReadyReason`, `PrReadinessRow`, classification, CI/review summary helpers, and sorting.
- [x] Run `cargo test ready::tests --lib` and confirm the tests pass.

### Task 2: CLI Routing

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/commands/pr.rs`

- [x] Add failing parser tests or CLI tests proving `st ready --help` exists, `st pr list --ready --help` exposes `--all`, and plain `st pr list` remains a repo list command.
- [x] Run the targeted CLI tests and confirm they fail before implementation.
- [x] Add `Commands::Ready { all, json }`, extend `PrCommands::List` with `ready` and ready-only `all`, and route both entrypoints to `commands::ready::run`.
- [x] Run the targeted CLI tests and confirm they pass.

### Task 3: Live Readiness Fetch And Render

**Files:**
- Modify: `src/commands/ready.rs`

- [x] Add tests for render-row/table helper behavior that assert column titles and action labels appear.
- [x] Run `cargo test ready::tests --lib` and confirm failures before implementation.
- [x] Implement scope collection, PR number resolution with branch lookup fallback, live `get_pr_merge_status`, detailed check fetch, JSON output, and responsive table rendering.
- [x] Run `cargo test ready::tests --lib` and confirm tests pass.

### Task 4: Documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/commands/core.md` or `docs/commands/reference.md`
- Modify: `skills.md`

- [x] Add user-facing command docs for `st ready` and `st pr list --ready`.
- [x] Run targeted checks that compile docs-sensitive CLI tests where practical.

### Task 5: Verification And Submit

**Files:**
- All changed files.

- [x] Run targeted tests: readiness unit tests and relevant CLI parser tests.
- [x] Run `cargo fmt`.
- [x] Run `make test` for full-suite validation, per repository policy. Docker was unavailable, so native targeted checks and `cargo check --all-targets` were run instead.
- [ ] Commit implementation and docs.
- [ ] Run `st submit --draft --yes --no-prompt --no-template` to update PR #447.
