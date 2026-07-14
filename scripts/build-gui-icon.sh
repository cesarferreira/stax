#!/usr/bin/env bash
set -euo pipefail

if [ "$(uname)" != "Darwin" ]; then
  echo "Stax app icon conversion is only supported on macOS." >&2
  exit 1
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
source="${1:-$repo_root/crates/stax-gui/resources/AppIcon-1024.png}"
output="${2:-$repo_root/crates/stax-gui/resources/AppIcon.icns}"

if [ "$#" -gt 2 ]; then
  echo "Usage: $0 [source-1024.png] [output.icns]" >&2
  exit 1
fi

if [ ! -f "$source" ]; then
  echo "Missing icon source: $source" >&2
  exit 1
fi

width="$(sips -g pixelWidth "$source" | awk '/pixelWidth/{print $2}')"
height="$(sips -g pixelHeight "$source" | awk '/pixelHeight/{print $2}')"
format="$(sips -g format "$source" | awk '/format:/{print $2}')"
if [ "$format" != "png" ]; then
  echo "Icon source must be a PNG file; got $format." >&2
  exit 1
fi
if [ "$width" != "1024" ] || [ "$height" != "1024" ]; then
  echo "Icon source must be exactly 1024x1024 pixels; got ${width}x${height}." >&2
  exit 1
fi

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
iconset="$fixture/AppIcon.iconset"
mkdir -p "$iconset"

for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$source" \
    --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$source" \
    --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done

mkdir -p "$(dirname "$output")"
iconutil --convert icns --output "$fixture/AppIcon.icns" "$iconset"
cp "$fixture/AppIcon.icns" "$output"
echo "Built Stax app icon at $output"
