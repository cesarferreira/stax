#!/usr/bin/env bash
set -euo pipefail

version="${INPUT_VERSION:-latest}"
runner_os="${RUNNER_OS:-}"
arch="${STAX_INSTALL_ARCH:-$(uname -m)}"

case "${runner_os}:${arch}" in
  macOS:arm64|macOS:aarch64)
    artifact="stax-aarch64-apple-darwin.tar.gz"
    executable="stax"
    ;;
  macOS:x86_64|macOS:amd64)
    artifact="stax-x86_64-apple-darwin.tar.gz"
    executable="stax"
    ;;
  Linux:arm64|Linux:aarch64)
    artifact="stax-aarch64-unknown-linux-gnu.tar.gz"
    executable="stax"
    ;;
  Linux:x86_64|Linux:amd64)
    artifact="stax-x86_64-unknown-linux-gnu.tar.gz"
    executable="stax"
    ;;
  Windows:x86_64|Windows:amd64)
    artifact="stax-x86_64-pc-windows-msvc.zip"
    executable="stax.exe"
    ;;
  *)
    echo "Unsupported runner platform: ${runner_os:-unknown}/${arch:-unknown}" >&2
    exit 1
    ;;
esac

if [[ "$version" == "latest" ]]; then
  download_url="https://github.com/cesarferreira/stax/releases/latest/download/${artifact}"
else
  [[ "$version" == v* ]] || version="v${version}"
  download_url="https://github.com/cesarferreira/stax/releases/download/${version}/${artifact}"
fi

if [[ "${STAX_INSTALL_DRY_RUN:-0}" == "1" ]]; then
  printf '%s\n' "$download_url"
  exit 0
fi

temp_dir="$(mktemp -d)"
trap 'rm -rf "$temp_dir"' EXIT
archive="$temp_dir/$artifact"
install_dir="${RUNNER_TEMP:-$temp_dir}/stax-bin"
rm -rf "$install_dir"
mkdir -p "$install_dir"

curl --fail --silent --show-error --location "$download_url" --output "$archive"
if [[ "$artifact" == *.zip ]]; then
  unzip -q "$archive" -d "$install_dir"
else
  tar -xzf "$archive" -C "$install_dir"
  chmod +x "$install_dir/stax" "$install_dir/st"
fi

installed_version="$($install_dir/$executable --version | awk '{print $2}')"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  printf '%s\n' "$install_dir" >> "$GITHUB_PATH"
fi
if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
  printf 'installed-version=%s\n' "$installed_version" >> "$GITHUB_OUTPUT"
fi
printf 'Installed stax %s from %s\n' "$installed_version" "$artifact"
