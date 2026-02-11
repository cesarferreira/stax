# Plan
- [x] Add submit scope support in `src/commands/submit.rs` (`SubmitScope`, scoped branch selection, and narrow-scope safety checks).
- [x] Wire scoped submit subcommands in `src/main.rs` for `branch submit`, `upstack submit`, and `downstack submit` while keeping top-level `submit` stack-scoped.
- [x] Update docs in `README.md` and `skills.md` for scoped submit commands and freephite mapping.
- [x] Add/adjust CLI and integration tests for new help/output and scope behavior.
- [x] Run requested test suites and document review results.

# Review
- [x] Added `SubmitScope` and scope-aware submit branch selection in `/Users/cesarferreira/code/github/stax/src/commands/submit.rs`.
- [x] Added narrow-scope safety guards for `branch`/`upstack` submit and explicit trunk submit failure in `/Users/cesarferreira/code/github/stax/src/commands/submit.rs`.
- [x] Added `--no-pr` push-only planning path that avoids mandatory GitHub API calls while preserving best-effort PR metadata refresh in `/Users/cesarferreira/code/github/stax/src/commands/submit.rs`.
- [x] Added reusable `SubmitOptions` plus subcommand wiring for `stax branch submit`, `stax upstack submit`, `stax downstack submit` in `/Users/cesarferreira/code/github/stax/src/main.rs`.
- [x] Updated cascade submit invocation to pass stack scope in `/Users/cesarferreira/code/github/stax/src/commands/cascade.rs`.
- [x] Updated user-facing docs and parity mappings in `/Users/cesarferreira/code/github/stax/README.md` and `/Users/cesarferreira/code/github/stax/skills.md`.
- [x] Added scoped submit help coverage in `/Users/cesarferreira/code/github/stax/tests/cli_tests.rs`.
- [x] Added integration coverage for scoped submit behaviors and narrow-scope safety in `/Users/cesarferreira/code/github/stax/tests/integration_tests.rs`.
- [x] Validation run: `cargo test --test cli_tests` and `cargo test --test integration_tests` (both passing).
