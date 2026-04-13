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

#[test]
fn test_split_file_extracts_matching_paths_into_new_parent_branch() {
    let repo = TestRepo::new();

    repo.run_stax(&["status"]).assert_success();
    repo.run_stax(&["create", "feature"]).assert_success();

    // Single commit touching both files — `split --file` requires one commit above parent.
    repo.create_file("keep.txt", "keep");
    repo.create_file("move.txt", "move");
    repo.commit("add keep and move");

    let feature_branch = repo.current_branch();

    let output = repo.run_stax(&["split", "--file", "move.txt"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Created") && stdout.contains("Reparented"),
        "Expected split summary, got: {}",
        stdout
    );

    let split_branch = repo
        .find_branch_containing("feature-split")
        .expect("expected split branch to be created");
    assert_eq!(repo.current_branch(), feature_branch);

    let parent_output = repo.git(&["show", &format!("refs/branch-metadata/{}", feature_branch)]);
    let parent_metadata = TestRepo::stdout(&parent_output);
    assert!(
        parent_metadata.contains(&split_branch),
        "Expected current branch metadata to point to split branch, got: {}",
        parent_metadata
    );

    let split_diff = repo.git(&["diff", "--name-only", "main", &split_branch]);
    let split_files = TestRepo::stdout(&split_diff);
    assert!(
        split_files.contains("move.txt"),
        "Split branch should contain move.txt, got: {}",
        split_files
    );

    let feature_diff = repo.git(&["diff", "--name-only", "main", &feature_branch]);
    let feature_files = TestRepo::stdout(&feature_diff);
    assert!(
        feature_files.contains("keep.txt"),
        "Current branch should still contain keep.txt vs main, got: {}",
        feature_files
    );
    assert!(
        !feature_files.contains("move.txt"),
        "Current branch should no longer carry move.txt in its own history, got: {}",
        feature_files
    );
}

#[test]
fn test_split_file_on_multi_commit_branch_fails() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "feature"]).assert_success();

    // Two commits on the branch above parent, both touching the same file.
    repo.create_file("move.txt", "v1");
    repo.commit("add move.txt");
    repo.create_file("move.txt", "v2");
    repo.commit("modify move.txt");

    let branch_before = repo.current_branch();
    let head_before = TestRepo::stdout(&repo.git(&["rev-parse", "HEAD"]))
        .trim()
        .to_string();

    let output = repo.run_stax(&["split", "--file", "move.txt"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("multi-commit") && stderr.contains("stax split --hunk"),
        "Expected guidance toward --hunk, got: {}",
        stderr
    );

    // Branch state must be untouched after the hard-fail.
    assert_eq!(repo.current_branch(), branch_before);
    let head_after = TestRepo::stdout(&repo.git(&["rev-parse", "HEAD"]))
        .trim()
        .to_string();
    assert_eq!(
        head_before, head_after,
        "HEAD should not move when split --file aborts"
    );
    let branches = TestRepo::stdout(&repo.git(&["branch", "--list"]));
    assert!(
        !branches.contains("feature-split"),
        "No split branch should be created on hard-fail, got branches: {}",
        branches
    );
}
