#!/usr/bin/env bash
set -euo pipefail

mode="${1:-full}"
cargo_bin="${CLIPPY_LINT_CARGO:-cargo}"

case "${mode}" in
  fast)
    targets=(--lib --bins)
    ;;
  full)
    targets=(--all-targets --all-features)
    ;;
  *)
    echo "invalid lint mode '${mode}'; expected 'fast' or 'full'" >&2
    exit 2
    ;;
esac

# Keep new Clippy warnings fatal while the explicitly listed legacy lint debt is
# paid down. Removing an allowance is always safe; adding one requires review.
"${cargo_bin}" clippy "${targets[@]}" --no-deps -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::useless_format \
  -A clippy::useless_vec
