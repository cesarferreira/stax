#!/usr/bin/env bash
set -euo pipefail

mode="${1:-full}"
case "${mode}" in
  fast|full) ;;
  *)
    echo "invalid lint mode '${mode}'; expected 'fast' or 'full'" >&2
    exit 2
    ;;
esac

if ! command -v rg >/dev/null 2>&1; then
  echo "lint requires ripgrep (rg) to check test environment mutations" >&2
  exit 1
fi

global_env_mutations="$(rg -n '(std::)?env::(set_var|remove_var)' tests --glob '*.rs' || true)"
if [[ -n "${global_env_mutations}" ]]; then
  echo "integration tests must configure child commands instead of mutating process-global environment" >&2
  echo "${global_env_mutations}" >&2
  exit 1
fi

bash scripts/application-boundary-lint-tests.sh
bash scripts/application-boundary-lint.sh
bash scripts/clippy-lint-tests.sh
bash scripts/clippy-lint.sh "${mode}"
