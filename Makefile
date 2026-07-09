.PHONY: build build-release release ensure-git-cliff install clean test test-native test-local-fast test-local-ramdisk test-image test-container-image test-docker test-container ramdisk-up ramdisk-down test-unit test-integration check fmt lint all

RAMDISK_NAME ?= STAXRAM
RAMDISK_SIZE_MB ?= 2048
RAMDISK_MOUNT ?= /Volumes/$(RAMDISK_NAME)
MAC_LOCAL_TEST_THREADS ?= 8
LEVEL ?= minor
GIT_CLIFF_VERSION ?= 2.13.1
TEST_IMAGE_HASH := $(shell git hash-object Dockerfile | cut -c1-12)
STAX_TEST_IMAGE ?= stax-test:$(TEST_IMAGE_HASH)
CONTAINER ?= $(shell if [ -x /opt/homebrew/opt/container/bin/container ]; then printf '%s\n' /opt/homebrew/opt/container/bin/container; else printf '%s\n' container; fi)
CONTAINER_MEMORY ?= 8G
CONTAINER_CARGO_BUILD_JOBS ?= 4
CONTAINER_CARGO_PROFILE ?= test-container
NATIVE_CARGO_PROFILE ?= test-container

# Default target
all: check build test

# Build debug version
build:
	cargo build

# Build release version
build-release:
	cargo build --release

# Publish a new release (usage: make release or make release LEVEL=patch)
release: ensure-git-cliff
	cargo release $(LEVEL) --execute --no-confirm

# Ensure git-cliff (changelog generator, used by cargo-release's pre-release hook)
# is available via cargo, so releasing does not depend on an OS-level install.
ensure-git-cliff:
	@command -v git-cliff >/dev/null 2>&1 || { \
		echo "git-cliff not found; installing v$(GIT_CLIFF_VERSION) via cargo..."; \
		cargo install git-cliff --version $(GIT_CLIFF_VERSION) --locked; \
	}

# Install to ~/.cargo/bin
install:
	CARGO_INCREMENTAL=0 cargo install --path . --locked --bins --debug
	STAX_DISABLE_UPDATE_CHECK=1 "$${CARGO_HOME:-$$HOME/.cargo}/bin/stax" shell-setup --refresh

# Clean build artifacts
clean:
	cargo clean

# Run all tests
test:
	@if [ "$$(uname)" = "Darwin" ] && command -v docker >/dev/null 2>&1; then \
		$(MAKE) test-docker; \
	else \
		$(MAKE) test-local-fast; \
	fi

# Run all tests natively on host
test-native:
	$(MAKE) test-local-fast

# Run tests with automation-friendly native defaults (custom temp root, token-free env, optimized profile)
test-local-fast:
	mkdir -p .test-tmp
	@threads="$${NEXTEST_TEST_THREADS:-}"; \
	if [ -z "$$threads" ] && [ "$$(uname)" = "Darwin" ]; then \
		threads="$(MAC_LOCAL_TEST_THREADS)"; \
	fi; \
	if [ -z "$$threads" ]; then \
		threads="num-cpus"; \
	fi; \
	env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$$(pwd)/.test-tmp" TMPDIR="$$(pwd)/.test-tmp" NEXTEST_TEST_THREADS="$$threads" RUST_MIN_STACK=4194304 cargo nextest run --cargo-profile "$(NATIVE_CARGO_PROFILE)"

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
	@threads="$${NEXTEST_TEST_THREADS:-}"; \
	if [ -z "$$threads" ] && [ "$$(uname)" = "Darwin" ]; then \
		threads="$(MAC_LOCAL_TEST_THREADS)"; \
	fi; \
	if [ -z "$$threads" ]; then \
		threads="num-cpus"; \
	fi; \
	env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$(RAMDISK_MOUNT)/tmp" TMPDIR="$(RAMDISK_MOUNT)/tmp" NEXTEST_TEST_THREADS="$$threads" cargo nextest run

# Build the pre-baked Linux test image (nextest + mold linker).
test-image:
	@docker image inspect $(STAX_TEST_IMAGE) >/dev/null 2>&1 || \
		docker build -t $(STAX_TEST_IMAGE) -f Dockerfile .

# Build the same image into Apple Container's local store.
test-container-image:
	@$(CONTAINER) image inspect $(STAX_TEST_IMAGE) >/dev/null 2>&1 || \
		$(CONTAINER) build -t $(STAX_TEST_IMAGE) -f Dockerfile .

# Shared container test env (Docker and Apple Container).
define CONTAINER_TEST_RUN
	@threads="$${NEXTEST_TEST_THREADS:-$(MAC_LOCAL_TEST_THREADS)}"; \
	$(1) run --rm -t \
		-u "$$(id -u):$$(id -g)" \
		$(2) \
		-v "$$(pwd):/work" \
		-w /work \
		-e CARGO_HOME=/work/$(3)/cargo \
		-e CARGO_INCREMENTAL=0 \
		-e CARGO_BUILD_JOBS="$(CONTAINER_CARGO_BUILD_JOBS)" \
		-e STAX_CONTAINER_CARGO_PROFILE="$(CONTAINER_CARGO_PROFILE)" \
		-e NEXTEST_TEST_THREADS="$$threads" \
		-e RUST_MIN_STACK=4194304 \
		-v "$$(pwd)/$(3)/target:/work/target" \
		$(STAX_TEST_IMAGE) \
		bash /work/scripts/container-nextest.sh $(4)
endef

# Run tests in Linux Docker (fast path on macOS)
test-docker: test-image
	mkdir -p .docker-cache/$(TEST_IMAGE_HASH)/cargo .docker-cache/$(TEST_IMAGE_HASH)/target
	$(call CONTAINER_TEST_RUN,docker,,.docker-cache/$(TEST_IMAGE_HASH),)

# Run tests with Apple container (experimental macOS fast path)
test-container: test-container-image
	mkdir -p .container-cache/$(TEST_IMAGE_HASH)/cargo .container-cache/$(TEST_IMAGE_HASH)/target
	$(call CONTAINER_TEST_RUN,$(CONTAINER),-m "$(CONTAINER_MEMORY)",.container-cache/$(TEST_IMAGE_HASH),)

# Run fast unit tests only
test-unit:
	cargo nextest run --lib --bins

# Run integration tests only
test-integration:
	cargo nextest run --tests

# Run clippy and check
check:
	cargo check --all-targets --all-features
	./scripts/lint.sh

# Format code
fmt:
	cargo fmt

# Lint (check formatting)
lint:
	cargo fmt -- --check
	./scripts/lint.sh

# Run with arguments (usage: make run ARGS="status")
run:
	cargo run -- $(ARGS)

# Quick demo
demo: install
	@echo "=== stax demo ==="
	stax --help
