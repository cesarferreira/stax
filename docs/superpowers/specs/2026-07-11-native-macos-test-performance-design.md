# Native macOS test performance

**Date:** 2026-07-11
**Status:** Design approved, self-reviewed, pending user review

## Problem

The full stax test suite is fast and stable in the Linux Docker path but has
historically taken much longer when run natively on macOS. The suite is
process- and filesystem-heavy: nextest launches each test in its own process,
integration tests launch the real `stax` binary, and fixture setup repeatedly
launches Git.

Direct measurements on the target Apple Silicon Mac showed:

| Configuration | Result |
|---|---:|
| Warm native full suite, APFS, nextest, 8 workers | 138.49s |
| 237-test Git-heavy module, APFS, 8 workers | 54.74s |
| Same module, HFS+ RAM disk, 8 workers | 64.41s |
| Same module, APFS, 10 workers | 60.41s |
| Same module, APFS, 6 workers | 69.00s |
| 790 unit/bin tests under nextest | 6.55s |
| 237-test module batched in one libtest process | 52.81s |

The RAM disk was non-journaled and still regressed performance, so APFS
journaling is not the dominant cause. During a sustained integration run,
`syspolicyd`, `trustd`, and the installed Endpoint Security extension consumed
substantial CPU. The locally built test and `stax` binaries are linker-signed
ad hoc but are not accepted by Gatekeeper. This makes repeated process launch
and inspection the primary native-specific cost.

Two clipboard fallback tests also fail natively because removing Linux display
variables does not disable the macOS pasteboard. They can overwrite the user's
real clipboard while claiming to test an unavailable clipboard.

## Goal

Provide a supported native macOS test path that:

- runs every discovered test successfully (1,838 tests at the measured
  baseline);
- has a median warm runtime of at most 75 seconds across three runs;
- has no individual warm run longer than 90 seconds;
- does not disable or bypass Gatekeeper, Endpoint Security, or other host
  security controls;
- preserves the existing Docker and CI nextest paths;
- continues to exercise the real `stax` executable in integration tests.

## Decisions

| Decision | Choice |
|---|---|
| Native runner | Hybrid: nextest for unit/bin tests, one controlled libtest process for the consolidated integration binary. |
| Native concurrency | Eight integration test threads, matching the measured optimum. |
| Docker and CI | Keep nextest process isolation and existing container behavior unchanged. |
| Fixture bootstrap | Use `git2` in-process for repository initialization and the initial commit. |
| Behavior under test | Continue launching the real `stax` binary and Git where the scenario explicitly exercises Git behavior. |
| Environment isolation | Configure child processes directly; do not mutate process-global environment variables in integration tests. |
| Clipboard isolation | Add a test-only `STAX_TEST_*` override that deterministically makes clipboard access unavailable. |
| Security controls | Do not remove provenance attributes, alter Gatekeeper policy, or request Endpoint Security exclusions. |
| Raw `cargo test` | Remains unsupported as a contributor workflow; only the guarded `make test-native` orchestration is sanctioned. |

## Architecture

### Native runner

Add a focused native test script invoked by `make test-native` and
`make test-local-fast` on macOS. It performs two phases:

1. Run unit and bin tests with nextest and the `test-container` Cargo profile.
2. Run the single `all_tests` integration binary through libtest with exactly
   eight test threads.

The script owns the sanitized environment currently assembled in the Makefile:
GitHub token variables are removed, update checks are disabled,
`STAX_TEST_TMPDIR` and `TMPDIR` point at the repository-local temporary root,
`RUST_MIN_STACK` is set, and the `test-container` profile is used. It raises
the process file-descriptor limit when the current soft limit is lower than
4,096, then reports the effective limit and fails before running tests if the
hard limit prevents a 4,096 soft limit. The current development machine already
has a higher limit; the guard protects contributors with the older macOS
default of 256 descriptors.

Linux native fallback, Docker, Apple Container, and CI retain nextest for all
tests. `make test` continues to select Docker on macOS when Docker is present;
`make test-native` is the explicit fast native path.

### Hermetic integration environment

The consolidated integration binary can only run tests concurrently if they do
not modify process-global state. Tests that currently call
`std::env::set_var` or `std::env::remove_var` will instead set or remove values
on the exact `Command` they spawn. Helpers will expose explicit methods for
token-bearing and token-free child environments so the intended state is
visible at each call site.

Clipboard fallback tests will set a narrowly named `STAX_TEST_*` environment
variable on their child `stax` command. The copy command will check this hook
before constructing `arboard::Clipboard` and return the same clipboard
unavailable error used for real backend failures. The hook is undocumented,
test-only infrastructure consistent with existing `STAX_TEST_*` controls and
does not change normal CLI behavior.

### In-process repository bootstrap

Extract repository bootstrap into a shared test fixture module used by both
`tests/common/mod.rs` and the legacy fixture inside
`tests/integration_tests.rs`. The initializer uses the existing `git2`
dependency to:

1. initialize a repository with `main` as the initial branch;
2. create the baseline worktree file;
3. add it to the index;
4. create the initial commit with a deterministic test signature;
5. leave `HEAD` attached to `main`.

This removes repeated `git init`, `git config`, `git add`, `git commit`, and
branch-renaming processes from every fixture while preserving the observable
repository state. Scenario helpers continue to use the Git CLI when a test is
specifically validating CLI-facing Git behavior, remotes, hooks, rebases, or
failure modes.

Fixture equivalence is tested explicitly: initial branch, clean status, initial
commit message, configured identity behavior, and compatibility with the
existing `TestRepo` methods.

## Data flow

```text
make test-native
  -> native test script
     -> sanitize environment and validate file limit
     -> cargo nextest run --lib --bins (8 workers)
     -> cargo test --test all_tests (one process, 8 libtest threads)
        -> shared git2 fixture bootstrap
        -> per-test temporary repository
        -> real stax child process
        -> Git/libgit2 behavior exercised by the scenario
```

No repository state is shared between tests. Batching changes process lifetime,
not fixture isolation.

## Error handling

- If unit/bin tests fail, the native runner stops before integration tests.
- If the file-descriptor limit cannot be raised to the required value, the
  runner exits with an actionable message rather than attempting a flaky run.
- If an integration test mutates process-global environment, a dedicated
  source check/test fails and identifies the call site.
- Clipboard test forcing is scoped to child commands and cannot affect the
  parent test runner or the user's clipboard.
- Fixture initialization errors include the failed bootstrap operation and
  temporary repository path.
- The native runner does not silently fall back to Docker or a slower profile.

## Performance implementation stages

Changes are applied and measured in this order:

1. Make clipboard and environment-sensitive integration tests hermetic.
2. Add the guarded hybrid native runner and benchmark it.
3. Replace fixture bootstrap subprocesses with the shared `git2` initializer
   and benchmark again.
4. If the median remains above 75 seconds, profile the hybrid runner and
   convert only additional high-volume fixture setup operations to `git2`.
   Production command execution and scenario-defining Git subprocesses remain
   out of scope.

Each stage must preserve correctness before its performance result is used.
Optimizations that regress the representative module or introduce flakes are
removed rather than accumulated.

## Tests and verification

### Correctness

- Clipboard fallback tests pass on macOS without changing the real clipboard.
- Token/environment isolation tests pass when integration tests share one
  process.
- Shared fixture bootstrap produces `main`, one initial commit, and a clean
  worktree.
- Fixture bootstrap failures return contextual errors.
- The native runner rejects an intentionally insufficient file limit with the
  expected guidance.
- The native runner propagates failures from both unit and integration phases.

### Targeted commands

Use nextest module filters during development. Use the guarded native runner
only after the environment and fixture changes are in place. Do not use raw
full-suite `cargo test` as a contributor command.

### Full verification

1. Run `make test` for the existing Docker full-suite path.
2. Run `make test-native` three times after a warm build.
3. Require every discovered test to pass on every run, with no reduction from
   the 1,838-test baseline.
4. Require a median runtime no greater than 75 seconds and no run greater than
   90 seconds.
5. Run formatting and lint checks required by the repository.

## Documentation

Update contributor-facing documentation in the same change:

- `README.md`: explain the fast native path and keep Docker as the default
  macOS full-suite path until the native performance gate is met.
- `CONTRIBUTING.md`: document `make test-native`, its requirements, and expected
  warm runtime.
- `AGENTS.md`: permit the guarded native runner while continuing to prohibit
  raw full-suite `cargo test`.
- `skills.md`: update test workflow guidance if it references the old native
  behavior.

## Out of scope

- Disabling journaling or moving tests to a RAM disk.
- Removing macOS provenance metadata or changing Gatekeeper policy.
- Requesting exclusions from Kandji or another Endpoint Security product.
- Replacing scenario-defining Git operations with mocks.
- Changing Docker, Apple Container, or CI test isolation.
- Reducing test coverage to meet the runtime target.
