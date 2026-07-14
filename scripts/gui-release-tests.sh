#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "GUI release packaging tests are only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
packager="$repo_root/scripts/package-gui-release.sh"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

grep -Fq 'stax = { path = "../..", features = ["vendored-openssl"] }' "$repo_root/crates/stax-gui/Cargo.toml"

case "$(uname -m)" in
  arm64) target=aarch64-apple-darwin ;;
  x86_64) target=x86_64-apple-darwin ;;
  *) echo "Unsupported macOS test architecture: $(uname -m)" >&2; exit 1 ;;
esac

run_validation() {
  env \
    -u STAX_GUI_SIGNING_IDENTITY \
    -u APPLE_ID \
    -u APPLE_TEAM_ID \
    -u APPLE_APP_PASSWORD \
    "$@" \
    "$packager" --validate-environment --target "$target"
}

expect_validation() {
  expected="$1"
  shift
  actual="$(run_validation "$@")"
  test "$actual" = "$expected"
}

expect_validation unsigned
expect_validation signed STAX_GUI_SIGNING_IDENTITY="Developer ID Application: Test"
expect_validation notarized \
  STAX_GUI_SIGNING_IDENTITY="Developer ID Application: Test" \
  APPLE_ID="release@example.com" \
  APPLE_TEAM_ID="TEAM123456" \
  APPLE_APP_PASSWORD="app-password"

for partial in \
  "APPLE_ID=release@example.com" \
  "APPLE_ID=release@example.com APPLE_TEAM_ID=TEAM123456" \
  "APPLE_ID=release@example.com APPLE_TEAM_ID=TEAM123456 APPLE_APP_PASSWORD=app-password"
do
  read -r -a variables <<<"$partial"
  if run_validation "${variables[@]}" \
    >"$fixture/partial.stdout" 2>"$fixture/partial.stderr"
  then
    echo "Expected partial notarization configuration to fail: $partial" >&2
    exit 1
  fi
  grep -q "notarization" "$fixture/partial.stderr"
done

if "$packager" --validate-environment --target mips64-apple-darwin \
  >"$fixture/target.stdout" 2>"$fixture/target.stderr"
then
  echo "Expected unsupported GUI release target to fail." >&2
  exit 1
fi
grep -q "unsupported GUI release target" "$fixture/target.stderr"

version="$(cd "$repo_root" && cargo pkgid -p stax-gui | sed 's/.*#//')"
output_dir="$fixture/output"
env \
  -u STAX_GUI_SIGNING_IDENTITY \
  -u APPLE_ID \
  -u APPLE_TEAM_ID \
  -u APPLE_APP_PASSWORD \
  STAX_GUI_MAX_ARCHIVE_BYTES=83886080 \
  "$packager" \
    --target "$target" \
    --version "$version" \
    --build-number 999 \
    --output-dir "$output_dir"

archive="$output_dir/Stax-$target.zip"
test -f "$archive"
unzip -l "$archive" >"$fixture/archive-list.txt"
grep -q 'Stax.app/Contents/MacOS/Stax' "$fixture/archive-list.txt"
grep -q 'Stax.app/Contents/Resources/AppIcon.icns' "$fixture/archive-list.txt"
