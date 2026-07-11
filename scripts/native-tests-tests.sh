#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="${root}/scripts/native-tests.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

fake_cargo="${tmp}/fake-cargo"
cat >"${fake_cargo}" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if env | grep -Eq '^(GITHUB_TOKEN|STAX_GITHUB_TOKEN|GH_TOKEN)='; then
  echo "GitHub token environment leaked into cargo" >&2
  exit 90
fi
printf '%s\n' "$*" >>"${STAX_NATIVE_TEST_LOG}"
if [[ -n "${STAX_NATIVE_TEST_FAIL_MATCH:-}" ]] && \
  [[ "$*" == *"${STAX_NATIVE_TEST_FAIL_MATCH}"* ]]; then
  exit 23
fi
exit 0
FAKE
chmod +x "${fake_cargo}"

log="${tmp}/cargo.log"
GITHUB_TOKEN=secret STAX_GITHUB_TOKEN=secret GH_TOKEN=secret \
  STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
  STAX_NATIVE_TEST_LOG="${log}" \
  STAX_TEST_TMPDIR="${tmp}/test-tmp" \
  TMPDIR="${tmp}/test-tmp" \
  NEXTEST_TEST_THREADS=8 \
  NATIVE_CARGO_PROFILE=test-container \
  "${runner}"

grep -Fxq 'nextest run --cargo-profile test-container' "${log}"
[[ "$(wc -l <"${log}" | tr -d ' ')" -eq 1 ]]

: >"${log}"
set +e
STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
  STAX_NATIVE_TEST_LOG="${log}" \
  STAX_NATIVE_TEST_FAIL_MATCH='nextest run' \
  STAX_TEST_TMPDIR="${tmp}/test-tmp" \
  "${runner}"
status=$?
set -e
[[ "${status}" -eq 23 ]]
[[ "$(wc -l <"${log}" | tr -d ' ')" -eq 1 ]]

set +e
(
  ulimit -Sn 64
  ulimit -Hn 64
  STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
    STAX_NATIVE_TEST_LOG="${tmp}/limit.log" \
    STAX_NATIVE_TEST_REQUIRED_NOFILE=4096 \
    STAX_TEST_TMPDIR="${tmp}/test-tmp" \
    "${runner}"
) >"${tmp}/limit.stdout" 2>"${tmp}/limit.stderr"
status=$?
set -e
[[ "${status}" -eq 2 ]]
if ! grep -q 'require a file-descriptor limit of at least 4096' "${tmp}/limit.stderr"; then
  echo "expected native file-limit error, got:" >&2
  cat "${tmp}/limit.stderr" >&2
  exit 1
fi
[[ ! -e "${tmp}/limit.log" ]]

echo "native test runner checks passed"
