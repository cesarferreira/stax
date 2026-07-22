#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
release_workflow="$repo_root/.github/workflows/release.yml"
test_workflow="$repo_root/.github/workflows/rust-tests.yml"

require_literal() {
  file="$1"
  literal="$2"
  if ! grep -Fq -- "$literal" "$file"; then
    echo "Missing release workflow contract in ${file#$repo_root/}: $literal" >&2
    exit 1
  fi
}

reject_literal() {
  file="$1"
  literal="$2"
  if grep -Fq -- "$literal" "$file"; then
    echo "Unexpected release workflow contract in ${file#$repo_root/}: $literal" >&2
    exit 1
  fi
}

require_literal "$release_workflow" "gui-build:"
require_literal "$release_workflow" "needs: [build, gui-build]"
require_literal "$release_workflow" "scripts/package-gui-release.sh"
require_literal "$release_workflow" "MACOS_CERTIFICATE_P12"
require_literal "$release_workflow" "security create-keychain"
require_literal "$release_workflow" "codesign"
require_literal "$release_workflow" "APPLE_APP_PASSWORD"
reject_literal "$release_workflow" 'STAX_GUI_REQUIRE_NOTARIZATION: "1"'

for target in aarch64-apple-darwin x86_64-apple-darwin; do
  require_literal "$release_workflow" "target: $target"
  require_literal "$release_workflow" "Stax-$target.zip"
done

for artifact in \
  stax-x86_64-apple-darwin.tar.gz \
  stax-aarch64-apple-darwin.tar.gz \
  stax-x86_64-unknown-linux-gnu.tar.gz \
  stax-aarch64-unknown-linux-gnu.tar.gz \
  stax-x86_64-pc-windows-msvc.zip
do
  require_literal "$release_workflow" "$artifact"
done

require_literal "$test_workflow" "gui-quality:"
require_literal "$test_workflow" "dorny/paths-filter@v3"
reject_literal "$test_workflow" "gui-packaging:"
reject_literal "$test_workflow" "make gui-app-test"
reject_literal "$test_workflow" "make gui-app"
reject_literal "$test_workflow" "make gui-release-test"

echo "GUI release workflow contract is complete."
