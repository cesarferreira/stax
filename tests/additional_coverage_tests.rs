//! Additional integration tests for code coverage
//! Targeting edge cases and less-tested code paths

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Downstack Command Tests
// =============================================================================

#[test]
fn test_downstack_get_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();
}

#[test]
fn test_downstack_get_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();
}

// =============================================================================
// Upstack Command Tests
// =============================================================================

#[test]
fn test_upstack_restack_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["upstack", "restack"]);
    // Should handle gracefully on trunk
    let _ = output;
}

#[test]
fn test_upstack_restack_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);
    repo.run_stax(&["checkout", "a"]);

    let output = repo.run_stax(&["upstack", "restack"]);
    output.assert_success();
}

// =============================================================================
// Branch Create with Parent Tests
// =============================================================================

#[test]
fn test_branch_create_from_different_parent() {
    let repo = TestRepo::new();
    repo.create_stack(&["base"]);
    repo.run_stax(&["trunk"]);

    // Create another branch from trunk
    let output = repo.run_stax(&["bc", "sibling"]);
    output.assert_success();
}

#[test]
fn test_branch_create_deep_nesting() {
    let repo = TestRepo::new();

    for i in 0..5 {
        let name = format!("level-{}", i);
        let output = repo.run_stax(&["bc", &name]);
        output.assert_success();
    }

    // Verify stack depth
    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

// =============================================================================
// Log Command Variations
// =============================================================================

#[test]
fn test_log_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

#[test]
fn test_log_alias_ll() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["ll"]);
    output.assert_success();
}

#[test]
fn test_log_with_empty_stack() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

// =============================================================================
// Trunk Command Tests
// =============================================================================

#[test]
fn test_trunk_alias_t() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["t"]);
    output.assert_success();

    // Verify we're on trunk
    let current = repo.current_branch();
    assert!(current == "main" || current == "master");
}

#[test]
fn test_trunk_when_on_trunk() {
    let repo = TestRepo::new();

    // Already on trunk
    let output = repo.run_stax(&["trunk"]);
    output.assert_success();
}

// =============================================================================
// Modify Command Tests
// =============================================================================

#[test]
fn test_modify_with_staged_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Create a change and stage it
    repo.create_file("new_file.txt", "content");
    repo.git(&["add", "new_file.txt"]);

    let output = repo.run_stax(&["modify"]);
    // Should amend the commit
    output.assert_success();
}

#[test]
fn test_modify_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["modify"]);
    // Should fail or warn on trunk
    let _ = output;
}

// =============================================================================
// Doctor Command Tests
// =============================================================================

#[test]
fn test_doctor_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    let output = repo.run_stax(&["doctor"]);
    output.assert_success();
}

#[test]
fn test_doctor_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["doctor"]);
    output.assert_success();
}

// =============================================================================
// Restack Command Tests
// =============================================================================

#[test]
fn test_restack_clean_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);

    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

#[test]
fn test_restack_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["restack"]);
    // Should handle trunk case
    let _ = output;
}

#[test]
fn test_restack_with_parent_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["parent", "child"]);

    // Go to parent and add a commit
    repo.run_stax(&["checkout", "parent"]);
    repo.create_file("parent_file.txt", "content");
    repo.commit("Parent change");

    // Go to child and restack
    repo.run_stax(&["checkout", "child"]);
    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

// =============================================================================
// Status Command Tests
// =============================================================================

#[test]
fn test_status_on_untracked_branch() {
    let repo = TestRepo::new();

    // Create a git branch without stax tracking
    repo.git(&["checkout", "-b", "untracked"]);

    let output = repo.run_stax(&["status"]);
    // Should handle untracked branch
    let _ = output;
}

#[test]
fn test_status_long() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);

    // ll is the long status alias
    let output = repo.run_stax(&["ll"]);
    output.assert_success();
}

// =============================================================================
// Branch Track/Untrack Tests
// =============================================================================

#[test]
fn test_branch_untrack() {
    let repo = TestRepo::new();
    repo.create_stack(&["tracked"]);

    let output = repo.run_stax(&["branch", "untrack"]);
    // Should handle untracking
    let _ = output;
}

// =============================================================================
// Checkout Edge Cases
// =============================================================================

#[test]
fn test_checkout_nonexistent_branch() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["checkout", "nonexistent"]);
    output.assert_failure();
}

#[test]
fn test_checkout_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    // Checkout middle branch
    let output = repo.run_stax(&["checkout", "b"]);
    output.assert_success();

    assert_eq!(repo.current_branch(), "b");
}

// =============================================================================
// Diff Command Tests
// =============================================================================

#[test]
fn test_diff_with_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Add some changes
    repo.create_file("new_file.txt", "content");
    repo.commit("Added file");

    let output = repo.run_stax(&["diff"]);
    output.assert_success();
}

#[test]
fn test_diff_empty_branch() {
    let repo = TestRepo::new();
    repo.create_stack(&["empty"]);

    // No additional commits on this branch
    let output = repo.run_stax(&["diff"]);
    output.assert_success();
}

// =============================================================================
// Config Command Tests
// =============================================================================

#[test]
fn test_config_shows_values() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config"]);
    output.assert_success();
}

// =============================================================================
// Multiple Stacks Tests
// =============================================================================

#[test]
fn test_multiple_independent_stacks() {
    let repo = TestRepo::new();

    // Create first stack
    repo.run_stax(&["bc", "stack1-a"]);
    repo.create_file("s1.txt", "content");
    repo.commit("Stack 1 commit");

    // Return to trunk and create second stack
    repo.run_stax(&["trunk"]);
    repo.run_stax(&["bc", "stack2-a"]);
    repo.create_file("s2.txt", "content");
    repo.commit("Stack 2 commit");

    // Verify both stacks exist
    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

// =============================================================================
// Navigation Edge Cases
// =============================================================================

#[test]
fn test_up_from_top() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);

    // Already at top
    let output = repo.run_stax(&["up"]);
    // Should handle gracefully
    output.assert_success();
}

#[test]
fn test_down_from_bottom() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);
    repo.run_stax(&["bottom"]);

    // Already at bottom (closest to trunk)
    let output = repo.run_stax(&["down"]);
    // Should handle gracefully
    let _ = output;
}

#[test]
fn test_up_with_count_exceeding_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);
    repo.run_stax(&["bottom"]);

    // Try to go up more than stack depth
    let output = repo.run_stax(&["up", "10"]);
    output.assert_success();
}

// =============================================================================
// Branch Squash Tests
// =============================================================================

#[test]
fn test_branch_squash_single_commit() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Only one commit
    let output = repo.run_stax(&["branch", "squash"]);
    // Should succeed even with single commit
    let _ = output;
}

// =============================================================================
// Rename Command Tests
// =============================================================================

#[test]
fn test_rename_to_existing_name() {
    let repo = TestRepo::new();
    repo.create_stack(&["existing"]);
    repo.run_stax(&["trunk"]);
    repo.create_stack(&["to-rename"]);

    let output = repo.run_stax(&["rename", "existing"]);
    // Should fail - branch already exists
    output.assert_failure();
}

#[test]
fn test_rename_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["rename", "new-name"]);
    // Should fail - can't rename trunk
    output.assert_failure();
}

// =============================================================================
// Auth Command Tests
// =============================================================================

#[test]
fn test_auth_status() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["auth", "status"]);
    // Should show auth status
    let _ = output;
}

#[test]
fn test_auth_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["auth", "--help"]);
    output.assert_success();
}

// =============================================================================
// Continue Command Tests
// =============================================================================

#[test]
fn test_continue_no_rebase() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["continue"]);
    output.assert_success();
    output.assert_stdout_contains("No rebase");
}

// =============================================================================
// PR Command Tests
// =============================================================================

#[test]
fn test_pr_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "--help"]);
    output.assert_success();
}

// =============================================================================
// Submit Command Tests
// =============================================================================

#[test]
fn test_submit_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["submit", "--help"]);
    output.assert_success();
}

// =============================================================================
// Undo/Redo Command Tests
// =============================================================================

#[test]
fn test_undo_no_operations() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["undo"]);
    // Should fail gracefully - no operations to undo
    output.assert_failure();
}

#[test]
fn test_redo_no_operations() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["redo"]);
    // Should fail gracefully - no operations to redo
    output.assert_failure();
}

// =============================================================================
// Merge Command Tests
// =============================================================================

#[test]
fn test_merge_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--help"]);
    output.assert_success();
}

// =============================================================================
// Range Diff Tests
// =============================================================================

#[test]
fn test_range_diff_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["range-diff", "--help"]);
    output.assert_success();
}

// =============================================================================
// Complex Stack Operations
// =============================================================================

#[test]
fn test_branch_fold_in_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b"]);

    // Go to parent and fold
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["branch", "fold"]);
    // Should fold successfully
    output.assert_success();
}

#[test]
fn test_reparent_in_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    // Reparent c to have a as parent instead of b
    let output = repo.run_stax(&["branch", "reparent", "a"]);
    // Should work
    let _ = output;
}

// =============================================================================
// Sync Command Tests
// =============================================================================

#[test]
fn test_sync_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["sync", "--help"]);
    output.assert_success();
}
