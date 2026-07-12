#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT

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
