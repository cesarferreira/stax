# Contributing to stax

Thanks for your interest in contributing! Here's how to get started.

## Prerequisites

- **Rust** via [rustup](https://rustup.rs/) or your system package manager. Local
  commands use the Rust toolchain installed on your machine; it must satisfy the
  `rust-version` declared in `Cargo.toml`.
- **cargo-nextest** for running tests: `cargo install cargo-nextest`
- **Docker** (optional, recommended on macOS for faster full test suite)

## Development Setup

```bash
# Clone the repo
git clone https://github.com/cesarferreira/stax.git
cd stax

# Build
cargo build

# Install locally (debug build)
cargo install --path .

# Run
cargo run -- <command>
```

## Running Tests

The test suite is process/filesystem heavy (spawns `git` and `stax`
subprocesses). `make test` uses Docker on macOS when available; otherwise it
runs native nextest with the optimized test profile, sanitized token env,
disabled update checks, and a repo-local temp directory. On macOS,
`make test-native` additionally checks and raises the file-descriptor limit
before starting. Endpoint-security tooling can make native macOS runs much
slower and more variable than Docker.

```bash
# Full test suite (preferred — uses Docker on macOS when available)
make test

# Guarded native fallback (timings vary on macOS)
make test-native

# Explicit optimized local nextest path used by native full-suite runs
make test-local-fast

# Rebuild the pre-baked Linux test image (after Dockerfile changes)
make test-image

# Experimental Apple Container path (builds into container's local image store)
make test-container

# Run a single test by name
cargo nextest run test_name

# Unit tests only
cargo nextest run --lib --bins

# Integration tests only
cargo nextest run --tests
```

**Important:** Do not use `cargo test` directly — always use `make test` for the full suite.

## Code Quality

Use the focused lint target while iterating:

```bash
make lint-fast
```

Before submitting a PR, make sure these pass:

```bash
# Format code
cargo fmt

# Check formatting and Clippy diagnostics
make lint

# Type check
cargo check
```

CI enforces `make lint` on every PR. This checks all targets and features,
validates formatting, and treats new Clippy diagnostics as errors while the
explicit legacy-lint allowlist in `scripts/clippy-lint.sh` is paid down.

## Submitting Changes

1. Fork the repository and create your branch from `main`.
2. Make your changes and ensure tests pass.
3. Run `cargo fmt` and `make lint`.
4. Open a pull request with a clear description of the change.

## Project Structure

See the [documentation](https://cesarferreira.github.io/stax/) and the [concepts pages](docs/concepts/stacked-branches.md) for an overview of how stax models stacked branches, metadata, and the overall architecture.
