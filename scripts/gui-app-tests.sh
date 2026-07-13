#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

source_icon="$repo_root/crates/stax-gui/resources/AppIcon-1024.png"
icon="$repo_root/crates/stax-gui/resources/AppIcon.icns"
test "$(sips -g pixelWidth "$source_icon" | awk '/pixelWidth/{print $2}')" = "1024"
test "$(sips -g pixelHeight "$source_icon" | awk '/pixelHeight/{print $2}')" = "1024"
iconutil --convert iconset --output "$fixture/roundtrip.iconset" "$icon"
for representation in \
  icon_16x16.png \
  icon_16x16@2x.png \
  icon_32x32.png \
  icon_32x32@2x.png \
  icon_128x128.png \
  icon_128x128@2x.png \
  icon_256x256.png \
  icon_256x256@2x.png \
  icon_512x512.png \
  icon_512x512@2x.png
do
  test -f "$fixture/roundtrip.iconset/$representation"
done

custom_icon="$fixture/CustomAppIcon.icns"
"$repo_root/scripts/build-gui-icon.sh" "$source_icon" "$custom_icon" >/dev/null
test -f "$custom_icon"

jpeg_source="$fixture/AppIcon-1024.jpg"
sips -s format jpeg "$source_icon" --out "$jpeg_source" >/dev/null
if "$repo_root/scripts/build-gui-icon.sh" "$jpeg_source" "$fixture/jpeg.icns" \
  >"$fixture/jpeg.stdout" 2>"$fixture/jpeg.stderr"
then
  echo "Expected JPEG icon source to be rejected." >&2
  exit 1
fi
grep -q "PNG" "$fixture/jpeg.stderr"

if "$repo_root/scripts/build-gui-icon.sh" "$fixture/missing.png" "$fixture/missing.icns" \
  >"$fixture/missing.stdout" 2>"$fixture/missing.stderr"
then
  echo "Expected missing icon source to be rejected." >&2
  exit 1
fi
grep -q "Missing icon source" "$fixture/missing.stderr"

binary="$fixture/stax-gui-fixture"
app="$fixture/Stax.app"

cat >"$binary" <<'SCRIPT'
#!/usr/bin/env bash
exit 0
SCRIPT
chmod +x "$binary"

STAX_GUI_BINARY="$binary" \
STAX_GUI_OUTPUT="$app" \
STAX_GUI_VERSION="1.2.3" \
STAX_GUI_BUILD_NUMBER="456" \
  "$repo_root/scripts/build-gui-app.sh"

test -x "$app/Contents/MacOS/Stax"
/usr/bin/plutil -lint "$app/Contents/Info.plist" >/dev/null
buddy=/usr/libexec/PlistBuddy
plist="$app/Contents/Info.plist"
test "$("$buddy" -c 'Print :CFBundleIdentifier' "$plist")" = "com.cesarferreira.stax"
test "$("$buddy" -c 'Print :CFBundleExecutable' "$plist")" = "Stax"
test "$("$buddy" -c 'Print :CFBundleDisplayName' "$plist")" = "Stax"
test "$("$buddy" -c 'Print :CFBundlePackageType' "$plist")" = "APPL"
test "$("$buddy" -c 'Print :CFBundleShortVersionString' "$plist")" = "1.2.3"
test "$("$buddy" -c 'Print :CFBundleVersion' "$plist")" = "456"
test "$("$buddy" -c 'Print :CFBundleIconFile' "$plist")" = "AppIcon"
cmp "$icon" "$app/Contents/Resources/AppIcon.icns"

default_app="$fixture/default/Stax.app"
STAX_GUI_BINARY="$binary" \
STAX_GUI_OUTPUT="$default_app" \
  "$repo_root/scripts/build-gui-app.sh"
workspace_version="$(cargo pkgid -p stax-gui | sed 's/.*#//')"
test "$("$buddy" -c 'Print :CFBundleShortVersionString' "$default_app/Contents/Info.plist")" = "$workspace_version"

invalid_app="$fixture/invalid/Stax.app"
mkdir -p "$invalid_app"
touch "$invalid_app/existing-bundle"
if STAX_GUI_BINARY="$binary" \
  STAX_GUI_OUTPUT="$invalid_app" \
  STAX_GUI_VERSION='1.2.<bad>' \
  "$repo_root/scripts/build-gui-app.sh" \
  >"$fixture/invalid-version.stdout" 2>"$fixture/invalid-version.stderr"
then
  echo "Expected invalid GUI version to be rejected." >&2
  exit 1
fi
grep -q "Invalid STAX_GUI_VERSION" "$fixture/invalid-version.stderr"
test -f "$invalid_app/existing-bundle"

if STAX_GUI_BINARY="$binary" \
  STAX_GUI_OUTPUT="$fixture/invalid-build/Stax.app" \
  STAX_GUI_BUILD_NUMBER='abc' \
  "$repo_root/scripts/build-gui-app.sh" \
  >"$fixture/invalid-build.stdout" 2>"$fixture/invalid-build.stderr"
then
  echo "Expected invalid GUI build number to be rejected." >&2
  exit 1
fi
grep -q "Invalid STAX_GUI_BUILD_NUMBER" "$fixture/invalid-build.stderr"
test ! -e "$fixture/invalid-build/Stax.app"

if STAX_GUI_BINARY="$binary" \
  STAX_GUI_OUTPUT="$fixture/missing-icon/Stax.app" \
  STAX_GUI_ICON="$fixture/missing.icns" \
  "$repo_root/scripts/build-gui-app.sh" \
  >"$fixture/missing-icon.stdout" 2>"$fixture/missing-icon.stderr"
then
  echo "Expected missing GUI icon to be rejected." >&2
  exit 1
fi
grep -q "Missing app icon" "$fixture/missing-icon.stderr"
test ! -e "$fixture/missing-icon/Stax.app"

home="$fixture/home"
mkdir -p "$home"
recorder="$fixture/lsregister"
recorded_path="$fixture/registered-path"
cat >"$recorder" <<SCRIPT
#!/usr/bin/env bash
test "\$1" = "-f"
printf '%s\n' "\$2" > "$recorded_path"
SCRIPT
chmod +x "$recorder"

HOME="$home" \
STAX_GUI_BINARY="$binary" \
STAX_GUI_OUTPUT="$fixture/install-source/Stax.app" \
STAX_GUI_LSREGISTER="$recorder" \
STAX_GUI_VERSION="1.2.3" \
STAX_GUI_BUILD_NUMBER="456" \
  "$repo_root/scripts/build-gui-app.sh" --install

installed_app="$home/Applications/Stax.app"
test -d "$installed_app"
test "$(cat "$recorded_path")" = "$installed_app"
