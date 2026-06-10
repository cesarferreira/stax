# AGENTS.md

## Test Command Policy

- **AI agents:** for full-suite validation always run `make test`. On macOS this routes through Docker, which is the only sane way to run the entire integration suite — `cargo test` natively will be slow, flaky, and may exhaust file handles. Default to `make test` and only fall back to native runners when explicitly told to.
- **Start Docker before running `make test`.** On macOS the Docker daemon is not always running; if `make test` fails with `failed to connect to the docker API at unix:///.../docker.sock`, ask the user to launch Docker Desktop (or run `open -a Docker`) and retry — do not silently fall back to `make test-native`.
- Do not run the full suite via `cargo test` in this repo.
- For full-suite validation, always use `make test`.
- On macOS, `make test` intentionally routes to the Docker fast path.
- Use native paths only when explicitly needed:
  - `make test-native`
  - `make test-local-ramdisk`
  - `make test-local-fast`
- Targeted single-test runs via `cargo nextest run <pattern>` are fine and encouraged for tight feedback loops; switch to `make test` once changes are ready for verification.
- All integration tests compile into a **single** binary (`tests/all_tests.rs`, with `autotests = false` in `Cargo.toml`) so cargo links one test binary instead of ~50 — this is what keeps test builds fast. Because of this, there is only one `[[test]]` target named `all_tests`: `cargo test --test status_tests` no longer works. To scope a run, filter by module path instead, e.g. `cargo nextest run status_tests::` (one former file) or `cargo nextest run status_tests::status_json_output` (one test). When adding a new `tests/*_tests.rs` file, register it with a `#[path = "..."] mod ...;` entry in `tests/all_tests.rs`.

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
