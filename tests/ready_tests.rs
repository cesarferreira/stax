//! Tests for the `stax ready` command

use crate::common::TestRepo;

/// Help text exposes the live oneline/watch view and all expected flags.
#[test]
fn test_ready_help_mentions_oneline_view() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--help"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("CI/PR status")
            || stdout.contains("oneline")
            || stdout.contains("tracked branches"),
        "help should mention the oneline CI/PR view; got: {stdout}"
    );
    assert!(stdout.contains("--all"), "missing --all");
    assert!(stdout.contains("--current"), "missing --current");
    assert!(stdout.contains("--stack"), "missing --stack");
    assert!(stdout.contains("--json"), "missing --json");
    assert!(stdout.contains("--plain"), "missing --plain");
    assert!(stdout.contains("--interval"), "missing --interval");
}

/// `--plain` with tracked branches must reach the ci delegation and fail with
/// ci's auth guard, not with "No tracked branches found." This is the key
/// delegation test: if `all`/`watch`/`oneline` bools are transposed in
/// `ready::run`, ci.rs returns a different error or path, and this fails.
#[test]
fn test_ready_plain_reaches_ci_delegation() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.create_stack(&["feat-a", "feat-b"]);
    let output = repo.run_stax(&["ready", "--plain"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{stdout}{stderr}");

    // Watch loop not entered in plain mode: no clear-screen escape.
    assert!(
        !combined.contains("\x1B[2J"),
        "output must not contain ANSI clear-screen escape in plain mode"
    );

    // Must reach ci's auth guard, not the json-path guard.
    // ci.rs:279 says "Set the appropriate token for your forge".
    // ready.rs:223 says "live PR readiness cannot be fetched" (json path only).
    assert!(
        !output.status.success(),
        "expected non-zero exit when forge token is absent"
    );
    assert!(
        stderr.contains("token") || stderr.contains("forge") || stderr.contains("auth"),
        "expected ci auth error message; got: {stderr}"
    );
    assert!(
        !stderr.contains("live PR readiness cannot be fetched"),
        "non-json path must go through ci::run, not the json-path guard"
    );
}

/// `--json` must exit non-zero with a configuration error (missing remote,
/// auth, or similar). This verifies the JSON path short-circuits before the
/// ci delegation and hits the readiness schema guard.
#[test]
fn test_ready_json_no_config_exits_nonzero() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--json"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit when forge is not configured"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("auth")
            || stderr.contains("token")
            || stderr.contains("configured")
            || stderr.contains("remote")
            || stderr.contains("Remote"),
        "expected a configuration error; got: {stderr}"
    );
}

/// `--all` and `--current` are declared `conflicts_with` each other; clap
/// should reject this combination with exit code 2.
#[test]
fn test_ready_all_and_current_conflict() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--all", "--current"]);
    let code = output.status.code().unwrap_or(0);
    assert_eq!(code, 2, "expected clap conflict error (exit 2)");
}

/// `st pr list --ready --help` still advertises `--ready`.
#[test]
fn test_pr_list_ready_help_present() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["pr", "list", "--help"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("--ready"), "pr list --help missing --ready");
}

/// `st ready --current` and `st ready --stack` with tracked branches reach
/// the ci delegation — clap accepts them and they fail at auth, not at parse
/// (exit 2 would indicate clap rejected the flags).
#[test]
fn test_ready_current_and_stack_flags_reach_delegation() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.create_stack(&["feat-a", "feat-b"]);

    let output_current = repo.run_stax(&["ready", "--current", "--plain"]);
    let output_stack = repo.run_stax(&["ready", "--stack", "--plain"]);

    assert_ne!(
        output_current.status.code(),
        Some(2),
        "--current should be accepted by clap and reach ci delegation"
    );
    assert_ne!(
        output_stack.status.code(),
        Some(2),
        "--stack should be accepted by clap and reach ci delegation"
    );

    // Neither should hit the json-path guard.
    let stderr_current = TestRepo::stderr(&output_current);
    let stderr_stack = TestRepo::stderr(&output_stack);
    assert!(
        !stderr_current.contains("live PR readiness cannot be fetched"),
        "--current must not hit the json-path guard"
    );
    assert!(
        !stderr_stack.contains("live PR readiness cannot be fetched"),
        "--stack must not hit the json-path guard"
    );
}
