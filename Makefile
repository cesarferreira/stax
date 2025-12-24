.PHONY: build release install clean test check fmt lint all

# Default target
all: check build test

# Build debug version
build:
	cargo build

# Build release version
release:
	cargo build --release

# Install to ~/.cargo/bin
install: release
	cargo install --path .

# Clean build artifacts
clean:
	cargo clean

# Run tests
test:
	cargo nextest run

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
