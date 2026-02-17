# Plan
- [x] Make stack loading ignore/prune metadata entries for branches that no longer exist locally.
- [x] Make metadata ref deletion/writes fail on git errors (so stale metadata cannot silently survive).
- [x] Switch TUI diff/stat generation to PR-style merge-base diffs instead of direct parent-vs-branch tree diffs.
- [x] Add TUI diff caching so up/down navigation doesn’t recompute expensive diffs on every keypress.
- [x] Run focused tests and document review results.

# Review
- [x] `Stack::load` now skips metadata entries whose local branch no longer exists and best-effort deletes those stale metadata refs (`/Users/cesarferreira/code/github/stax/src/engine/stack.rs`).
- [x] Metadata git-ref operations now fail loudly on non-zero `git update-ref` exit status (`/Users/cesarferreira/code/github/stax/src/git/refs.rs`).
- [x] TUI diff/stat now uses merge-base diff range (`parent...branch`) to match PR semantics and avoid pulling in unrelated parent-side changes (`/Users/cesarferreira/code/github/stax/src/git/repo.rs`).
- [x] Added in-memory diff cache keyed by `parent...branch`, and clear cache on refresh so navigation reuses previously loaded diffs (`/Users/cesarferreira/code/github/stax/src/tui/app.rs`).
- [x] Verification: `cargo test --test tui_commands_tests` passed; focused suites `cargo test engine::stack::tests::` and `cargo test git::repo::tests::` passed.
- [x] Note: `cargo test --lib` has pre-existing unrelated failures in config tests (`test_github_token_roundtrip`, `test_github_token_trims_whitespace_from_file`, `test_format_template_empty_user_message_only_format`).
