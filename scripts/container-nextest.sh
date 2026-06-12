#!/usr/bin/env bash
# Shared entrypoint for Linux container test runs (Docker and Apple Container).
set -euo pipefail

export PATH="${CARGO_HOME:-/usr/local/cargo}/bin:/usr/local/cargo/bin:${PATH}"

ensure_nextest() {
	if command -v cargo-nextest >/dev/null 2>&1; then
		return 0
	fi
	mkdir -p "${CARGO_HOME}/bin"
	case "$(uname -m)" in
	aarch64 | arm64) nextest_platform=linux-arm ;;
	x86_64 | amd64) nextest_platform=linux ;;
	*)
		echo "unsupported container architecture: $(uname -m)" >&2
		exit 1
		;;
	esac
	curl -LsSf "https://get.nexte.st/latest/${nextest_platform}" | tar zxf - -C "${CARGO_HOME}/bin"
}

ensure_nextest

profile="${STAX_CONTAINER_CARGO_PROFILE:-test-container}"

exec env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN \
	STAX_DISABLE_UPDATE_CHECK=1 \
	cargo nextest run --cargo-profile "${profile}" "$@"
