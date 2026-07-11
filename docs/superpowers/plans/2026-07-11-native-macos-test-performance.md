# Native macOS Test Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** Experiment concluded; the original performance gate was not met. The user authorized a draft PR containing only the safe partial improvements.

**Outcome:** The hybrid and sharded libtest runners were rejected after unstable
measurements ranging from 73.29 to 136.17 seconds. The retained runner uses
nextest for the complete suite. The final retained native path passed all 1,843
tests in 115.58 seconds; after adding the final timeout regression test, Docker
passed all 1,844 tests in 35.861 seconds of test execution and remains
recommended. The unchecked steps below are the historical
execution plan, not claims that the original acceptance gate passed.

**Original goal (not met):** Make the guarded native macOS full-suite path run every discovered test with a median warm runtime of at most 75 seconds while preserving Docker/CI nextest behavior.

**Original architecture hypothesis (rejected):** Keep nextest for the 790 fast unit/bin tests, but run the consolidated integration binary once with eight libtest threads to avoid per-test macOS security inspection. The retained implementation instead uses guarded nextest for the full suite plus the safe fixture and isolation improvements.

**Tech Stack:** Rust 1.96, cargo-nextest 0.9.114, libtest, git2 0.21, Bash, GNU Make, stax 0.94.

## Global Constraints

- Every discovered test must pass; the suite must never drop below the 1,838-test baseline.
- Three warm `make test-native` runs must have a median of at most 75 seconds and no run above 90 seconds.
- Native integration concurrency is exactly eight threads unless `NEXTEST_TEST_THREADS` explicitly overrides it.
- Do not disable or bypass Gatekeeper, Endpoint Security, provenance checks, or journaling.
- Docker, Apple Container, Linux native fallback, and CI keep nextest process isolation.
- Integration scenarios continue to execute the real `stax` binary and scenario-defining Git operations.
- Raw full-suite `cargo test` remains unsupported; only the guarded `make test-native` orchestration may batch integration tests.
- Keep changes focused on test correctness, fixture setup, native orchestration, and required contributor documentation.

---

### Task 1: Make clipboard fallback deterministic and harmless

**Files:**
- Modify: `tests/copy_tests.rs:5-28`
- Modify: `src/commands/copy.rs:68-83`

**Interfaces:**
- Consumes: existing `STAX_TEST_*` convention and `write_to_clipboard(&str) -> anyhow::Result<()>`.
- Produces: child-process override `STAX_TEST_FORCE_CLIPBOARD_UNAVAILABLE=1`.

- [ ] **Step 1: Add the failing native integration expectation**

Update `run_copy_without_clipboard` so the child process explicitly requests the unavailable path:

```rust
cmd.args(args)
    .current_dir(repo.path())
    .env("HOME", home)
    .env("GIT_CONFIG_GLOBAL", null_path)
    .env("GIT_CONFIG_SYSTEM", null_path)
    .env("STAX_DISABLE_UPDATE_CHECK", "1")
    .env("STAX_TEST_DISABLE_HEAD_SYNC", "1")
    .env("STAX_TEST_FORCE_CLIPBOARD_UNAVAILABLE", "1")
    .env_remove("DISPLAY")
    .env_remove("WAYLAND_DISPLAY")
    .env_remove("XDG_SESSION_TYPE")
    .env_remove("GITHUB_TOKEN")
    .env_remove("STAX_GITHUB_TOKEN")
    .env_remove("GH_TOKEN")
    .env_remove("STAX_SHELL_INTEGRATION");
```

- [ ] **Step 2: Run the clipboard tests and verify the new expectation fails on macOS**

Run:

```bash
cargo nextest run copy_tests::
```

Expected: the two fallback tests fail because `stax copy` still reaches the native pasteboard and prints `copied to clipboard`.

- [ ] **Step 3: Implement the minimal clipboard test hook**

Add the guard at the start of `write_to_clipboard`:

```rust
fn write_to_clipboard(text: &str) -> Result<()> {
    if std::env::var_os("STAX_TEST_FORCE_CLIPBOARD_UNAVAILABLE").is_some() {
        anyhow::bail!("clipboard unavailable by test request");
    }

    let mut clipboard =
        Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to access clipboard: {}", e))?;
    clipboard
        .set_text(text)
        .map_err(|e| anyhow::anyhow!("Failed to copy to clipboard: {}", e))?;
    Ok(())
}
```

- [ ] **Step 4: Verify clipboard happy and fallback paths**

Run:

```bash
cargo nextest run copy_tests:: commands::copy::tests::
```

Expected: all copy integration and unit tests pass, and the integration tests no longer touch the user's clipboard.

- [ ] **Step 5: Commit the clipboard isolation change**

```bash
git add src/commands/copy.rs tests/copy_tests.rs
git commit -m "test: isolate native clipboard fallback"
```

---

### Task 2: Remove process-global environment mutation from integration tests

**Files:**
- Modify: `src/github/gh_stack.rs:340-351`
- Modify: `tests/gh_stack_tests.rs:420-460`
- Modify: `tests/track_all_prs_tests.rs:40-52`
- Modify: `tests/integration_tests.rs:7456-7465`
- Modify: `scripts/lint.sh:1-5`

**Interfaces:**
- Consumes: `link_stack_with_env(..., env: &[(&str, &str)])` and per-command environment APIs.
- Produces: lint invariant that `tests/**/*.rs` never calls process-global `env::set_var` or `env::remove_var`.

- [ ] **Step 1: Add the failing lint invariant**

Insert this guard before the Clippy invocation in `scripts/lint.sh`:

```bash
global_env_mutations="$(rg -n '(std::)?env::(set_var|remove_var)' tests --glob '*.rs' || true)"
if [[ -n "${global_env_mutations}" ]]; then
  echo "integration tests must configure child commands instead of mutating process-global environment" >&2
  echo "${global_env_mutations}" >&2
  exit 1
fi
```

- [ ] **Step 2: Run the invariant and verify it reports every current mutation**

Run:

```bash
./scripts/lint.sh
```

Expected: FAIL before Clippy, listing `gh_stack_tests.rs`, `track_all_prs_tests.rs`, and `integration_tests.rs`.

- [ ] **Step 3: Make injected gh-stack auth variables removable per command**

Change `gh_command` so explicit test environment is applied before auth overrides are removed:

```rust
fn gh_command(env: &[(&str, &str)]) -> Command {
    let mut command = Command::new("gh");
    for (key, value) in env {
        command.env(key, value);
    }
    for var in AUTH_OVERRIDE_ENV_VARS {
        command.env_remove(var);
    }
    command
}
```

Rename the test to `link_stack_strips_injected_token_env_vars_before_calling_gh`
and replace the global mutation with explicit entries:

```rust
let outcome = stax::github::gh_stack::link_stack_with_env(
    &[10, 20],
    "main",
    "origin",
    &[
        ("PATH", path.as_str()),
        ("ENV_DUMP_FILE", env_dump_file.to_str().unwrap()),
        ("GH_TOKEN", "ghp_should_be_stripped"),
        ("GITHUB_TOKEN", "ghp_should_also_be_stripped"),
    ],
);
```

Delete the surrounding `std::env::set_var` and `std::env::remove_var` blocks.

- [ ] **Step 4: Remove redundant global token mutation from binary integration tests**

In `test_track_all_prs_no_token`, remove both `std::env::remove_var` calls because `TestRepo::run_stax` already removes both tokens from the exact child command.

In `setup_mock_github`, remove the unused process-global assignment:

```rust
async fn setup_mock_github() -> (TestRepo, MockServer) {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let repo = TestRepo::new_with_remote();
    (repo, mock_server)
}
```

- [ ] **Step 5: Verify the affected behavior and the lint invariant**

Run:

```bash
cargo nextest run gh_stack_tests:: track_all_prs_tests:: integration_tests::forge_mock_tests::test_submit_with_mock_pr_creation
./scripts/lint.sh
```

Expected: targeted tests pass; lint reaches and passes Clippy with no process-global environment matches under `tests/`.

- [ ] **Step 6: Commit shared-process environment safety**

```bash
git add src/github/gh_stack.rs tests/gh_stack_tests.rs tests/track_all_prs_tests.rs tests/integration_tests.rs scripts/lint.sh
git commit -m "test: make integration environments process-local"
```

---

### Task 3: Experiment with a guarded hybrid native runner (rejected)

The steps in this task record the tested hypothesis. The final draft reverts
to one guarded full-suite nextest invocation because the hybrid path was slower
and less stable under sustained endpoint inspection.

**Files:**
- Create: `scripts/native-tests.sh`
- Create: `scripts/native-tests-tests.sh`
- Modify: `Makefile:1-72`

**Interfaces:**
- Consumes: `NEXTEST_TEST_THREADS`, `NATIVE_CARGO_PROFILE`, `STAX_TEST_TMPDIR`, and `TMPDIR`.
- Produces: `scripts/native-tests.sh`; Make targets `test-native-script`, `test-native`, and the macOS branch of `test-local-fast`.

- [ ] **Step 1: Write shell-level tests for command order, environment sanitation, failure propagation, and file limits**

Create `scripts/native-tests-tests.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runner="${root}/scripts/native-tests.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

fake_cargo="${tmp}/fake-cargo"
cat >"${fake_cargo}" <<'FAKE'
#!/usr/bin/env bash
set -euo pipefail
if env | grep -Eq '^(GITHUB_TOKEN|STAX_GITHUB_TOKEN|GH_TOKEN)='; then
  echo "GitHub token environment leaked into cargo" >&2
  exit 90
fi
printf '%s\n' "$*" >>"${STAX_NATIVE_TEST_LOG}"
if [[ "${STAX_NATIVE_TEST_FAIL_PHASE:-}" == "${1:-}" ]]; then
  exit 23
fi
exit 0
FAKE
chmod +x "${fake_cargo}"

log="${tmp}/cargo.log"
GITHUB_TOKEN=secret STAX_GITHUB_TOKEN=secret GH_TOKEN=secret \
  STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
  STAX_NATIVE_TEST_LOG="${log}" \
  STAX_TEST_TMPDIR="${tmp}/test-tmp" \
  TMPDIR="${tmp}/test-tmp" \
  NEXTEST_TEST_THREADS=8 \
  NATIVE_CARGO_PROFILE=test-container \
  "${runner}"

expected="${tmp}/expected.log"
printf '%s\n' \
  'nextest run --lib --bins --cargo-profile test-container' \
  'test --profile test-container --test all_tests -- --test-threads=8' \
  >"${expected}"
diff -u "${expected}" "${log}"

: >"${log}"
set +e
STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
  STAX_NATIVE_TEST_LOG="${log}" \
  STAX_NATIVE_TEST_FAIL_PHASE=nextest \
  STAX_TEST_TMPDIR="${tmp}/test-tmp" \
  "${runner}"
status=$?
set -e
[[ "${status}" -eq 23 ]]
[[ "$(wc -l <"${log}" | tr -d ' ')" -eq 1 ]]

: >"${log}"
set +e
STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
  STAX_NATIVE_TEST_LOG="${log}" \
  STAX_NATIVE_TEST_FAIL_PHASE=test \
  STAX_TEST_TMPDIR="${tmp}/test-tmp" \
  "${runner}"
status=$?
set -e
[[ "${status}" -eq 23 ]]
[[ "$(wc -l <"${log}" | tr -d ' ')" -eq 2 ]]

set +e
(
  ulimit -Sn 64
  ulimit -Hn 64
  STAX_NATIVE_TEST_CARGO="${fake_cargo}" \
    STAX_NATIVE_TEST_LOG="${tmp}/limit.log" \
    STAX_NATIVE_TEST_REQUIRED_NOFILE=4096 \
    STAX_TEST_TMPDIR="${tmp}/test-tmp" \
    "${runner}"
) >"${tmp}/limit.stdout" 2>"${tmp}/limit.stderr"
status=$?
set -e
[[ "${status}" -eq 2 ]]
grep -q 'requires a file-descriptor limit of at least 4096' "${tmp}/limit.stderr"
[[ ! -e "${tmp}/limit.log" ]]

echo "native test runner checks passed"
```

- [ ] **Step 2: Make the shell test executable and verify it fails because the runner is absent**

Run:

```bash
chmod +x scripts/native-tests-tests.sh
./scripts/native-tests-tests.sh
```

Expected: FAIL because `scripts/native-tests.sh` does not exist.

- [ ] **Step 3: Implement the guarded native runner**

Create `scripts/native-tests.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${root}"

required_nofile="${STAX_NATIVE_TEST_REQUIRED_NOFILE:-4096}"
threads="${NEXTEST_TEST_THREADS:-8}"
profile="${NATIVE_CARGO_PROFILE:-test-container}"
test_tmpdir="${STAX_TEST_TMPDIR:-${root}/.test-tmp}"
cargo_cmd="${STAX_NATIVE_TEST_CARGO:-cargo}"

soft_nofile="$(ulimit -Sn)"
hard_nofile="$(ulimit -Hn)"

if [[ "${soft_nofile}" != "unlimited" && "${soft_nofile}" -lt "${required_nofile}" ]]; then
  if [[ "${hard_nofile}" != "unlimited" && "${hard_nofile}" -lt "${required_nofile}" ]]; then
    echo "native tests require a file-descriptor limit of at least ${required_nofile}; hard limit is ${hard_nofile}" >&2
    exit 2
  fi
  if ! ulimit -Sn "${required_nofile}"; then
    echo "failed to raise the native test file-descriptor limit to ${required_nofile}" >&2
    exit 2
  fi
fi

mkdir -p "${test_tmpdir}"
unset GITHUB_TOKEN STAX_GITHUB_TOKEN GH_TOKEN
export STAX_DISABLE_UPDATE_CHECK=1
export STAX_TEST_TMPDIR="${test_tmpdir}"
export TMPDIR="${TMPDIR:-${test_tmpdir}}"
export NEXTEST_TEST_THREADS="${threads}"
export RUST_MIN_STACK="${RUST_MIN_STACK:-4194304}"

echo "native tests: profile=${profile} threads=${threads} nofile=$(ulimit -Sn)"
"${cargo_cmd}" nextest run --lib --bins --cargo-profile "${profile}"
"${cargo_cmd}" test --profile "${profile}" --test all_tests -- --test-threads="${threads}"
```

- [ ] **Step 4: Make the runner executable and verify all shell-level tests pass**

Run:

```bash
chmod +x scripts/native-tests.sh
./scripts/native-tests-tests.sh
bash -n scripts/native-tests.sh scripts/native-tests-tests.sh
```

Expected: `native test runner checks passed`; Bash syntax check exits zero.

- [ ] **Step 5: Wire the runner into the Makefile without changing Docker or non-macOS behavior**

Add `test-native-script` to `.PHONY`, then use:

```make
test-native-script:
	./scripts/native-tests-tests.sh

test-native: test-native-script
	$(MAKE) test-local-fast

test-local-fast:
	mkdir -p .test-tmp
	@if [ "$$(uname)" = "Darwin" ]; then \
		NEXTEST_TEST_THREADS="$${NEXTEST_TEST_THREADS:-$(MAC_LOCAL_TEST_THREADS)}" \
		NATIVE_CARGO_PROFILE="$(NATIVE_CARGO_PROFILE)" \
		STAX_TEST_TMPDIR="$$(pwd)/.test-tmp" \
		TMPDIR="$$(pwd)/.test-tmp" \
		./scripts/native-tests.sh; \
	else \
		threads="$${NEXTEST_TEST_THREADS:-num-cpus}"; \
		env -u GITHUB_TOKEN -u STAX_GITHUB_TOKEN -u GH_TOKEN \
			STAX_DISABLE_UPDATE_CHECK=1 \
			STAX_TEST_TMPDIR="$$(pwd)/.test-tmp" \
			TMPDIR="$$(pwd)/.test-tmp" \
			NEXTEST_TEST_THREADS="$$threads" \
			RUST_MIN_STACK=4194304 \
			cargo nextest run --cargo-profile "$(NATIVE_CARGO_PROFILE)"; \
	fi
```

Do not change `test`, `test-docker`, `test-container`, or the container runner macro.

- [ ] **Step 6: Run the new native path once for correctness and record the warm runtime**

Run:

```bash
/usr/bin/time -p make test-native
```

Expected: shell runner checks pass, all unit/bin and integration tests pass, and the output reports the effective profile, thread count, and file limit. Record the wall-clock result for comparison; do not claim the 75-second gate until three warm runs complete.

- [ ] **Step 7: Commit the guarded native runner**

```bash
git add Makefile scripts/native-tests.sh scripts/native-tests-tests.sh
git commit -m "test: batch native macOS integration runs"
```

---

### Task 4: Replace repository bootstrap subprocesses with git2

**Files:**
- Create: `tests/common/git_fixture.rs`
- Modify: `tests/common/mod.rs:1-14,143-191,800-835`
- Modify: `tests/integration_tests.rs:1-110`

**Interfaces:**
- Consumes: `git2::RepositoryInitOptions`, `git2::Signature`, existing `git2` dependency.
- Produces: `pub(crate) fn init_test_repo(path: &Path) -> anyhow::Result<()>`.

- [ ] **Step 1: Write fixture equivalence and error-path tests before the initializer exists**

Create `tests/common/git_fixture.rs` with only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::init_test_repo;
    use tempfile::{NamedTempFile, tempdir};

    #[test]
    fn initializes_clean_main_repo_with_deterministic_identity() {
        let dir = tempdir().expect("temp dir");
        init_test_repo(dir.path()).expect("initialize fixture");

        let repo = git2::Repository::open(dir.path()).expect("open fixture");
        assert_eq!(repo.head().unwrap().shorthand(), Some("main"));
        assert_eq!(repo.head().unwrap().peel_to_commit().unwrap().message(), Some("Initial commit"));
        assert!(repo.statuses(None).unwrap().is_empty());

        let config = repo.config().unwrap();
        assert_eq!(config.get_string("user.name").unwrap(), "Test User");
        assert_eq!(config.get_string("user.email").unwrap(), "test@test.com");
    }

    #[test]
    fn reports_repository_path_when_initialization_fails() {
        let file = NamedTempFile::new().expect("temp file");
        let error = init_test_repo(file.path()).unwrap_err().to_string();
        assert!(error.contains("initialize test repository"));
        assert!(error.contains(&file.path().display().to_string()));
    }
}
```

Declare `mod git_fixture;` in `tests/common/mod.rs`.

- [ ] **Step 2: Run the fixture tests and verify compilation fails**

Run:

```bash
cargo nextest run common::git_fixture::tests::
```

Expected: FAIL to compile because `init_test_repo` is not defined.

- [ ] **Step 3: Implement the git2 initializer above the tests**

Add:

```rust
use anyhow::{Context, Result};
use git2::{Repository, RepositoryInitOptions, Signature};
use std::fs;
use std::path::Path;

pub(crate) fn init_test_repo(path: &Path) -> Result<()> {
    let mut options = RepositoryInitOptions::new();
    options.initial_head("main");
    let repo = Repository::init_opts(path, &options).with_context(|| {
        format!("initialize test repository at {}", path.display())
    })?;

    let mut config = repo.config().context("open test repository config")?;
    config.set_str("user.name", "Test User")?;
    config.set_str("user.email", "test@test.com")?;
    config.set_bool("commit.gpgSign", false)?;

    fs::write(path.join("README.md"), "# Test Repo\n")
        .context("write test repository README")?;
    let mut index = repo.index().context("open test repository index")?;
    index.add_path(Path::new("README.md"))?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let signature = Signature::now("Test User", "test@test.com")?;
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "Initial commit",
        &tree,
        &[],
    )?;
    Ok(())
}
```

- [ ] **Step 4: Verify fixture unit tests pass**

Run:

```bash
cargo nextest run common::git_fixture::tests::
```

Expected: both happy-path and invalid-path tests pass.

- [ ] **Step 5: Use the initializer in both TestRepo constructors**

Re-export the helper in `tests/common/mod.rs`:

```rust
mod git_fixture;
pub(crate) use git_fixture::init_test_repo;
```

Replace the five-process bootstrap in `tests/common/mod.rs::TestRepo::new` with:

```rust
pub fn new() -> Self {
    let dir = test_tempdir();
    init_test_repo(dir.path()).expect("Failed to initialize test repository");
    Self {
        dir,
        home_dir: test_tempdir(),
        remote_dir: None,
    }
}
```

Import and use the same helper in `tests/integration_tests.rs`:

```rust
use crate::common::init_test_repo;

fn new() -> Self {
    let dir = test_tempdir();
    init_test_repo(dir.path()).expect("Failed to initialize test repository");
    Self {
        dir,
        remote_dir: None,
    }
}
```

- [ ] **Step 6: Verify representative fixture consumers and the full native runner**

Run:

```bash
cargo nextest run common::tests:: integration_tests:: create_below_tests:: worktree_tests::
/usr/bin/time -p make test-native
```

Expected: targeted fixture, remote, branch, and worktree scenarios pass; the full native runner passes every discovered test and is faster than the Task 3 measurement.

- [ ] **Step 7: Commit the shared bootstrap optimization**

```bash
git add tests/common/git_fixture.rs tests/common/mod.rs tests/integration_tests.rs
git commit -m "perf(test): initialize fixtures with git2"
```

---

### Task 5: Optimize repeated fixture commits only if the 75-second median is not met

**Files:**
- Modify: `tests/common/git_fixture.rs`
- Modify: `tests/common/mod.rs:480-520`
- Modify: `tests/integration_tests.rs:320-365`

**Interfaces:**
- Consumes: repositories created by `init_test_repo` and deterministic local identity config.
- Produces: `pub(crate) fn commit_all(path: &Path, message: &str) -> anyhow::Result<git2::Oid>`.

- [ ] **Step 1: Measure three warm native runs after Task 4**

Run three separate times and record each `real` value:

```bash
/usr/bin/time -p make test-native
```

If the median is at most 75 seconds and no run exceeds 90 seconds, mark the remaining steps in this task skipped with the three measured values. Otherwise continue.

- [ ] **Step 2: Add failing commit helper tests**

Add to `tests/common/git_fixture.rs`:

```rust
#[test]
fn commits_added_modified_and_deleted_files() {
    let dir = tempdir().expect("temp dir");
    init_test_repo(dir.path()).expect("initialize fixture");
    std::fs::write(dir.path().join("added.txt"), "added\n").unwrap();
    std::fs::write(dir.path().join("README.md"), "changed\n").unwrap();
    std::fs::remove_file(dir.path().join("README.md")).unwrap();

    let oid = super::commit_all(dir.path(), "fixture update").expect("commit fixture");
    let repo = git2::Repository::open(dir.path()).unwrap();
    assert_eq!(repo.find_commit(oid).unwrap().message(), Some("fixture update"));
    assert!(repo.statuses(None).unwrap().is_empty());
}

#[test]
fn commit_all_rejects_an_empty_change() {
    let dir = tempdir().expect("temp dir");
    init_test_repo(dir.path()).expect("initialize fixture");
    let error = super::commit_all(dir.path(), "empty").unwrap_err().to_string();
    assert!(error.contains("no fixture changes to commit"));
}
```

- [ ] **Step 3: Run the commit helper tests and verify they fail to compile**

Run:

```bash
cargo nextest run common::git_fixture::tests::
```

Expected: FAIL because `commit_all` is undefined.

- [ ] **Step 4: Implement in-process add-all and commit**

Add:

```rust
pub(crate) fn commit_all(path: &Path, message: &str) -> Result<git2::Oid> {
    let repo = Repository::open(path)
        .with_context(|| format!("open test repository at {}", path.display()))?;
    let parent = repo.head()?.peel_to_commit()?;
    let mut index = repo.index()?;
    index.add_all(["*"], git2::IndexAddOption::DEFAULT, None)?;
    index.update_all(["*"], None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    if tree.id() == parent.tree_id() {
        anyhow::bail!("no fixture changes to commit");
    }
    let signature = Signature::now("Test User", "test@test.com")?;
    Ok(repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        message,
        &tree,
        &[&parent],
    )?)
}
```

Re-export `commit_all` from `tests/common/mod.rs`, import it in the legacy
module with `use crate::common::{commit_all, init_test_repo};`, and replace
only the plain fixture `TestRepo::commit` implementations in
`tests/common/mod.rs` and `tests/integration_tests.rs`. Do not replace helpers
used to exercise hooks, signing failures, merge behavior, or explicit Git
error paths.

- [ ] **Step 5: Verify commit semantics and full native performance**

Run:

```bash
cargo nextest run common::git_fixture::tests:: create_rollback_tests:: modify_tests:: integration_tests::
/usr/bin/time -p make test-native
```

Expected: commit helper tests and representative behavior tests pass; full native run improves without losing coverage.

If three subsequent warm runs still have a median above 75 seconds or any run
above 90 seconds, stop before documentation and PR submission, return to
systematic profiling with the new runner, and revise this plan from the fresh
profile. Do not weaken the gate or add unmeasured optimizations.

- [ ] **Step 6: Commit the conditional fixture commit optimization**

```bash
git add tests/common/git_fixture.rs tests/common/mod.rs tests/integration_tests.rs
git commit -m "perf(test): commit fixture changes in process"
```

---

### Task 6: Document the supported native path

**Final deviation:** Documentation describes a guarded nextest fallback and
explicitly keeps Docker recommended; it does not claim the rejected hybrid
runner or a passing 75-second gate.

**Files:**
- Modify: `README.md:430-440`
- Modify: `CONTRIBUTING.md:7-60`
- Modify: `AGENTS.md:3-20`
- Review only: `skills.md`

**Interfaces:**
- Consumes: final measured native behavior and guarded Make targets.
- Produces: contributor guidance that distinguishes `make test`, `make test-native`, and unsupported raw `cargo test`.

- [ ] **Step 1: Update README contributor guidance**

Replace the sentence after the `make test` block with:

```markdown
On macOS, `make test` uses Docker when available. Use `make test-native` for the
guarded native path: unit/bin tests run under nextest, while the consolidated
integration binary runs once with bounded concurrency to avoid macOS
per-process security overhead. Linux and container paths continue to use
nextest for the full suite. Do not run the raw full suite with `cargo test`.
```

- [ ] **Step 2: Update CONTRIBUTING commands and expectations**

Describe `make test-native` as the supported guarded native runner, state that
it requires Bash and a soft file-descriptor limit of 4,096 (raised by the
runner when the hard limit permits), and document the measured warm median from
Task 5. Keep Docker recommended for the default `make test` path until all
three native measurements meet the performance gate.

Use this command block:

````markdown
```bash
# Full suite; preferred and Docker-backed on macOS when available
make test

# Guarded native macOS suite
make test-native

# Targeted iteration remains process-isolated
cargo nextest run module_name::test_name
```
````

- [ ] **Step 3: Update AGENTS.md policy without blessing raw cargo test**

State explicitly:

```markdown
- `make test-native` is the only sanctioned batched native macOS full-suite
  path. It runs unit/bin tests with nextest and the consolidated integration
  binary with eight libtest threads, after sanitizing environment and checking
  the file-descriptor limit.
- Do not run the raw full suite via `cargo test`; the Make target owns the
  concurrency and environment safeguards.
```

Keep `make test` as the required final validation command and Docker as its
default macOS route.

- [ ] **Step 4: Confirm skills.md needs no change**

Run:

```bash
rg -n 'make test|test-native|test-local-fast|full suite|cargo test' skills.md
```

Expected: no contributor/native test workflow entry exists. Record in the PR
body: `skills.md unchanged because it documents the stax CLI command map, not
repository contributor test orchestration.`

- [ ] **Step 5: Verify documentation consistency and commit**

Run:

```bash
rg -n 'make test-native|raw full suite|eight libtest threads|4,096' README.md CONTRIBUTING.md AGENTS.md
git diff --check
```

Expected: each document describes the same guarded path and raw `cargo test`
restriction; no whitespace errors.

```bash
git add README.md CONTRIBUTING.md AGENTS.md
git commit -m "docs: document fast native macOS tests"
```

---

### Task 7: Run final quality, correctness, and performance gates

**Recorded result:** Formatting/lint passed, Docker passed the final 1,844-test
suite, and the guarded native nextest path passed the then-current 1,843 tests
in 115.58 seconds. The performance gate failed, so submission is an explicitly
authorized draft PR, not completion of the original goal.

**Files:**
- Modify only if verification exposes a defect in files already listed above.
- Update: `docs/superpowers/plans/2026-07-11-native-macos-test-performance.md` checkbox state.

**Interfaces:**
- Consumes: completed implementation and repository test policy.
- Produces: fresh evidence for the stax PR body.

- [ ] **Step 1: Run formatting and lint verification**

Run:

```bash
cargo fmt -- --check
make lint
git diff --check
```

Expected: all commands exit zero.

- [ ] **Step 2: Run the required Docker full suite**

Run:

```bash
make test
```

Expected: the full Docker suite passes with every discovered test. If Docker is unavailable, start Docker Desktop and retry; do not substitute another full-suite command.

- [ ] **Step 3: Warm the native artifacts once**

Run:

```bash
make test-native
```

Expected: every discovered test passes. This warm-up is not one of the three measured runs.

- [ ] **Step 4: Run and record three native performance measurements**

Run three separate times:

```bash
/usr/bin/time -p make test-native
```

Expected: every run passes all discovered tests; median `real` time is at most 75 seconds; no run exceeds 90 seconds. Record the three values and median in the PR body.

- [ ] **Step 5: Review the complete branch before publishing**

Invoke `superpowers:requesting-code-review`, inspect every finding, and resolve all correctness, regression, or test-policy issues. Re-run the affected targeted checks after any change.

- [ ] **Step 6: Commit the completed plan checklist**

Mark completed plan checkboxes, then commit the execution record:

```bash
git add docs/superpowers/plans/2026-07-11-native-macos-test-performance.md
git commit -m "docs: complete native test performance plan"
```

- [ ] **Step 7: Check stack and branch state**

Run:

```bash
git status --short
stax validate
stax status
git log --oneline main..HEAD
```

Expected: clean worktree, valid metadata for `codex/native-macos-test-performance`, and only the focused design/implementation/documentation commits above `main`.

- [ ] **Step 8: Submit the completed branch as a stax PR**

Run:

```bash
stax submit --yes --no-prompt
```

Expected: branch is pushed and a PR targeting `main` is created or updated. The PR body must include the Docker result, three native timings and median, the two rejected filesystem/concurrency experiments, and the `skills.md` no-change note. Do not add agent attribution.
