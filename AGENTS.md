# AGENTS.md

## Test Command Policy

- Do not run the full suite via `cargo test` in this repo.
- For full-suite validation, always use `make test` or `just test`.
- On macOS, `make test`/`just test` intentionally routes to the Docker fast path.
- Use native paths only when explicitly needed:
  - `make test-native` / `just test-native`
  - `make test-local-ramdisk` / `just test-local-ramdisk`
  - `make test-local-fast` / `just test-local-fast`

## Why

- This suite is process/filesystem heavy (`git` + `stax` subprocesses), and Linux Docker is dramatically faster and more stable than native macOS for full runs.

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
