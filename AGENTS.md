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
