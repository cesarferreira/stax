#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
DESKTOP="$ROOT/desktop"
APP="${STAX_DESKTOP_APP:-$DESKTOP/dist/Stax.app}"
APP_BIN="$APP/Contents/MacOS/Stax"
ENGINE_BIN="$APP/Contents/Resources/bin/st"
JQ="$(command -v jq)"
MODE="${STAX_DESKTOP_SMOKE_MODE:-production}"

test -x "$APP_BIN"
test -x "$ENGINE_BIN"
/usr/bin/codesign --verify --deep --strict "$APP"

WORK="$(mktemp -d "${TMPDIR:-/tmp}/stax-desktop-smoke.XXXXXX")"
APP_PID=""
cleanup() {
  local status=$?
  if [[ -n "$APP_PID" ]]; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi
  if [[ $status -ne 0 && -f "$WORK/app.log" ]]; then
    echo "--- packaged app log ---" >&2
    tail -n 80 "$WORK/app.log" >&2 || true
  fi
  rm -rf "$WORK"
  return "$status"
}
trap cleanup EXIT

FIXTURE_REPO="$WORK/repository"
mkdir -p "$FIXTURE_REPO"
/usr/bin/git -C "$FIXTURE_REPO" init -q -b main
/usr/bin/git -C "$FIXTURE_REPO" config user.name "Stax Smoke"
/usr/bin/git -C "$FIXTURE_REPO" config user.email "stax-smoke@example.com"
printf 'desktop smoke\n' > "$FIXTURE_REPO/README.md"
/usr/bin/git -C "$FIXTURE_REPO" add README.md
/usr/bin/git -C "$FIXTURE_REPO" commit -q -m "initial"
/usr/bin/git -C "$FIXTURE_REPO" checkout -q -b feature/desktop
printf 'native workspace\n' >> "$FIXTURE_REPO/README.md"
/usr/bin/git -C "$FIXTURE_REPO" commit -q -am "desktop change"

STAX_DISABLE_UPDATE_CHECK=1 "$ENGINE_BIN" desktop snapshot \
  --repo "$FIXTURE_REPO" \
  --schema-version 1 \
  --request-id smoke | "$JQ" -e \
  '.ok == true and .schema_version == 1 and .data.trunk != null' >/dev/null

if [[ "$MODE" == "automation" ]]; then
  SMOKE_HOME="$WORK/home"
  mkdir -p "$SMOKE_HOME/Library/Application Support/Stax"
  printf '%s\n' "$FIXTURE_REPO" > "$SMOKE_HOME/Library/Application Support/Stax/recent-repositories"
  EMPTY_PATH="$WORK/empty-path"
  mkdir -p "$EMPTY_PATH"

  rm -rf "$DESKTOP/.zig-cache/native-sdk-automation"
  (
    cd "$DESKTOP"
    env \
      HOME="$SMOKE_HOME" \
      PATH="$EMPTY_PATH:/usr/bin:/bin" \
      STAX_DISABLE_UPDATE_CHECK=1 \
      "$APP_BIN"
  ) >"$WORK/app.log" 2>&1 &
  APP_PID=$!

  (
    cd "$DESKTOP"
    npm exec -- native automate wait >/dev/null
    npm exec -- native automate assert \
      'gpu_nonblank=true' \
      'name="Stack"' \
      'name="Branch"' \
      'name="Patch"' \
      'name="repository"'
    npm exec -- native automate assert --absent \
      'error event=' \
      'dispatch_errors=[1-9]'
  )
elif [[ "$MODE" == "production" ]]; then
  /usr/bin/open -F -n "$APP"
  for _ in {1..20}; do
    APP_PID="$(/usr/bin/pgrep -n -f -x "$APP_BIN" || true)"
    [[ -n "$APP_PID" ]] && break
    sleep 0.1
  done
  if [[ -z "$APP_PID" ]]; then
    echo "packaged app exited during Launch Services startup" >&2
    exit 1
  fi
  sleep 1
  if ! kill -0 "$APP_PID" 2>/dev/null; then
    echo "packaged app exited during Launch Services startup" >&2
    exit 1
  fi
else
  echo "unknown desktop smoke mode: $MODE" >&2
  exit 2
fi

echo "Stax.app $MODE smoke test passed"
