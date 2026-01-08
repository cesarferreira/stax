//! Tests for the `stax ci` command
//!
//! These tests verify the CI status display functionality.

mod common;
use common::TestRepo;

/// Test that ci command shows "no tracked branches" when there are none
#[test]
fn test_ci_no_tracked_branches() {
    let repo = TestRepo::new();

    // Running ci on a repo with no tracked branches should show appropriate message
    // Note: This will fail with "GitHub token not set" before checking branches,
    // which is expected behavior
    let output = repo.run_stax(&["ci"]);
    let stderr = TestRepo::stderr(&output);
    let stdout = TestRepo::stdout(&output);

    // Either we get "no tracked branches" or "GitHub token not set" - both are valid
    assert!(
        stdout.contains("No tracked branches") || stderr.contains("GitHub token"),
        "Expected 'No tracked branches' or 'GitHub token' message, got stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}

/// Test that ci command requires GitHub token
#[test]
fn test_ci_requires_github_token() {
    let repo = TestRepo::new();

    // Create a branch to track
    repo.run_stax(&["bc", "feature"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Add feature");

    // ci should fail without a GitHub token (unless already set in env)
    let output = repo.run_stax(&["ci"]);
    let stderr = TestRepo::stderr(&output);
    let stdout = TestRepo::stdout(&output);

    // Should either request token, fail on remote info, or show no CI (if no PRs)
    let has_token_error = stderr.contains("GitHub token") || stderr.contains("GITHUB_TOKEN");
    let has_remote_error = stderr.contains("remote");
    let has_no_ci_output = stdout.contains("No CI") || stdout.contains("No tracked");
    let success = output.status.success();

    // Either fails asking for token/remote, or succeeds showing no CI checks
    assert!(
        has_token_error || has_remote_error || has_no_ci_output || success,
        "Expected token/remote error or no CI output, got stdout: {}, stderr: {}",
        stdout,
        stderr
    );
}

/// Test ci --json output format (when no branches/token)
#[test]
fn test_ci_json_format_structure() {
    let repo = TestRepo::new();

    // Create a tracked branch
    repo.run_stax(&["bc", "test-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test commit");

    // Try JSON output - will fail without token but tests the flag parsing
    let output = repo.run_stax(&["ci", "--json"]);

    // Should either produce JSON or fail with token error
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);

    // If it succeeds, stdout should be valid JSON array
    if output.status.success() && !stdout.trim().is_empty() {
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&stdout);
        assert!(parsed.is_ok(), "Expected valid JSON output: {}", stdout);
        let json = parsed.unwrap();
        assert!(json.is_array(), "Expected JSON array output");
    } else {
        // Should fail with token or remote error
        assert!(
            stderr.contains("GitHub") || stderr.contains("token") || stderr.contains("remote"),
            "Expected GitHub token or remote error, got: {}",
            stderr
        );
    }
}

/// Test ci --all flag is recognized
#[test]
fn test_ci_all_flag() {
    let repo = TestRepo::new();

    // Just verify the flag is accepted
    let output = repo.run_stax(&["ci", "--all"]);

    // Should not fail with "unrecognized flag" or similar
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stderr.contains("unrecognized") && !stderr.contains("unknown"),
        "Flag --all should be recognized: {}",
        stderr
    );
}

/// Test ci --refresh flag is recognized
#[test]
fn test_ci_refresh_flag() {
    let repo = TestRepo::new();

    // Just verify the flag is accepted
    let output = repo.run_stax(&["ci", "--refresh"]);

    // Should not fail with "unrecognized flag" or similar
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stderr.contains("unrecognized") && !stderr.contains("unknown"),
        "Flag --refresh should be recognized: {}",
        stderr
    );
}

/// Test multiple flags can be combined
#[test]
fn test_ci_combined_flags() {
    let repo = TestRepo::new();

    // Combine multiple flags
    let output = repo.run_stax(&["ci", "--all", "--json", "--refresh"]);

    // Should not fail with flag parsing errors
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stderr.contains("unrecognized") && !stderr.contains("unknown") && !stderr.contains("unexpected"),
        "Combined flags should be accepted: {}",
        stderr
    );
}
