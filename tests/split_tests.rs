//! Split command integration tests
//!
//! Tests for the `split` command that splits a branch into multiple stacked branches.
//! Note: The split command launches an interactive TUI, so we test
//! pre-condition validation that exits before the TUI launches.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Help Tests
// =============================================================================

#[test]
fn test_split_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["split", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("split") || stdout.contains("Split"));
    assert!(stdout.contains("branch"));
}

// =============================================================================
// Error Case Tests (validation before TUI)
// =============================================================================

#[test]
fn test_split_on_trunk_fails() {
    let repo = TestRepo::new();
    // Stay on main (trunk)

    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot split"),
        "Expected message about trunk branch, got: {}",
        stderr
    );
}

#[test]
fn test_split_untracked_branch_fails() {
    let repo = TestRepo::new();

    // Create an untracked branch directly with git
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("file1.txt", "content 1");
    repo.commit("commit 1");
    repo.create_file("file2.txt", "content 2");
    repo.commit("commit 2");

    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("track"),
        "Expected message about untracked branch, got: {}",
        stderr
    );
}

#[test]
fn test_split_no_commits_fails() {
    let repo = TestRepo::new();

    // Create a tracked branch with just the initial commit from create_stack
    repo.create_stack(&["feature-a"]);
    assert!(repo.current_branch_contains("feature-a"));

    // Branch has only 1 commit from create_stack
    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("Only 1 commit")
            || stderr.contains("at least 2")
            || stderr.contains("No commits"),
        "Expected message about insufficient commits, got: {}",
        stderr
    );
}

#[test]
fn test_split_single_commit_fails() {
    let repo = TestRepo::new();

    // Create a tracked branch
    repo.create_stack(&["feature-a"]);

    // The create_stack already adds 1 commit, so we have exactly 1 commit above parent
    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("Only 1 commit")
            || stderr.contains("at least 2")
            || stderr.contains("commits"),
        "Expected message about needing 2+ commits, got: {}",
        stderr
    );
}

// =============================================================================
// Validation with Multiple Commits
// =============================================================================

#[test]
fn test_split_with_two_commits_passes_validation() {
    let repo = TestRepo::new();

    // Create a tracked branch
    repo.create_stack(&["feature-a"]);

    // Add a second commit so we have 2 commits above parent
    repo.create_file("extra.txt", "extra content");
    repo.commit("second commit");

    // Run split - it should pass validation but fail because no terminal
    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);

    // Should fail with "interactive terminal" error (validation passed)
    assert!(
        stderr.contains("interactive terminal"),
        "Should have passed validation and failed on terminal check, got: {}",
        stderr
    );
}

#[test]
fn test_split_with_many_commits_passes_validation() {
    let repo = TestRepo::new();

    // Create a tracked branch
    repo.create_stack(&["feature-a"]);

    // Add multiple commits
    for i in 2..=5 {
        repo.create_file(&format!("file{}.txt", i), &format!("content {}", i));
        repo.commit(&format!("commit {}", i));
    }

    // Run split - should pass validation but fail because no terminal
    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);

    // Should fail with "interactive terminal" error (validation passed)
    assert!(
        stderr.contains("interactive terminal"),
        "Should have passed validation and failed on terminal check, got: {}",
        stderr
    );
}

// =============================================================================
// Stack Context Tests
// =============================================================================

#[test]
fn test_split_from_middle_of_stack_passes_validation() {
    let repo = TestRepo::new();

    // Create a stack: main -> feature-a -> feature-b
    let branches = repo.create_stack(&["feature-a", "feature-b"]);

    // Go to feature-a and add more commits
    repo.run_stax(&["checkout", &branches[0]]);
    repo.create_file("extra1.txt", "content");
    repo.commit("extra commit 1");
    repo.create_file("extra2.txt", "content");
    repo.commit("extra commit 2");

    // Run split - should pass validation but fail because no terminal
    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);

    // Should fail with "interactive terminal" error (validation passed)
    assert!(
        stderr.contains("interactive terminal"),
        "Should have passed validation and failed on terminal check, got: {}",
        stderr
    );
}
