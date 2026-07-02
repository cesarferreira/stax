# Contributing to stax

Thanks for your interest in contributing! Here's how to get started.

## Prerequisites

- **Rust** (stable toolchain) via [rustup](https://rustup.rs/)
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

The test suite is process/filesystem heavy (spawns `git` and `stax` subprocesses). On macOS, running tests inside Docker is significantly faster.

```bash
# Full test suite (preferred — uses Docker on macOS when available)
make test

# Full suite natively (slower on macOS)
make test-native

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

Before submitting a PR, make sure these pass:

```bash
# Format code
cargo fmt

# Lint (must pass with zero warnings)
cargo clippy -- -D warnings

# Type check
cargo check
```

CI enforces `cargo fmt --check` and `cargo clippy -- -D warnings` on every PR.

## Submitting Changes

1. Fork the repository and create your branch from `main`.
2. Make your changes and ensure tests pass.
3. Run `cargo fmt` and `cargo clippy -- -D warnings`.
4. Open a pull request with a clear description of the change.

## Project Structure

See the [documentation](https://cesarferreira.github.io/stax/) and the [concepts pages](docs/concepts/stacked-branches.md) for an overview of how stax models stacked branches, metadata, and the overall architecture.
