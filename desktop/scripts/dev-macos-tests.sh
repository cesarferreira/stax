#!/usr/bin/env bash
set -euo pipefail

SOURCE_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SOURCE_DEV_SCRIPT="$SOURCE_ROOT/desktop/scripts/dev-macos.sh"
SOURCE_PACKAGE_SCRIPT="$SOURCE_ROOT/desktop/scripts/package-macos.sh"
TMP_ROOT="${TMPDIR:-/tmp}"
WORK="$(mktemp -d "${TMP_ROOT%/}/stax-desktop-dev-tests.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

new_fixture() {
  local name="$1"
  local repo="$WORK/$name/repo"
  local fake_bin="$WORK/$name/bin"

  mkdir -p "$repo/desktop/scripts" "$fake_bin"
  cp "$SOURCE_DEV_SCRIPT" "$repo/desktop/scripts/dev-macos.sh"
  cp "$SOURCE_PACKAGE_SCRIPT" "$repo/desktop/scripts/package-macos.sh"

  cat >"$fake_bin/cargo" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'cargo:%s\n' "$*" >>"$CALLS"
if [[ -n "${FAIL_CARGO:-}" ]]; then
  exit "$FAIL_CARGO"
fi
profile=debug
binary=""
previous=""
for argument in "$@"; do
  if [[ "$argument" == "--release" ]]; then
    profile=release
  elif [[ "$previous" == "--bin" ]]; then
    binary="$argument"
  fi
  previous="$argument"
done
mkdir -p "$CARGO_TARGET_DIR/$profile"
cat >"$CARGO_TARGET_DIR/$profile/$binary" <<'ENGINE'
#!/usr/bin/env bash
exit 0
ENGINE
chmod +x "$CARGO_TARGET_DIR/$profile/$binary"
EOF
  chmod +x "$fake_bin/cargo"

  cat >"$fake_bin/uname" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "${1:-}" in
  -s) printf 'Darwin\n' ;;
  -m) printf 'arm64\n' ;;
  *) printf 'Darwin\n' ;;
esac
EOF
  chmod +x "$fake_bin/uname"

  cat >"$fake_bin/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'npm:%s\n' "$*" >>"$CALLS"
if [[ "${FAIL_NPM_CI:-0}" == "1" ]]; then
  exit 42
fi
mkdir -p "$REPO/desktop/node_modules/.bin"
cat >"$REPO/desktop/node_modules/.bin/native" <<'NATIVE'
#!/usr/bin/env bash
set -euo pipefail
printf 'native:%s\n' "$*" >>"$CALLS"
printf 'engine:%s\n' "$STAX_DESKTOP_ENGINE" >>"$CALLS"
NATIVE
chmod +x "$REPO/desktop/node_modules/.bin/native"
EOF
  chmod +x "$fake_bin/npm"

  printf '%s\n%s\n' "$repo" "$fake_bin"
}

test_installs_missing_cli_and_launches_local_binary() {
  local fixture repo fake_bin calls expected_engine
  fixture="$(new_fixture missing-cli)"
  repo="$(sed -n '1p' <<<"$fixture")"
  fake_bin="$(sed -n '2p' <<<"$fixture")"
  calls="$WORK/missing-cli/calls"
  expected_engine="$repo/target/desktop-engine/debug/stax"

  CALLS="$calls" REPO="$repo" PATH="$fake_bin:/usr/bin:/bin" \
    bash "$repo/desktop/scripts/dev-macos.sh"

  grep -Fx "npm:ci --prefix $repo/desktop" "$calls" >/dev/null
  grep -Fx 'cargo:build --bin stax' "$calls" >/dev/null
  grep -Fx 'native:dev' "$calls" >/dev/null
  grep -Fx "engine:$expected_engine" "$calls" >/dev/null
}

test_reuses_installed_cli_without_reinstalling() {
  local fixture repo fake_bin calls native_bin
  fixture="$(new_fixture installed-cli)"
  repo="$(sed -n '1p' <<<"$fixture")"
  fake_bin="$(sed -n '2p' <<<"$fixture")"
  calls="$WORK/installed-cli/calls"
  native_bin="$repo/desktop/node_modules/.bin/native"
  mkdir -p "$(dirname "$native_bin")"
  cat >"$native_bin" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'native:%s\n' "$*" >>"$CALLS"
EOF
  chmod +x "$native_bin"

  CALLS="$calls" REPO="$repo" PATH="$fake_bin:/usr/bin:/bin" \
    bash "$repo/desktop/scripts/dev-macos.sh"

  if grep -q '^npm:' "$calls"; then
    echo "desktop dev reinstalled an already available Native SDK CLI" >&2
    return 1
  fi
  grep -Fx 'native:dev' "$calls" >/dev/null
}

test_propagates_dependency_install_failure() {
  local fixture repo fake_bin calls status
  fixture="$(new_fixture install-failure)"
  repo="$(sed -n '1p' <<<"$fixture")"
  fake_bin="$(sed -n '2p' <<<"$fixture")"
  calls="$WORK/install-failure/calls"

  set +e
  CALLS="$calls" REPO="$repo" FAIL_NPM_CI=1 PATH="$fake_bin:/usr/bin:/bin" \
    bash "$repo/desktop/scripts/dev-macos.sh"
  status=$?
  set -e

  if [[ $status -ne 42 ]]; then
    echo "expected npm ci failure status 42, got $status" >&2
    return 1
  fi
  if grep -q '^native:' "$calls"; then
    echo "desktop dev launched Native SDK after npm ci failed" >&2
    return 1
  fi
}

test_package_builds_the_real_engine_binary() {
  local fixture repo fake_bin calls status
  fixture="$(new_fixture package-engine)"
  repo="$(sed -n '1p' <<<"$fixture")"
  fake_bin="$(sed -n '2p' <<<"$fixture")"
  calls="$WORK/package-engine/calls"

  set +e
  CALLS="$calls" REPO="$repo" FAIL_CARGO=42 PATH="$fake_bin:/usr/bin:/bin" \
    bash "$repo/desktop/scripts/package-macos.sh"
  status=$?
  set -e

  if [[ $status -ne 42 ]]; then
    echo "expected the package probe to stop after Cargo, got $status" >&2
    return 1
  fi
  grep -Fx 'cargo:build --release --bin stax' "$calls" >/dev/null
}

test_installs_missing_cli_and_launches_local_binary
test_reuses_installed_cli_without_reinstalling
test_propagates_dependency_install_failure
test_package_builds_the_real_engine_binary
echo "desktop dev wrapper tests passed"
