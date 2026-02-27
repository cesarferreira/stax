.PHONY: build release install clean test test-native test-local-fast test-local-ramdisk test-docker ramdisk-up ramdisk-down test-unit test-integration check fmt lint all

RAMDISK_NAME ?= STAXRAM
RAMDISK_SIZE_MB ?= 2048
RAMDISK_MOUNT ?= /Volumes/$(RAMDISK_NAME)

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
	@if [ "$$(uname)" = "Darwin" ] && command -v docker >/dev/null 2>&1; then \
		$$(MAKE) test-docker; \
	else \
		cargo nextest run; \
	fi

# Run all tests natively on host
test-native:
	cargo nextest run

# Run tests with macOS-friendly defaults (custom temp root + lower concurrency)
test-local-fast:
	mkdir -p .test-tmp
	env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$$(pwd)/.test-tmp" TMPDIR="$$(pwd)/.test-tmp" cargo nextest run

# Create a RAM disk for fast local test temp dirs (macOS only)
ramdisk-up:
	@if [ "$$(uname)" != "Darwin" ]; then \
		echo "ramdisk-up is only supported on macOS"; \
		exit 1; \
	fi
	@if [ ! -d "$(RAMDISK_MOUNT)" ]; then \
		echo "Creating RAM disk $(RAMDISK_NAME) ($(RAMDISK_SIZE_MB)MB)"; \
		disk=$$(hdiutil attach -nomount ram://$$(( $(RAMDISK_SIZE_MB) * 2048 )) | awk 'NR==1{print $$1}'); \
		diskutil erasevolume HFS+ "$(RAMDISK_NAME)" "$$disk" >/dev/null; \
	else \
		echo "RAM disk already mounted at $(RAMDISK_MOUNT)"; \
	fi
	@mkdir -p "$(RAMDISK_MOUNT)/tmp"

# Detach the RAM disk (macOS only)
ramdisk-down:
	@if [ "$$(uname)" != "Darwin" ]; then \
		echo "ramdisk-down is only supported on macOS"; \
		exit 1; \
	fi
	@if [ -d "$(RAMDISK_MOUNT)" ]; then \
		disk=$$(diskutil info "$(RAMDISK_MOUNT)" | awk -F': *' '/Device Node/{print $$2; exit}'); \
		if [ -n "$$disk" ]; then \
			echo "Detaching $(RAMDISK_MOUNT) ($$disk)"; \
			hdiutil detach "$$disk" >/dev/null; \
		fi; \
	else \
		echo "RAM disk not mounted: $(RAMDISK_MOUNT)"; \
	fi

# Run tests with temp repos on RAM disk (macOS only)
test-local-ramdisk: ramdisk-up
	env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$(RAMDISK_MOUNT)/tmp" TMPDIR="$(RAMDISK_MOUNT)/tmp" cargo nextest run

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
