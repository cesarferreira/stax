#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "Stax.app assembly is only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle_id="com.cesarferreira.stax"
executable_name="Stax"
output="${STAX_GUI_OUTPUT:-$repo_root/target/gui/Stax.app}"
template="$repo_root/crates/stax-gui/resources/Info.plist.in"
icon="${STAX_GUI_ICON:-$repo_root/crates/stax-gui/resources/AppIcon.icns}"

install=false
if [ "$#" -eq 1 ] && [ "$1" = "--install" ]; then
  install=true
elif [ "$#" -gt 0 ]; then
  echo "Usage: $0 [--install]" >&2
  exit 1
fi

if [ ! -f "$template" ]; then
  echo "Missing Info.plist template: $template" >&2
  exit 1
fi

if [ ! -f "$icon" ]; then
  echo "Missing app icon: $icon" >&2
  exit 1
fi

if [ -n "${STAX_GUI_VERSION:-}" ]; then
  version="$STAX_GUI_VERSION"
else
  package_id="$(cd "$repo_root" && cargo pkgid -p stax-gui)"
  version="${package_id##*#}"
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "Invalid STAX_GUI_VERSION: $version" >&2
  exit 1
fi

if [ -n "${STAX_GUI_BUILD_NUMBER:-}" ]; then
  build_number="$STAX_GUI_BUILD_NUMBER"
else
  version_core="${version%%-*}"
  version_core="${version_core%%+*}"
  IFS=. read -r major minor patch <<<"$version_core"
  build_number="$((10#$major * 1000000 + 10#$minor * 1000 + 10#$patch))"
fi

if [[ ! "$build_number" =~ ^[0-9]+$ ]]; then
  echo "Invalid STAX_GUI_BUILD_NUMBER: $build_number" >&2
  exit 1
fi

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

mkdir -p "$(dirname "$output")"
staging="${output}.tmp.$$"
rm -rf "$staging"
cleanup() {
  if [ -n "$staging" ]; then
    rm -rf "$staging"
  fi
}
trap cleanup EXIT

contents="$staging/Contents"
macos="$contents/MacOS"
resources="$contents/Resources"
mkdir -p "$macos" "$resources"
cp "$binary" "$macos/$executable_name"
chmod +x "$macos/$executable_name"
cp "$icon" "$resources/AppIcon.icns"

sed \
  -e "s|@EXECUTABLE@|$executable_name|g" \
  -e "s|@BUNDLE_ID@|$bundle_id|g" \
  -e "s|@VERSION@|$version|g" \
  -e "s|@BUILD_NUMBER@|$build_number|g" \
  "$template" >"$contents/Info.plist"

/usr/bin/plutil -lint "$contents/Info.plist" >/dev/null

rm -rf "$output"
mv "$staging" "$output"
staging=""

echo "Assembled unsigned Stax.app at $output"

if [ "$install" = true ]; then
  install_root="$HOME/Applications"
  installed_app="$install_root/Stax.app"
  mkdir -p "$install_root"
  rm -rf "$installed_app"
  cp -R "$output" "$installed_app"
  lsregister="${STAX_GUI_LSREGISTER:-/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister}"
  "$lsregister" -f "$installed_app"
  echo "Installed unsigned Stax.app at $installed_app"
fi
