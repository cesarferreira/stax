//! Comments command integration tests
//!
//! Tests for the `comments` command that shows PR comments.
//! Note: Full functionality requires GitHub API access, so we test
//! pre-condition validation that exits before API calls.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Help Tests
// =============================================================================

#[test]
fn test_comments_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["comments", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("comments"));
    assert!(stdout.contains("PR"));
}

// =============================================================================
// Error Case Tests (validation before API calls)
// =============================================================================

#[test]
fn test_comments_on_trunk_fails() {
    let repo = TestRepo::new();
    // Stay on main (trunk)

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    // On trunk, the command may fail with "not tracked", "trunk", or "No PR" message
    // depending on how the validation logic proceeds
    assert!(
        stderr.contains("not tracked") || stderr.contains("trunk") || stderr.contains("No PR"),
        "Expected message about untracked/trunk branch or missing PR, got: {}",
        stderr
    );
}

#[test]
fn test_comments_untracked_branch_fails() {
    let repo = TestRepo::new();

    // Create an untracked branch directly with git
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("track"),
        "Expected message about untracked branch, got: {}",
        stderr
    );
}

#[test]
fn test_comments_no_pr_fails() {
    let repo = TestRepo::new();

    // Create a tracked branch but without a PR
    repo.create_stack(&["feature-a"]);
    assert!(repo.current_branch_contains("feature-a"));

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No PR") || stderr.contains("submit") || stderr.contains("PR"),
        "Expected message about missing PR, got: {}",
        stderr
    );
}

// =============================================================================
// Integration with Stack
// =============================================================================

#[test]
fn test_comments_from_deep_stack_no_pr() {
    let repo = TestRepo::new();

    // Create a deeper stack
    repo.create_stack(&["feature-a", "feature-b", "feature-c"]);
    assert!(repo.current_branch_contains("feature-c"));

    // Should still fail because no PR exists
    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No PR") || stderr.contains("submit"),
        "Expected message about missing PR, got: {}",
        stderr
    );
}
