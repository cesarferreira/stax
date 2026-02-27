.PHONY: build release install clean test test-unit test-integration check fmt lint all

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
