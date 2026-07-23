#!/bin/sh
set -eu

RELEASE_BASE="${STAX_GUI_RELEASE_BASE:-https://github.com/cesarferreira/stax/releases/latest/download}"
INSTALL_DIR="${STAX_GUI_INSTALL_DIR:-/Applications}"

main() {
  echo ""
  echo "  stax GUI installer"
  echo "  cesarferreira.com/stax"
  echo ""

  case "$(uname -s)" in
    Darwin) ;;
    *)
      err "Stax GUI is macOS-only"
      ;;
  esac

  case "$(uname -m)" in
    arm64|aarch64) arch="aarch64" ;;
    x86_64) arch="x86_64" ;;
    *)
      err "unsupported architecture: $(uname -m)"
      ;;
  esac

  log "detected macos/${arch}"

  need curl

  url="${STAX_GUI_DOWNLOAD_URL:-${RELEASE_BASE}/Stax-${arch}-apple-darwin.zip}"
  dest="$INSTALL_DIR"

  if [ ! -d "$dest" ]; then
    if [ "$dest" = "/Applications" ] && [ -d "$HOME/Applications" ]; then
      dest="$HOME/Applications"
      warn "using ${dest} because /Applications is unavailable"
    else
      err "install directory does not exist: ${dest}"
    fi
  elif [ ! -w "$dest" ]; then
    if [ "$dest" = "/Applications" ] && [ -d "$HOME/Applications" ] && [ -w "$HOME/Applications" ]; then
      dest="$HOME/Applications"
      warn "using ${dest} because /Applications is not writable"
    else
      err "install directory is not writable: ${dest}"
    fi
  fi

  log "downloading latest Stax.app..."
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  if ! curl -fsSL --retry 3 --connect-timeout 10 --max-time 300 "$url" -o "${tmp}/Stax.zip"; then
    err "download failed from ${url}"
  fi

  log "extracting archive..."
  if ! ditto -x -k "${tmp}/Stax.zip" "$tmp"; then
    err "failed to extract Stax.zip"
  fi

  if [ ! -d "${tmp}/Stax.app" ]; then
    err "archive did not contain Stax.app"
  fi

  log "installing to ${dest}/Stax.app..."
  rm -rf "${dest}/Stax.app"
  mv "${tmp}/Stax.app" "${dest}/Stax.app"

  echo ""
  log "installed Stax.app to ${dest}/Stax.app"
  echo ""
  echo "  launch from Finder or Spotlight, or run:"
  echo ""
  echo "    st gui [path]"
  echo ""
  warn "download only from GitHub Releases or cesarferreira.com/stax"
  echo ""
  echo "  If macOS blocks the first launch, open ${dest}/Stax.app once,"
  echo "  then choose System Settings → Privacy & Security → Open Anyway."
  echo "  Do not disable Gatekeeper globally."
  echo ""
}

log() { printf '  \033[32m>\033[0m %s\n' "$1"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$1"; }
err() { printf '  \033[31m✗\033[0m %s\n' "$1" >&2; exit 1; }

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    err "requires '$1'"
  fi
}

main "$@"
