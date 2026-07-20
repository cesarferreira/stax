# gh-stack OAuth Diagnostic Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Warn env-token users when `gh-stack` is installed but no keyring-backed OAuth account can run native Stack operations, without adding work to normal commands or normal doctor runs.

**Architecture:** Keep GitHub CLI process construction and credential classification in `src/github/gh_stack.rs`. Gate the new token-stripped `gh auth status` subprocess inside the installed-extension branch of `stax doctor`, and only when a non-empty `GH_TOKEN` or `GITHUB_TOKEN` exists. Cover the command boundary with fake-`gh` integration tests and document the new conditional warning.

**Tech Stack:** Rust, `std::process::Command`, clap-based stax CLI, existing integration-test `TestRepo` and fake shell scripts, Cargo nextest, Make lint/test targets.

## Global Constraints

- Normal stax commands perform no additional authentication probe.
- Doctor performs no additional authentication probe when both `GH_TOKEN` and `GITHUB_TOKEN` are missing or empty.
- The OAuth probe removes both token overrides and never prints captured auth output, usernames, scopes, or tokens.
- Missing/invalid OAuth is a non-blocking warning; probe execution failure is unknown and silent.
- Extension discovery/install/upgrade preserve token overrides; stack link/unstack remove them.
- Do not add new dependencies or broaden native-stack behavior beyond github.com.
- Use `cargo nextest run gh_stack_tests::` for targeted feedback, `make lint` for final lint, and `make test` for the full suite.

---

### Task 1: Specify the conditional OAuth diagnostic and command boundary

**Files:**
- Modify: `tests/gh_stack_tests.rs`

**Interfaces:**
- Consumes: existing `TestRepo::run_stax_with_env`, `fake_gh_dir`, `path_with_fake_gh`, `link_stack_with_env`, `unlink_stack_with_env`, `install_extension_with_env`, and `upgrade_extension_with_env`.
- Produces: regression expectations for the `OAuthLoginStatus` API and doctor warning added in Task 2.

- [ ] **Step 1: Extend the env-only doctor regression with a failing OAuth assertion**

Add this fake-`gh` arm, which distinguishes a leaked token override from the intended missing-keyring state:

```sh
"auth status")
  if [ -n "${GH_TOKEN:-}" ] || [ -n "${GITHUB_TOKEN:-}" ]; then
    echo "token override leaked into OAuth probe" >&2
    exit 5
  fi
  echo "no active OAuth account" >&2
  exit 1
  ;;
```

Then extend `doctor_detects_installed_gh_stack_with_env_only_auth` with:

```rust
assert!(
    stdout.contains("no usable OAuth-authenticated `gh` account"),
    "doctor should explain why native stack operations still cannot authenticate, stdout was:\n{stdout}"
);
```

- [ ] **Step 2: Add the OAuth-success regression**

Add this test; extension discovery requires the environment tokens, while the OAuth probe succeeds only after both have been removed:

```rust
#[test]
fn doctor_accepts_keyring_oauth_with_env_token_overrides() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.96.0"; exit 0 ;;
  "extension list")
    [ "${GH_TOKEN:-}" = "ghp_env_only" ] || exit 4
    [ "${GITHUB_TOKEN:-}" = "github_env_only" ] || exit 4
    echo "gh stack github/gh-stack v0.0.8"
    exit 0
    ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "auth status")
    [ -z "${GH_TOKEN:-}" ] || exit 5
    [ -z "${GITHUB_TOKEN:-}" ] || exit 5
    exit 0
    ;;
esac
exit 1
"#,
    );

    let output = repo.run_stax_with_env(
        &["doctor"],
        &[
            ("PATH", &path_with_fake_gh(fake.path())),
            ("GH_TOKEN", "ghp_env_only"),
            ("GITHUB_TOKEN", "github_env_only"),
        ],
    );

    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("gh-stack extension installed"), "stdout was:\n{stdout}");
    assert!(
        !stdout.contains("no usable OAuth-authenticated"),
        "valid keyring OAuth should not warn, stdout was:\n{stdout}"
    );
}
```

- [ ] **Step 3: Add the no-slowdown regression**

Add this test; the marker file makes an accidental extra process observable:

```rust
#[test]
fn doctor_skips_oauth_probe_without_token_overrides() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.96.0"; exit 0 ;;
  "extension list") echo "gh stack github/gh-stack v0.0.8"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "auth status") echo called > "$OAUTH_PROBE_FILE"; exit 1 ;;
esac
exit 1
"#,
    );
    let probe_file = fake.path().join("oauth-probe.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["doctor"],
        &[
            ("PATH", path.as_str()),
            ("OAUTH_PROBE_FILE", probe_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    assert!(
        !probe_file.exists(),
        "doctor must not add an OAuth subprocess without token overrides"
    );
}
```

- [ ] **Step 4: Add direct command-boundary regressions**

Add these four focused tests:

```rust
#[test]
fn unlink_stack_strips_injected_token_env_vars_before_calling_gh() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "stack unstack" ]; then
  printf 'GH_TOKEN=%s\nGITHUB_TOKEN=%s\n' "${GH_TOKEN:-unset}" "${GITHUB_TOKEN:-unset}" > "$ENV_DUMP_FILE"
  exit 0
fi
exit 1
"#,
    );
    let env_dump_file = fake.path().join("unlink-env.txt");
    let path = path_with_fake_gh(fake.path());

    let outcome = stax::github::gh_stack::unlink_stack_with_env(&[
        ("PATH", path.as_str()),
        ("ENV_DUMP_FILE", env_dump_file.to_str().unwrap()),
        ("GH_TOKEN", "ghp_should_be_stripped"),
        ("GITHUB_TOKEN", "github_should_be_stripped"),
    ]);

    assert_eq!(outcome, stax::github::gh_stack::LinkOutcome::Linked);
    assert_eq!(
        fs::read_to_string(env_dump_file).unwrap(),
        "GH_TOKEN=unset\nGITHUB_TOKEN=unset\n"
    );
}

#[test]
fn install_extension_preserves_injected_token_env_vars() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2 $3" = "extension install github/gh-stack" ]; then
  printf 'GH_TOKEN=%s\nGITHUB_TOKEN=%s\n' "${GH_TOKEN:-unset}" "${GITHUB_TOKEN:-unset}" > "$ENV_DUMP_FILE"
  exit 0
fi
exit 1
"#,
    );
    let env_dump_file = fake.path().join("install-env.txt");
    let path = path_with_fake_gh(fake.path());

    stax::github::gh_stack::install_extension_with_env(&[
        ("PATH", path.as_str()),
        ("ENV_DUMP_FILE", env_dump_file.to_str().unwrap()),
        ("GH_TOKEN", "ghp_install"),
        ("GITHUB_TOKEN", "github_install"),
    ])
    .expect("install extension");

    assert_eq!(
        fs::read_to_string(env_dump_file).unwrap(),
        "GH_TOKEN=ghp_install\nGITHUB_TOKEN=github_install\n"
    );
}

#[test]
fn upgrade_extension_preserves_injected_token_env_vars() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2 $3" = "extension upgrade gh-stack" ]; then
  printf 'GH_TOKEN=%s\nGITHUB_TOKEN=%s\n' "${GH_TOKEN:-unset}" "${GITHUB_TOKEN:-unset}" > "$ENV_DUMP_FILE"
  exit 0
fi
exit 1
"#,
    );
    let env_dump_file = fake.path().join("upgrade-env.txt");
    let path = path_with_fake_gh(fake.path());

    stax::github::gh_stack::upgrade_extension_with_env(&[
        ("PATH", path.as_str()),
        ("ENV_DUMP_FILE", env_dump_file.to_str().unwrap()),
        ("GH_TOKEN", "ghp_upgrade"),
        ("GITHUB_TOKEN", "github_upgrade"),
    ])
    .expect("upgrade extension");

    assert_eq!(
        fs::read_to_string(env_dump_file).unwrap(),
        "GH_TOKEN=ghp_upgrade\nGITHUB_TOKEN=github_upgrade\n"
    );
}

#[test]
fn oauth_status_is_unknown_when_gh_cannot_execute() {
    let empty_dir = TempDir::new().expect("empty temp dir");
    let status = stax::github::gh_stack::oauth_login_status_with_path(
        empty_dir.path().to_str().expect("utf8 path"),
    );

    assert_eq!(
        status,
        stax::github::gh_stack::OAuthLoginStatus::Unknown
    );
}
```

Use the existing env-dump pattern from `link_stack_strips_injected_token_env_vars_before_calling_gh`; each test invokes real `gh_stack` module code against the fake executable.

- [ ] **Step 5: Run the targeted tests and verify RED**

Run:

```bash
cargo nextest run gh_stack_tests::
```

Expected: compilation fails because `OAuthLoginStatus`/`oauth_login_status_with_path` do not exist, or the env-only doctor assertion fails because the warning is absent. Existing command-boundary tests may already pass because they lock down behavior merged in #648.

### Task 2: Implement the conditional token-stripped OAuth probe

**Files:**
- Modify: `src/github/gh_stack.rs`
- Modify: `src/commands/doctor.rs`
- Test: `tests/gh_stack_tests.rs`

**Interfaces:**
- Consumes: `AUTH_OVERRIDE_ENV_VARS`, `gh_stack_command`, and the tests from Task 1.
- Produces: `pub enum OAuthLoginStatus`, `pub fn auth_override_env_present() -> bool`, `pub fn oauth_login_status() -> OAuthLoginStatus`, and `pub fn oauth_login_status_with_path(path: &str) -> OAuthLoginStatus`.

- [ ] **Step 1: Add the OAuth status type and environment gate**

In `src/github/gh_stack.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthLoginStatus {
    Available,
    MissingOrInvalid,
    Unknown,
}

pub fn auth_override_env_present() -> bool {
    AUTH_OVERRIDE_ENV_VARS.iter().any(|name| {
        std::env::var_os(name).is_some_and(|value| !value.is_empty())
    })
}
```

- [ ] **Step 2: Add the token-stripped OAuth probe**

Use the existing command factory so the probe observes the same credentials as remote stack operations:

```rust
pub fn oauth_login_status() -> OAuthLoginStatus {
    oauth_login_status_with_env(&[])
}

pub fn oauth_login_status_with_path(path: &str) -> OAuthLoginStatus {
    oauth_login_status_with_env(&[("PATH", path)])
}

fn oauth_login_status_with_env(env: &[(&str, &str)]) -> OAuthLoginStatus {
    match gh_stack_command(env)
        .args(["auth", "status", "--active", "--hostname", "github.com"])
        .output()
    {
        Ok(output) if output.status.success() => OAuthLoginStatus::Available,
        Ok(_) => OAuthLoginStatus::MissingOrInvalid,
        Err(_) => OAuthLoginStatus::Unknown,
    }
}
```

- [ ] **Step 3: Render the conditional doctor warning**

Import `OAuthLoginStatus` in `src/commands/doctor.rs`. Inside `ExtensionStatus::Installed`, after the existing installed-extension line, add:

```rust
if gh_stack::auth_override_env_present()
    && gh_stack::oauth_login_status() == OAuthLoginStatus::MissingOrInvalid
{
    println!(
        "{} {}",
        "⚠".yellow(),
        "GitHub native stacks: GH_TOKEN/GITHUB_TOKEN can discover gh-stack, but no usable \
         OAuth-authenticated `gh` account was found (run `gh auth login` or `gh auth switch`)"
            .yellow()
    );
}
```

Do not increment `issues` or add a `RepairAction`; native stacks are optional and login is interactive.

- [ ] **Step 4: Run the targeted tests and verify GREEN**

Run:

```bash
cargo nextest run gh_stack_tests::
```

Expected: every `gh_stack_tests::` test passes, including the warning, no-probe, unknown, and command-boundary regressions.

- [ ] **Step 5: Run fast lint feedback**

Run:

```bash
make lint-fast
```

Expected: formatting and library/binary Clippy checks pass with no warnings.

- [ ] **Step 6: Commit the tested implementation**

```bash
git add src/github/gh_stack.rs src/commands/doctor.rs tests/gh_stack_tests.rs
git commit -m "fix: diagnose missing gh-stack OAuth login"
```

### Task 3: Document, fully verify, and publish with stax

**Files:**
- Modify: `README.md`
- Modify: `docs/integrations/github-native-stacks.md`
- Modify: `skills.md`
- Modify: `docs/superpowers/plans/2026-07-20-gh-stack-oauth-diagnostic.md` only if execution reveals a plan correction.

**Interfaces:**
- Consumes: the user-visible warning implemented in Task 2 and repository documentation policy.
- Produces: complete user/agent guidance and a stax-created GitHub pull request targeting `main`.

- [ ] **Step 1: Update user and agent documentation**

Add the same behavior statement to all three native-stack references:

```text
When GH_TOKEN or GITHUB_TOKEN is set, st doctor performs one token-stripped OAuth check and warns if gh stack would have no usable keyring login. Doctor skips this extra probe when neither override is set.
```

Keep README wording compact; put remediation detail (`gh auth login` / `gh auth switch`) in the integration guide and `skills.md`.

- [ ] **Step 2: Run documentation/diff hygiene checks**

Run:

```bash
git diff --check
```

Expected: exit 0 with no whitespace errors.

- [ ] **Step 3: Run final lint**

Run:

```bash
make lint
```

Expected: formatting and Clippy pass for all targets and features.

- [ ] **Step 4: Start Docker and run the required full suite**

Run:

```bash
open -a Docker
make test
```

Expected: Docker-backed full suite passes. If Docker reports its API socket is unavailable, report that exact blocker and retry after Docker Desktop is ready; do not fall back to native full-suite commands.

- [ ] **Step 5: Commit the documentation**

```bash
git add README.md docs/integrations/github-native-stacks.md skills.md docs/superpowers/plans/2026-07-20-gh-stack-oauth-diagnostic.md
git commit -m "docs: explain conditional gh-stack OAuth check"
```

- [ ] **Step 6: Inspect the stax branch and submission plan**

Run:

```bash
stax ls
stax submit --plan
```

Expected: `codex/doctor-gh-stack-oauth-diagnostic` is one branch above `main`, and the plan proposes pushing it and creating one PR against `main`.

- [ ] **Step 7: Create the PR using stax**

Run non-interactively with explicit PR text:

```bash
stax submit --yes --no-prompt \
  --title "fix: diagnose missing gh-stack OAuth login" \
  --body "## Summary

- warn env-token users when gh-stack lacks a usable keyring OAuth login
- preserve the common-path process count by probing only during doctor with token overrides
- add regressions for discovery/install/upgrade versus link/unstack token handling

## Testing

- cargo nextest run gh_stack_tests::
- make lint
- make test

## Documentation

- update README.md, docs/integrations/github-native-stacks.md, and skills.md"
```

Expected: stax pushes the branch, creates one PR targeting `main`, and records its PR metadata.

- [ ] **Step 8: Verify the published PR**

Run:

```bash
stax ll
stax ci --oneline
```

Expected: the current branch shows a GitHub PR URL/number; CI is either pending/running or successful, with no immediate failed checks.
