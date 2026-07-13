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
  "$repo_root/scripts/build-gui-app.sh"

test -x "$app/Contents/MacOS/Stax"
/usr/bin/plutil -lint "$app/Contents/Info.plist" >/dev/null
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")" = "dev.stax.Stax"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")" = "Stax"
test "$(/usr/libexec/PlistBuddy -c 'Print :CFBundlePackageType' "$app/Contents/Info.plist")" = "APPL"

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
  "$repo_root/scripts/build-gui-app.sh" --install

installed_app="$home/Applications/Stax.app"
test -d "$installed_app"
test "$(cat "$recorded_path")" = "$installed_app"
