.PHONY: build release install clean test test-local-fast test-docker test-unit test-integration check fmt lint all

# Default target
all: check build test

# Build debug version
build:
	cargo build

# Build release version
release:
	cargo build --release

# Install to ~/.cargo/bin
install:
	cargo install --path . --bin stax --bin st --force

# Clean build artifacts
clean:
	cargo clean

# Run all tests
test:
	cargo nextest run

# Run tests with macOS-friendly defaults (custom temp root + lower concurrency)
test-local-fast:
	mkdir -p .test-tmp
	env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$$(pwd)/.test-tmp" TMPDIR="$$(pwd)/.test-tmp" cargo nextest run

# Run tests in Linux Docker (fast path on macOS)
test-docker:
	mkdir -p .docker-cache/cargo .docker-cache/target
	docker run --rm -t \
		-u "$$(id -u):$$(id -g)" \
		-v "$$(pwd):/work" \
		-w /work \
		-e CARGO_HOME=/work/.docker-cache/cargo \
		-v "$$(pwd)/.docker-cache/target:/work/target" \
		rust:1.93 \
		bash -lc 'export PATH="$$CARGO_HOME/bin:/usr/local/cargo/bin:$$PATH"; if ! command -v cargo-nextest >/dev/null 2>&1; then cargo install cargo-nextest --locked; fi && cargo nextest run'

# Run fast unit tests only
test-unit:
	cargo nextest run --lib --bins

# Run integration tests only
test-integration:
	cargo nextest run --tests

# Run clippy and check
check:
	cargo check
	cargo clippy -- -D warnings

# Format code
fmt:
	cargo fmt

# Lint (check formatting)
lint:
	cargo fmt -- --check
	cargo clippy -- -D warnings

# Run with arguments (usage: make run ARGS="status")
run:
	cargo run -- $(ARGS)

# Quick demo
demo: install
	@echo "=== stax demo ==="
	stax --help
