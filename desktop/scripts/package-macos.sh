#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

cargo build --release --bin st
npm ci --prefix desktop
npm run --prefix desktop check
npm run --prefix desktop test
npm run --prefix desktop build

rm -rf desktop/dist/Stax.app
(
  cd desktop
  npm exec -- native package \
    --target macos \
    --output dist/Stax.app \
    --signing none
)

mkdir -p desktop/dist/Stax.app/Contents/Resources/bin
cp target/release/st desktop/dist/Stax.app/Contents/Resources/bin/st
chmod 755 desktop/dist/Stax.app/Contents/Resources/bin/st
/usr/bin/codesign --force --deep --sign - desktop/dist/Stax.app

bash desktop/scripts/smoke-macos.sh
