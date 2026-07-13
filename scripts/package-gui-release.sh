#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "Stax GUI release packaging is only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
validate_only=false
target=""
version=""
build_number=""
output_dir="$repo_root/target/gui-release"

fail() {
  echo "$1" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --validate-environment)
      validate_only=true
      shift
      ;;
    --target|--version|--build-number|--output-dir)
      option="$1"
      if [ "$#" -lt 2 ]; then
        fail "Missing value for $option"
      fi
      case "$option" in
        --target) target="$2" ;;
        --version) version="$2" ;;
        --build-number) build_number="$2" ;;
        --output-dir) output_dir="$2" ;;
      esac
      shift 2
      ;;
    *)
      fail "Unknown option: $1"
      ;;
  esac
done

case "$target" in
  aarch64-apple-darwin) expected_arch=arm64 ;;
  x86_64-apple-darwin) expected_arch=x86_64 ;;
  *) fail "unsupported GUI release target: $target" ;;
esac

signing_identity="${STAX_GUI_SIGNING_IDENTITY:-}"
apple_id="${APPLE_ID:-}"
team_id="${APPLE_TEAM_ID:-}"
app_password="${APPLE_APP_PASSWORD:-}"
notary_values=0
for value in "$apple_id" "$team_id" "$app_password"; do
  if [ -n "$value" ]; then
    notary_values=$((notary_values + 1))
  fi
done

if [ "$notary_values" -ne 0 ] && [ "$notary_values" -ne 3 ]; then
  fail "notarization requires APPLE_ID, APPLE_TEAM_ID, and APPLE_APP_PASSWORD together"
fi
if [ "$notary_values" -eq 3 ] && [ -z "$signing_identity" ]; then
  fail "notarization requires STAX_GUI_SIGNING_IDENTITY"
fi

release_mode=unsigned
if [ -n "$signing_identity" ]; then
  release_mode=signed
fi
if [ "$notary_values" -eq 3 ]; then
  release_mode=notarized
fi

if [ "$validate_only" = true ]; then
  echo "$release_mode"
  exit 0
fi

if [ -z "$version" ]; then
  package_id="$(cd "$repo_root" && cargo pkgid -p stax-gui)"
  version="${package_id##*#}"
fi
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  fail "Invalid GUI release version: $version"
fi

if [ -z "$build_number" ]; then
  version_core="${version%%-*}"
  version_core="${version_core%%+*}"
  IFS=. read -r major minor patch <<<"$version_core"
  build_number="$((10#$major * 1000000 + 10#$minor * 1000 + 10#$patch))"
fi
if [[ ! "$build_number" =~ ^[0-9]+$ ]]; then
  fail "Invalid GUI release build number: $build_number"
fi

max_archive_bytes="${STAX_GUI_MAX_ARCHIVE_BYTES:-83886080}"
if [[ ! "$max_archive_bytes" =~ ^[0-9]+$ ]] || [ "$max_archive_bytes" -eq 0 ]; then
  fail "STAX_GUI_MAX_ARCHIVE_BYTES must be a positive integer"
fi

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
case "$target_dir" in
  /*) ;;
  *) target_dir="$repo_root/$target_dir" ;;
esac

(cd "$repo_root" && cargo build -p stax-gui --release --locked --target "$target")
binary="$target_dir/$target/release/stax-gui"
if [ ! -x "$binary" ]; then
  fail "Missing release GUI binary: $binary"
fi

binary_archs="$(lipo -archs "$binary")"
if [ "$binary_archs" != "$expected_arch" ]; then
  fail "Expected $expected_arch GUI binary, got: $binary_archs"
fi

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
app="$fixture/Stax.app"
STAX_GUI_BINARY="$binary" \
STAX_GUI_OUTPUT="$app" \
STAX_GUI_VERSION="$version" \
STAX_GUI_BUILD_NUMBER="$build_number" \
  "$repo_root/scripts/build-gui-app.sh"

if [ -n "$signing_identity" ]; then
  codesign --force --deep --options runtime --timestamp \
    --sign "$signing_identity" "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
fi

archive="$fixture/Stax-$target.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"

if [ "$notary_values" -eq 3 ]; then
  xcrun notarytool submit "$archive" \
    --apple-id "$apple_id" \
    --team-id "$team_id" \
    --password "$app_password" \
    --wait
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  rm -f "$archive"
  ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
fi

extracted="$fixture/extracted"
mkdir -p "$extracted"
ditto -x -k "$archive" "$extracted"
app_count="$(find "$extracted" -maxdepth 1 -type d -name '*.app' | wc -l | tr -d ' ')"
if [ "$app_count" != "1" ]; then
  fail "Expected exactly one app in release archive, found $app_count"
fi

extracted_app="$extracted/Stax.app"
plist="$extracted_app/Contents/Info.plist"
executable="$extracted_app/Contents/MacOS/Stax"
buddy=/usr/libexec/PlistBuddy
actual_bundle_id="$("$buddy" -c 'Print :CFBundleIdentifier' "$plist")"
actual_version="$("$buddy" -c 'Print :CFBundleShortVersionString' "$plist")"
actual_archs="$(lipo -archs "$executable")"
actual_version_output="$($executable --version)"

[ "$actual_bundle_id" = "com.cesarferreira.stax" ] || \
  fail "Unexpected GUI bundle identifier: $actual_bundle_id"
[ "$actual_version" = "$version" ] || \
  fail "Unexpected GUI bundle version: $actual_version"
[ "$actual_archs" = "$expected_arch" ] || \
  fail "Unexpected archived GUI architecture: $actual_archs"
[ "$actual_version_output" = "stax-gui $version" ] || \
  fail "Unexpected archived GUI version output: $actual_version_output"

archive_bytes="$(stat -f%z "$archive")"
executable_bytes="$(stat -f%z "$executable")"
if [ "$archive_bytes" -gt "$max_archive_bytes" ]; then
  fail "GUI archive is $archive_bytes bytes; limit is $max_archive_bytes bytes"
fi

mkdir -p "$output_dir"
final_archive="$output_dir/Stax-$target.zip"
temporary_archive="$output_dir/.Stax-$target.zip.$$"
cp "$archive" "$temporary_archive"
mv -f "$temporary_archive" "$final_archive"

echo "Packaged $release_mode Stax app for $target"
echo "Executable bytes: $executable_bytes"
echo "Archive bytes: $archive_bytes"
echo "Artifact: $final_archive"
