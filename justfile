# Default target
default: check build test

# Build debug version
build:
    cargo build

# Build release version
release:
    cargo build --release

# Install to ~/.cargo/bin
install:
    cargo install --path . --locked --bins --debug

# Clean build artifacts
clean:
    cargo clean

# Run tests
test:
    cargo nextest run

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
