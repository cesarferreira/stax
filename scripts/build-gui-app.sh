#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "Stax.app assembly is only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle_id="dev.stax.Stax"
executable_name="Stax"
output="${STAX_GUI_OUTPUT:-$repo_root/target/gui/Stax.app}"
template="$repo_root/crates/stax-gui/resources/Info.plist.in"

if [ -n "${STAX_GUI_BINARY:-}" ]; then
  binary="$STAX_GUI_BINARY"
else
  cargo build -p stax-gui
  target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
  binary="$target_dir/debug/stax-gui"
fi

if [ ! -x "$binary" ]; then
  echo "GUI binary is not executable: $binary" >&2
  exit 1
fi

if [ ! -f "$template" ]; then
  echo "Missing Info.plist template: $template" >&2
  exit 1
fi

contents="$output/Contents"
macos="$contents/MacOS"
rm -rf "$output"
mkdir -p "$macos"
cp "$binary" "$macos/$executable_name"
chmod +x "$macos/$executable_name"

sed \
  -e "s|@EXECUTABLE@|$executable_name|g" \
  -e "s|@BUNDLE_ID@|$bundle_id|g" \
  "$template" >"$contents/Info.plist"

/usr/bin/plutil -lint "$contents/Info.plist" >/dev/null

echo "Assembled unsigned developer-preview Stax.app at $output"

if [ "${1:-}" = "--install" ]; then
  install_root="$HOME/Applications"
  installed_app="$install_root/Stax.app"
  mkdir -p "$install_root"
  rm -rf "$installed_app"
  cp -R "$output" "$installed_app"
  lsregister="${STAX_GUI_LSREGISTER:-/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister}"
  "$lsregister" -f "$installed_app"
  echo "Installed unsigned developer-preview Stax.app at $installed_app"
elif [ "$#" -gt 0 ]; then
  echo "Usage: $0 [--install]" >&2
  exit 1
fi
