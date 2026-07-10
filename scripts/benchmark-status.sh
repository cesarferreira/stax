#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
stax_bin=${STAX_BENCH_BIN:-"${repo_root}/target/release/stax"}
stack_sizes=${STAX_BENCH_STACK_SIZES:-"10 50 100"}
runs=${STAX_BENCH_RUNS:-10}
output_dir=${STAX_BENCH_OUTPUT_DIR:-"${repo_root}/target/benchmarks"}

command -v hyperfine >/dev/null 2>&1 || {
  echo "hyperfine is required (brew install hyperfine or cargo install hyperfine)" >&2
  exit 1
}

if [[ ! -x "${stax_bin}" ]]; then
  cargo build --manifest-path "${repo_root}/Cargo.toml" --release --bin stax
fi

mkdir -p "${output_dir}"
fixtures=$(mktemp -d)
trap 'rm -rf "${fixtures}"' EXIT

create_fixture() {
  local size=$1
  local fixture="${fixtures}/stack-${size}"
  git init -q -b main "${fixture}"
  git -C "${fixture}" config user.name "Stax Benchmark"
  git -C "${fixture}" config user.email "benchmark@stax.local"
  printf 'benchmark fixture\n' > "${fixture}/benchmark.txt"
  git -C "${fixture}" add benchmark.txt
  git -C "${fixture}" commit -qm "benchmark: initialize fixture"
  (
    cd "${fixture}"
    STAX_DISABLE_UPDATE_CHECK=1 "${stax_bin}" init --trunk main >/dev/null
  )

  local parent=main
  for ((index = 1; index <= size; index++)); do
    local branch="bench-${index}"
    local parent_revision
    parent_revision=$(git -C "${fixture}" rev-parse "${parent}")
    git -C "${fixture}" checkout -qb "${branch}"
    printf 'branch %s\n' "${index}" >> "${fixture}/benchmark.txt"
    git -C "${fixture}" add benchmark.txt
    git -C "${fixture}" commit -qm "benchmark: branch ${index}"

    local metadata oid
    metadata=$(printf '{"parentBranchName":"%s","parentBranchRevision":"%s"}' "${parent}" "${parent_revision}")
    oid=$(printf '%s' "${metadata}" | git -C "${fixture}" hash-object -w --stdin)
    git -C "${fixture}" update-ref "refs/branch-metadata/${branch}" "${oid}"
    parent=${branch}
  done

  printf '%s\n' "${fixture}"
}

for size in ${stack_sizes}; do
  fixture=$(create_fixture "${size}")
  cache_file="${fixture}/.git/stax/ahead-behind-cache.json"
  command="cd '${fixture}' && STAX_DISABLE_UPDATE_CHECK=1 '${stax_bin}' status --json >/dev/null"
  echo "Benchmarking cold status --json with ${size} branches"
  hyperfine \
    --warmup 1 \
    --runs "${runs}" \
    --prepare "rm -f '${cache_file}'" \
    --export-json "${output_dir}/status-${size}.json" \
    "${command}"
done

echo "Benchmark JSON written to ${output_dir}"
