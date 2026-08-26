#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/.." && pwd)"
root="${1:-$repo_root}"

if (($# > 1)); then
  echo "usage: application-boundary-lint.sh [repository-root]" >&2
  exit 2
fi

if ! command -v python3 >/dev/null 2>&1; then
  echo "application boundary lint requires python3" >&2
  exit 2
fi

exec python3 "$script_dir/application-boundary-lint.py" "$root"
