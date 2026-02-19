# Plan
- [x] Make stack loading ignore/prune metadata entries for branches that no longer exist locally.
- [x] Make metadata ref deletion/writes fail on git errors (so stale metadata cannot silently survive).
- [x] Switch TUI diff/stat generation to PR-style merge-base diffs instead of direct parent-vs-branch tree diffs.
- [x] Add TUI diff caching so up/down navigation doesn’t recompute expensive diffs on every keypress.
- [x] Run focused tests and document review results.

## GitHub auth mapping
- [ ] Inventory current GitHub auth resolution paths and where STAX token fits.
- [ ] Record docs that describe the token priority order and what must change.
- [ ] Capture tests covering token selection behavior and note update needs.

## GitHub auth-first implementation
- [x] Add `auth` config section with defaults and backward-compatible serde behavior.
- [x] Refactor token lookup order to `STAX_GITHUB_TOKEN -> .credentials -> gh auth token -> opt-in GITHUB_TOKEN`.
- [x] Add `stax auth --from-gh` and wire CLI arg conflict with `--token`.
- [x] Update user-facing missing-auth messages and doctor output wording.
- [x] Update README auth docs and config examples.
- [x] Update/add tests for new auth precedence and CLI behavior.
- [x] Run verification commands and capture results.

# Review
- [x] `Stack::load` now skips metadata entries whose local branch no longer exists and best-effort deletes those stale metadata refs (`/Users/cesarferreira/code/github/stax/src/engine/stack.rs`).
- [x] Metadata git-ref operations now fail loudly on non-zero `git update-ref` exit status (`/Users/cesarferreira/code/github/stax/src/git/refs.rs`).
- [x] TUI diff/stat now uses merge-base diff range (`parent...branch`) to match PR semantics and avoid pulling in unrelated parent-side changes (`/Users/cesarferreira/code/github/stax/src/git/repo.rs`).
- [x] Added in-memory diff cache keyed by `parent...branch`, and clear cache on refresh so navigation reuses previously loaded diffs (`/Users/cesarferreira/code/github/stax/src/tui/app.rs`).
- [x] Verification: `cargo test --test tui_commands_tests` passed; focused suites `cargo test engine::stack::tests::` and `cargo test git::repo::tests::` passed.
- [x] Note: `cargo test --lib` has pre-existing unrelated failures in config tests (`test_github_token_roundtrip`, `test_github_token_trims_whitespace_from_file`, `test_format_template_empty_user_message_only_format`).
- [x] Added `[auth]` config with defaults (`use_gh_cli=true`, `allow_github_token_env=false`, optional `gh_hostname`) and switched auth precedence to `STAX_GITHUB_TOKEN -> credentials -> gh -> opt-in GITHUB_TOKEN` (`/Users/cesarferreira/code/github/stax/src/config/mod.rs`).
- [x] Added `stax auth --from-gh` import path, with clap conflict against `--token` (`/Users/cesarferreira/code/github/stax/src/main.rs`, `/Users/cesarferreira/code/github/stax/src/commands/auth.rs`).
- [x] Updated missing-auth messaging and doctor output wording (`/Users/cesarferreira/code/github/stax/src/github/client.rs`, `/Users/cesarferreira/code/github/stax/src/commands/ci.rs`, `/Users/cesarferreira/code/github/stax/src/commands/doctor.rs`).
- [x] Updated README auth section and config reference for new behavior (`/Users/cesarferreira/code/github/stax/README.md`).
- [x] Added/updated tests for auth precedence and CLI flags; stabilized env-mutating config tests with a mutex (`/Users/cesarferreira/code/github/stax/src/config/tests.rs`, `/Users/cesarferreira/code/github/stax/tests/auth_tests.rs`, `/Users/cesarferreira/code/github/stax/tests/ci_tests.rs`).
- [x] Verification: `cargo test config::tests`, `cargo test --test auth_tests`, `cargo test --test ci_tests --test track_all_prs_tests` all passed.
- [x] Full suite note: `cargo test` still has one pre-existing unrelated failure in `/Users/cesarferreira/code/github/stax/src/config/tests.rs` (`test_format_template_empty_user_message_only_format`).

## Docs migration (README -> MkDocs Material)

# Plan
- [x] Add MkDocs Material config with structured navigation.
- [x] Split README topics into focused pages under `docs/`.
- [x] Keep README concise and link to docs sections.
- [x] Run docs sanity checks (file/link consistency) and capture results.

# Review
- [x] Added MkDocs Material config and nav in `/Users/cesarferreira/code/github/stax/mkdocs.yml`.
- [x] Added split docs sections under `/Users/cesarferreira/code/github/stax/docs/` (getting started, concepts, interface, commands, workflows, safety, configuration, integrations, compatibility, benchmarks).
- [x] Added docs assets + styling (`/Users/cesarferreira/code/github/stax/docs/assets/`, `/Users/cesarferreira/code/github/stax/docs/stylesheets/extra.css`).
- [x] Replaced large README with concise landing + links to docs and local docs run instructions (`/Users/cesarferreira/code/github/stax/README.md`).
- [x] Added docs dependency pinning for MkDocs Material workflow (`/Users/cesarferreira/code/github/stax/docs/requirements.txt`).
- [x] Added generated docs artifacts ignores (`/Users/cesarferreira/code/github/stax/.gitignore`).
- [x] Verification: `python3 -m venv .venv && source .venv/bin/activate && pip install -q -r docs/requirements.txt && mkdocs build --strict` passed.
- [x] Build note: MkDocs reported informational \"not in nav\" entries for existing `/Users/cesarferreira/code/github/stax/docs/plans/*` files.

## Docs deployment (GitHub Pages)

# Plan
- [x] Add GitHub Actions workflow to build and deploy MkDocs docs to GitHub Pages.
- [x] Set docs site URL in MkDocs config to GitHub Pages URL.
- [x] Validate workflow YAML and document verification results.

# Review
- [x] Added `/Users/cesarferreira/code/github/stax/.github/workflows/docs-pages.yml` with official Pages flow (`upload-pages-artifact` + `deploy-pages`), `pages/id-token` permissions, and push triggers on docs/config changes.
- [x] Updated MkDocs canonical site URL to `/Users/cesarferreira/code/github/stax/mkdocs.yml` -> `https://cesarferreira.github.io/stax/`.
- [x] Verification: `ruby -e 'require \"yaml\"; YAML.load_file(\".github/workflows/docs-pages.yml\")'` succeeded (valid YAML).
- [x] Verification: `source .venv/bin/activate && mkdocs build --strict` passed after workflow changes.
- [x] Build note: existing informational \"not in nav\" entries remain for `/Users/cesarferreira/code/github/stax/docs/plans/*`.

## Docs engine migration (MkDocs -> Zensical)

# Plan
- [x] Switch docs dependencies from MkDocs to Zensical.
- [x] Update GitHub Pages workflow to build docs with Zensical via `uv`.
- [x] Revert README to the long-form version and add a full-docs link.
- [x] Validate workflow YAML and run a local Zensical build.

# Review
- [x] Replaced docs build dependency with `zensical` in `/Users/cesarferreira/code/github/stax/docs/requirements.txt`.
- [x] Updated `/Users/cesarferreira/code/github/stax/.github/workflows/docs-pages.yml` to use `astral-sh/setup-uv@v6` and build via `uv run --with-requirements docs/requirements.txt zensical build --clean`.
- [x] Restored long-form README from pre-doc-split commit state and added a dedicated docs link section at `/Users/cesarferreira/code/github/stax/README.md`.
- [x] Verification: `ruby -e 'require \"yaml\"; YAML.load_file(\".github/workflows/docs-pages.yml\")'` succeeded.
- [x] Verification: `uv run --with-requirements docs/requirements.txt zensical build --clean` succeeded.
