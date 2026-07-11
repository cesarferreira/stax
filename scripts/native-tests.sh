#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

required_nofile="${STAX_NATIVE_TEST_REQUIRED_NOFILE:-4096}"
threads="${NEXTEST_TEST_THREADS:-8}"
profile="${NATIVE_CARGO_PROFILE:-test-container}"
test_tmpdir="${STAX_TEST_TMPDIR:-${root}/.test-tmp}"
cargo_cmd="${STAX_NATIVE_TEST_CARGO:-cargo}"

soft_nofile="$(ulimit -Sn)"
hard_nofile="$(ulimit -Hn)"

if [[ "${soft_nofile}" != "unlimited" ]] && (( soft_nofile < required_nofile )); then
  if [[ "${hard_nofile}" != "unlimited" ]] && (( hard_nofile < required_nofile )); then
    echo "native tests require a file-descriptor limit of at least ${required_nofile}; hard limit is ${hard_nofile}" >&2
    exit 2
  fi

  if ! ulimit -Sn "${required_nofile}"; then
    echo "failed to raise the native test file-descriptor limit to ${required_nofile}" >&2
    exit 2
  fi
fi

mkdir -p "${test_tmpdir}"

unset GITHUB_TOKEN STAX_GITHUB_TOKEN GH_TOKEN
export STAX_DISABLE_UPDATE_CHECK=1
export STAX_TEST_TMPDIR="${test_tmpdir}"
export TMPDIR="${TMPDIR:-${test_tmpdir}}"
export NEXTEST_TEST_THREADS="${threads}"
export RUST_MIN_STACK="${RUST_MIN_STACK:-4194304}"

echo "native tests: profile=${profile} threads=${threads} nofile=$(ulimit -Sn)"
"${cargo_cmd}" nextest run --cargo-profile "${profile}"
