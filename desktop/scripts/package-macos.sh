#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENGINE_TARGET_DIR="$ROOT/target/desktop-engine"
ENGINE="$ENGINE_TARGET_DIR/release/st"
DESKTOP="$ROOT/desktop"
AUTOMATION_APP="$DESKTOP/dist/Stax.automation.app"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" || "$(uname -m)" != "arm64" ]]; then
  echo "Stax.app packaging currently requires Apple Silicon macOS." >&2
  exit 1
fi

CARGO_TARGET_DIR="$ENGINE_TARGET_DIR" cargo build --release --bin st
/usr/bin/file "$ENGINE" | grep -q 'Mach-O 64-bit executable arm64'
npm ci --prefix desktop
npm run --prefix desktop check
npm run --prefix desktop test

assemble_bundle() {
  local output="$1"
  local app="$DESKTOP/$output"
  rm -rf "$app"
  (
    cd "$DESKTOP"
    npm exec -- native package \
      --target macos \
      --output "$output" \
      --signing none
  )
  mkdir -p "$app/Contents/Resources/bin"
  cp "$ENGINE" "$app/Contents/Resources/bin/st"
  chmod 755 "$app/Contents/Resources/bin/st"
  /usr/bin/codesign --force --deep --sign - "$app"
}

trap 'rm -rf "$AUTOMATION_APP"' EXIT

npm run --prefix desktop build:automation
/usr/bin/file desktop/zig-out/bin/Stax | grep -q 'Mach-O 64-bit executable arm64'
assemble_bundle dist/Stax.automation.app
STAX_DESKTOP_APP="$AUTOMATION_APP" STAX_DESKTOP_SMOKE_MODE=automation \
  bash desktop/scripts/smoke-macos.sh

npm run --prefix desktop build
/usr/bin/file desktop/zig-out/bin/Stax | grep -q 'Mach-O 64-bit executable arm64'
assemble_bundle dist/Stax.app
bash desktop/scripts/smoke-macos.sh

rm -rf "$AUTOMATION_APP"
trap - EXIT
