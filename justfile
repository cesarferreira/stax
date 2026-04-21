# Default target
default: check build test

MAC_LOCAL_TEST_THREADS := "8"

# Build debug version
build:
    cargo build

# Build release version
release:
    cargo build --release

# Install to ~/.cargo/bin
install:
    cargo install --path . --locked --bins --debug
    STAX_DISABLE_UPDATE_CHECK=1 "${CARGO_HOME:-$HOME/.cargo}/bin/stax" shell-setup --refresh

# Clean build artifacts
clean:
    cargo clean

# Run all tests
test:
    if [ "$(uname)" = "Darwin" ] && command -v docker >/dev/null 2>&1; then \
      just test-docker; \
    else \
      cargo nextest run; \
    fi

# Run all tests natively on host
test-native:
    if [ "$(uname)" = "Darwin" ]; then \
      just test-local-fast; \
    else \
      cargo nextest run; \
    fi

# Run tests with macOS-friendly defaults (custom temp root + capped concurrency)
test-local-fast:
    mkdir -p .test-tmp
    threads="${NEXTEST_TEST_THREADS:-}"; \
    if [ -z "$threads" ] && [ "$(uname)" = "Darwin" ]; then \
      threads="{{MAC_LOCAL_TEST_THREADS}}"; \
    fi; \
    if [ -z "$threads" ]; then \
      threads="num-cpus"; \
    fi; \
    env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="$(pwd)/.test-tmp" TMPDIR="$(pwd)/.test-tmp" NEXTEST_TEST_THREADS="$threads" cargo nextest run

# Create a RAM disk for fast local test temp dirs (macOS only)
ramdisk-up RAMDISK_NAME="STAXRAM" RAMDISK_SIZE_MB="2048":
    if [ "$(uname)" != "Darwin" ]; then \
      echo "ramdisk-up is only supported on macOS"; \
      exit 1; \
    fi
    if [ ! -d "/Volumes/{{RAMDISK_NAME}}" ]; then \
      echo "Creating RAM disk {{RAMDISK_NAME}} ({{RAMDISK_SIZE_MB}}MB)"; \
      disk=$(hdiutil attach -nomount ram://$(( {{RAMDISK_SIZE_MB}} * 2048 )) | awk 'NR==1{print $1}'); \
      diskutil erasevolume HFS+ "{{RAMDISK_NAME}}" "$disk" >/dev/null; \
    else \
      echo "RAM disk already mounted at /Volumes/{{RAMDISK_NAME}}"; \
    fi
    mkdir -p "/Volumes/{{RAMDISK_NAME}}/tmp"

# Detach the RAM disk (macOS only)
ramdisk-down RAMDISK_NAME="STAXRAM":
    if [ "$(uname)" != "Darwin" ]; then \
      echo "ramdisk-down is only supported on macOS"; \
      exit 1; \
    fi
    if [ -d "/Volumes/{{RAMDISK_NAME}}" ]; then \
      disk=$(diskutil info "/Volumes/{{RAMDISK_NAME}}" | awk -F': *' '/Device Node/{print $2; exit}'); \
      if [ -n "$disk" ]; then \
        echo "Detaching /Volumes/{{RAMDISK_NAME}} ($disk)"; \
        hdiutil detach "$disk" >/dev/null; \
      fi; \
    else \
      echo "RAM disk not mounted: /Volumes/{{RAMDISK_NAME}}"; \
    fi

# Run tests with temp repos on RAM disk (macOS only)
test-local-ramdisk RAMDISK_NAME="STAXRAM" RAMDISK_SIZE_MB="2048":
    just ramdisk-up {{RAMDISK_NAME}} {{RAMDISK_SIZE_MB}}
    threads="${NEXTEST_TEST_THREADS:-}"; \
    if [ -z "$threads" ] && [ "$(uname)" = "Darwin" ]; then \
      threads="{{MAC_LOCAL_TEST_THREADS}}"; \
    fi; \
    if [ -z "$threads" ]; then \
      threads="num-cpus"; \
    fi; \
    env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN STAX_DISABLE_UPDATE_CHECK=1 STAX_TEST_TMPDIR="/Volumes/{{RAMDISK_NAME}}/tmp" TMPDIR="/Volumes/{{RAMDISK_NAME}}/tmp" NEXTEST_TEST_THREADS="$threads" cargo nextest run

# Run tests in Linux Docker (fast path on macOS)
test-docker:
    mkdir -p .docker-cache/cargo .docker-cache/target
    docker run --rm -t \
      -u "$(id -u):$(id -g)" \
      -v "$(pwd):/work" \
      -w /work \
      -e CARGO_HOME=/work/.docker-cache/cargo \
      -v "$(pwd)/.docker-cache/target:/work/target" \
      rust:1.93 \
      bash -lc 'export PATH="$CARGO_HOME/bin:/usr/local/cargo/bin:$PATH"; if ! command -v cargo-nextest >/dev/null 2>&1; then cargo install cargo-nextest --locked; fi && cargo nextest run'

# Run fast unit tests only
test-unit:
    cargo nextest run --lib --bins

# Run integration tests only
test-integration:
    cargo nextest run --tests

# Run a single test
test-one name:
    cargo nextest run {{name}}

# Check + lint
check:
    cargo check
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Lint (check formatting + clippy)
lint:
    cargo fmt -- --check
    cargo clippy -- -D warnings

# Run with arguments
run *ARGS:
    cargo run -- {{ARGS}}

# Quick demo
demo: install
    @echo "=== stax demo ==="
    stax --help

# --- New targets ---

# Bump and release (patch/minor/major)
release-patch:
    cargo release patch --execute --no-confirm

release-minor:
    cargo release minor --execute --no-confirm

release-major:
    cargo release major --execute --no-confirm

# Dry-run release to see what would happen
release-dry level="patch":
    cargo release {{level}}

# Run clippy with auto-fix
fix:
    cargo clippy --fix --allow-dirty --allow-staged

# Check for outdated dependencies
outdated:
    cargo outdated -R

# Audit dependencies for security vulnerabilities
audit:
    cargo audit

# Show dependency tree
deps:
    cargo tree

# Generate and open docs
docs:
    cargo doc --open --no-deps

# Watch for changes and run tests
watch:
    cargo watch -x 'nextest run'

# Watch for changes and check
watch-check:
    cargo watch -x check -x 'clippy -- -D warnings'

# Full CI-like pipeline
ci: fmt lint test build

# Show binary size (release)
size: release
    ls -lh target/release/stax

# Install all dev tools needed
setup:
    cargo install cargo-nextest cargo-release cargo-outdated cargo-audit cargo-watch
