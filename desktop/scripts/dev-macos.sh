#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET_DIR="$ROOT/target/desktop-engine"
ENGINE="$TARGET_DIR/debug/st"

cd "$ROOT"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --bin st

if [[ ! -x "$ENGINE" ]]; then
  echo "desktop dev: expected engine at $ENGINE" >&2
  exit 1
fi

cd "$ROOT/desktop"
exec env STAX_DESKTOP_ENGINE="$ENGINE" native dev
