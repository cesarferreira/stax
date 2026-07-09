#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ENGINE_TARGET_DIR="$ROOT/target/desktop-engine"
ENGINE="$ENGINE_TARGET_DIR/release/st"
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
npm run --prefix desktop build
/usr/bin/file desktop/zig-out/bin/Stax | grep -q 'Mach-O 64-bit executable arm64'

rm -rf desktop/dist/Stax.app
(
  cd desktop
  npm exec -- native package \
    --target macos \
    --output dist/Stax.app \
    --signing none
)

mkdir -p desktop/dist/Stax.app/Contents/Resources/bin
cp "$ENGINE" desktop/dist/Stax.app/Contents/Resources/bin/st
chmod 755 desktop/dist/Stax.app/Contents/Resources/bin/st
/usr/bin/codesign --force --deep --sign - desktop/dist/Stax.app

bash desktop/scripts/smoke-macos.sh
