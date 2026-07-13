#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TARGET_DIR="$ROOT/target/desktop-engine"
ENGINE="$TARGET_DIR/debug/stax"
DESKTOP="$ROOT/desktop"
NATIVE_CLI="$DESKTOP/node_modules/.bin/native"

cd "$ROOT"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --bin stax

if [[ ! -x "$ENGINE" ]]; then
  echo "desktop dev: expected engine at $ENGINE" >&2
  exit 1
fi

if [[ ! -x "$NATIVE_CLI" ]]; then
  npm ci --prefix "$DESKTOP"
fi

cd "$DESKTOP"
exec env STAX_DESKTOP_ENGINE="$ENGINE" "$NATIVE_CLI" dev
