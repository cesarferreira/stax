//! Branch fold command integration tests
//!
//! Tests for the `branch fold` command that merges a branch into its parent.
//! Note: The fold command is interactive (requires confirmation), so we test
//! error cases that exit before the confirmation prompt.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Error Case Tests (these don't require confirmation)
// =============================================================================

#[test]
fn test_fold_into_trunk_not_allowed() {
    let repo = TestRepo::new();

    // Create a branch directly from main
    repo.create_stack(&["feature-1"]);
    assert!(repo.current_branch_contains("feature-1"));

    // Try to fold into trunk - should fail before confirmation
    let output = repo.run_stax(&["branch", "fold"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate we can't fold into trunk
    assert!(
        combined.contains("trunk")
            || combined.contains("Cannot fold")
            || combined.contains("submit"),
        "Expected message about trunk, got: {}",
        combined
    );
}

#[test]
fn test_fold_with_children_not_allowed() {
    let repo = TestRepo::new();

    // Create a stack: main -> feature-1 -> feature-2
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to feature-1 (which has feature-2 as child)
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // Try to fold - should fail because it has children
    let output = repo.run_stax(&["branch", "fold"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate we can't fold due to children
    assert!(
        combined.contains("children")
            || combined.contains("child")
            || combined.contains("Cannot fold"),
        "Expected message about children, got: {}",
        combined
    );
}

#[test]
fn test_fold_untracked_branch_fails() {
    let repo = TestRepo::new();

    // Create an untracked branch directly with git
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    // Try to fold - should fail because branch is not tracked
    let output = repo.run_stax(&["branch", "fold"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate branch is not tracked
    assert!(
        combined.contains("not tracked")
            || combined.contains("track")
            || !output.status.success(),
        "Expected message about tracking or failure, got: {}",
        combined
    );
}

#[test]
fn test_fold_on_trunk_not_allowed() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized
    repo.create_stack(&["feature-1"]);

    // Go back to trunk
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Try to fold on trunk - should fail
    let output = repo.run_stax(&["branch", "fold"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate can't fold on trunk (not tracked or similar)
    assert!(
        combined.contains("trunk")
            || combined.contains("not tracked")
            || !output.status.success(),
        "Expected failure on trunk, got: {}",
        combined
    );
}

#[test]
fn test_fold_no_commits_to_fold() {
    let repo = TestRepo::new();

    // Create a branch chain but with same commit as parent
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    // Create child without additional commits
    repo.run_stax(&["bc", "feature-2"]);
    // No commit here - same as parent

    // Delete child's branch so feature-1 has no children
    let feature2 = repo.current_branch();
    repo.run_stax(&["checkout", &feature1]);
    repo.run_stax(&["branch", "delete", &feature2, "--force"]);

    // Now create a new branch from feature-1 without any new commits
    // Actually, the branch creation itself creates a commit in our helper
    // So we need a different approach

    // Create a branch that points to the same commit as parent
    repo.git(&["checkout", "-b", "empty-branch"]);

    // Track it with stax
    repo.run_stax(&["branch", "track", "--parent", &feature1]);

    // Now try to fold - should say no commits to fold
    let output = repo.run_stax(&["branch", "fold"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate no commits to fold
    assert!(
        combined.contains("No commits")
            || combined.contains("no commits")
            || combined.contains("0 commit"),
        "Expected 'no commits' message, got: {}",
        combined
    );
}

// =============================================================================
// Help and Alias Tests
// =============================================================================

#[test]
fn test_fold_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "fold", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("--keep") || stdout.contains("-k"), "Expected --keep flag in help");
    assert!(
        stdout.contains("fold") || stdout.contains("Fold"),
        "Expected 'fold' in help"
    );
}

#[test]
fn test_fold_alias_f() {
    let repo = TestRepo::new();

    // b f should work as alias for branch fold
    let output = repo.run_stax(&["b", "f", "--help"]);
    output.assert_success();
}

#[test]
fn test_fold_keep_flag_in_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "fold", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("keep") || stdout.contains("Keep"),
        "Expected 'keep' in fold help: {}",
        stdout
    );
}

// =============================================================================
// Fold Scenario Setup Tests
// These test the preconditions for fold without running interactive fold
// =============================================================================

#[test]
fn test_fold_scenario_valid_setup() {
    let repo = TestRepo::new();

    // Create: main -> feature-1 -> feature-2 (no children)
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f1.txt", "content 1");
    repo.commit("Feature 1 commit");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "content 2");
    repo.commit("Feature 2 commit");

    // feature-2 has no children, so fold should be possible
    // (would require confirmation in real scenario)

    // Verify the setup is correct for fold
    let json = repo.get_status_json();
    let branches = json["branches"].as_array().unwrap();

    let f2_branch = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("feature-2"))
        .expect("Should find feature-2");

    // feature-2 should have feature-1 as parent (not trunk)
    assert!(
        f2_branch["parent"]
            .as_str()
            .unwrap_or("")
            .contains("feature-1"),
        "feature-2 should have feature-1 as parent"
    );

    // feature-2 should have commits ahead of its parent
    let ahead = f2_branch["ahead"].as_i64().unwrap_or(0);
    assert!(ahead > 0, "feature-2 should have commits ahead: {}", ahead);
}

#[test]
fn test_fold_scenario_branch_with_children_detected() {
    let repo = TestRepo::new();

    // Create: main -> feature-1 -> feature-2
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "content 1");
    repo.commit("Feature 1 commit");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "content 2");
    repo.commit("Feature 2 commit");

    // feature-1 has feature-2 as child
    let children = repo.get_children(&feature1);
    assert!(
        !children.is_empty(),
        "feature-1 should have children: {:?}",
        children
    );
    assert!(
        children.iter().any(|c| c.contains("feature-2")),
        "feature-1 should have feature-2 as child"
    );
}

#[test]
fn test_fold_scenario_parent_is_not_trunk() {
    let repo = TestRepo::new();

    // Create: main -> feature-1 -> feature-2
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);

    // Check parent is feature-1, not trunk
    let parent = repo.get_current_parent();
    assert!(parent.is_some(), "feature-2 should have a parent");
    assert!(
        parent.unwrap().contains("feature-1"),
        "feature-2's parent should be feature-1"
    );

    // This means fold would work (parent is not trunk)
}

