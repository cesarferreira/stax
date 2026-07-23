#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "install-gui tests are only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
installer="$repo_root/scripts/install-gui.sh"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

test -x "$installer" || chmod +x "$installer"

zip_app="$fixture/Stax.app"
mkdir -p "$zip_app/Contents/MacOS"
touch "$zip_app/Contents/MacOS/Stax"
chmod +x "$zip_app/Contents/MacOS/Stax"

archive="$fixture/Stax.zip"
(
  cd "$fixture"
  ditto -c -k --sequesterRsrc --keepParent Stax.app Stax.zip
)

install_root="$fixture/install"
mkdir -p "$install_root"

STAX_GUI_DOWNLOAD_URL="file://${archive}" \
STAX_GUI_INSTALL_DIR="$install_root" \
  "$installer" >"$fixture/install.stdout" 2>"$fixture/install.stderr"

test -d "$install_root/Stax.app"
test -x "$install_root/Stax.app/Contents/MacOS/Stax"
grep -q "installed Stax.app" "$fixture/install.stdout"

if STAX_GUI_DOWNLOAD_URL="file://${archive}" \
  STAX_GUI_INSTALL_DIR="$install_root" \
    "$installer" >"$fixture/reinstall.stdout" 2>&1
then
  test -d "$install_root/Stax.app"
else
  echo "Expected reinstall to succeed." >&2
  exit 1
fi

if STAX_GUI_DOWNLOAD_URL="file://${fixture}/missing.zip" \
  STAX_GUI_INSTALL_DIR="$install_root" \
    "$installer" >"$fixture/missing.stdout" 2>"$fixture/missing.stderr"
then
  echo "Expected missing archive to fail." >&2
  exit 1
fi
grep -q "download failed" "$fixture/missing.stderr"

broken_archive="$fixture/broken.zip"
printf 'not-a-zip' >"$broken_archive"
if STAX_GUI_DOWNLOAD_URL="file://${broken_archive}" \
  STAX_GUI_INSTALL_DIR="$install_root" \
    "$installer" >"$fixture/broken.stdout" 2>"$fixture/broken.stderr"
then
  echo "Expected broken archive to fail." >&2
  exit 1
fi
grep -Eq "failed to extract|did not contain Stax.app" "$fixture/broken.stderr"

empty_archive="$fixture/empty.zip"
(
  cd "$fixture"
  mkdir Empty.app
  ditto -c -k --sequesterRsrc --keepParent Empty.app empty.zip
)
if STAX_GUI_DOWNLOAD_URL="file://${empty_archive}" \
  STAX_GUI_INSTALL_DIR="$install_root" \
    "$installer" >"$fixture/empty.stdout" 2>"$fixture/empty.stderr"
then
  echo "Expected archive without Stax.app to fail." >&2
  exit 1
fi
grep -q "did not contain Stax.app" "$fixture/empty.stderr"

echo "install-gui tests passed."
