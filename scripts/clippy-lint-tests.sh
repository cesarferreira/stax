#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "${tmp_dir}"' EXIT

fake_cargo="${tmp_dir}/cargo"
calls_file="${tmp_dir}/calls"

cat >"${fake_cargo}" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" >>"${CLIPPY_LINT_TEST_CALLS}"
EOF
chmod +x "${fake_cargo}"

assert_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "${haystack}" != *"${needle}"* ]]; then
    echo "expected command to contain: ${needle}" >&2
    echo "actual command: ${haystack}" >&2
    exit 1
  fi
}

assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  if [[ "${haystack}" == *"${needle}"* ]]; then
    echo "expected command not to contain: ${needle}" >&2
    echo "actual command: ${haystack}" >&2
    exit 1
  fi
}

run_lint() {
  local mode="$1"
  : >"${calls_file}"
  CLIPPY_LINT_CARGO="${fake_cargo}" \
    CLIPPY_LINT_TEST_CALLS="${calls_file}" \
    bash "${repo_root}/scripts/clippy-lint.sh" "${mode}"
  cat "${calls_file}"
}

fast_call="$(run_lint fast)"
assert_contains "${fast_call}" "clippy --lib --bins --no-deps --"
assert_not_contains "${fast_call}" "--all-targets"
assert_not_contains "${fast_call}" "--all-features"

full_call="$(run_lint full)"
assert_contains "${full_call}" "clippy --all-targets --all-features --no-deps --"
assert_not_contains "${full_call}" "--lib"
assert_not_contains "${full_call}" "--bins"

: >"${calls_file}"
if CLIPPY_LINT_CARGO="${fake_cargo}" \
  CLIPPY_LINT_TEST_CALLS="${calls_file}" \
  bash "${repo_root}/scripts/clippy-lint.sh" unexpected >"${tmp_dir}/stdout" 2>"${tmp_dir}/stderr"; then
  echo "expected an invalid lint mode to fail" >&2
  exit 1
fi

if [[ -s "${calls_file}" ]]; then
  echo "invalid lint mode must not invoke cargo" >&2
  exit 1
fi

assert_contains "$(cat "${tmp_dir}/stderr")" "expected 'fast' or 'full'"

echo "clippy lint mode tests passed"
