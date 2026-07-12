#!/usr/bin/env bash
set -euo pipefail

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

bash scripts/application-boundary-lint.sh

# Keep new Clippy warnings fatal while the explicitly listed legacy lint debt is
# paid down. Removing an allowance is always safe; adding one requires review.
cargo clippy --all-targets --all-features --no-deps -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::clone_on_copy \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::let_and_return \
  -A clippy::manual_checked_ops \
  -A clippy::needless_borrow \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::unnecessary_sort_by \
  -A clippy::useless_format \
  -A clippy::useless_vec
