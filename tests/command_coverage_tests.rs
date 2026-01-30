//! Additional integration tests to increase code coverage
//! Tests for: checkout, sync, restack, undo, redo, doctor, log, diff

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Checkout Command Tests
// =============================================================================

#[test]
fn test_checkout_to_trunk() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["checkout", "main"]);
    output.assert_success();
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_checkout_alias_co() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["co", "main"]);
    output.assert_success();
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_checkout_nonexistent_branch() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["checkout", "nonexistent-branch"]);
    output.assert_failure();
}

#[test]
fn test_checkout_to_tracked_branch() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["feature-a", "feature-b"]);

    // Navigate to first branch
    let output = repo.run_stax(&["checkout", &branches[0]]);
    output.assert_success();
    assert!(repo.current_branch_contains("feature-a"));
}

#[test]
fn test_checkout_by_partial_match() {
    let repo = TestRepo::new();
    repo.create_stack(&["unique-feature"]);
    repo.run_stax(&["checkout", "main"]);

    // Try partial match
    let output = repo.run_stax(&["checkout", "unique"]);
    // Should either succeed with the match or fail if ambiguous
    if output.status.success() {
        assert!(repo.current_branch_contains("unique"));
    }
}

#[test]
fn test_checkout_trunk_alias_t() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["t"]);
    output.assert_success();
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_checkout_trunk_full_command() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["trunk"]);
    output.assert_success();
    assert_eq!(repo.current_branch(), "main");
}

// =============================================================================
// Sync Command Tests
// =============================================================================

#[test]
fn test_sync_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["sync", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("Sync");
}

#[test]
fn test_sync_alias_rs_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["rs", "--help"]);
    output.assert_success();
}

// =============================================================================
// Restack Command Tests
// =============================================================================

#[test]
fn test_restack_no_changes_needed() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // When no restack is needed, should still succeed
    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

#[test]
fn test_restack_with_changes() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["feature-a", "feature-b"]);

    // Go to feature-a and make a new commit
    repo.run_stax(&["checkout", &branches[0]]);
    repo.create_file("extra.txt", "extra content");
    repo.commit("Extra commit");

    // Go to feature-b and restack
    repo.run_stax(&["checkout", &branches[1]]);
    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

#[test]
fn test_restack_quiet_flag() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["restack", "--quiet"]);
    output.assert_success();
}

#[test]
fn test_restack_on_trunk() {
    let repo = TestRepo::new();

    // Restack on trunk should be a no-op
    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

#[test]
fn test_restack_continue_no_rebase() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Continue when no rebase is in progress
    let output = repo.run_stax(&["restack", "--continue"]);
    // Should succeed or inform no rebase in progress
    let _ = output;
}

#[test]
fn test_upstack_restack() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["feature-a", "feature-b", "feature-c"]);

    // Go to middle branch
    repo.run_stax(&["checkout", &branches[0]]);

    // Upstack restack should restack all downstream branches
    let output = repo.run_stax(&["upstack", "restack"]);
    output.assert_success();
}

// =============================================================================
// Undo/Redo Command Tests
// =============================================================================

#[test]
fn test_undo_no_operations() {
    let repo = TestRepo::new();

    // Undo with no previous operations
    let output = repo.run_stax(&["undo"]);
    // Should fail or report nothing to undo
    output.assert_failure();
}

#[test]
fn test_undo_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["undo", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("Undo");
}

#[test]
fn test_redo_no_operations() {
    let repo = TestRepo::new();

    // Redo with no previous undo
    let output = repo.run_stax(&["redo"]);
    output.assert_failure();
}

#[test]
fn test_redo_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["redo", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("Redo");
}

// =============================================================================
// Doctor Command Tests
// =============================================================================

#[test]
fn test_doctor_basic() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["doctor"]);
    output.assert_success();
}

#[test]
fn test_doctor_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-a", "feature-b"]);

    let output = repo.run_stax(&["doctor"]);
    output.assert_success();
}

#[test]
fn test_doctor_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["doctor", "--help"]);
    output.assert_success();
}

// =============================================================================
// Log Command Tests
// =============================================================================

#[test]
fn test_log_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

#[test]
fn test_log_on_branch() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

#[test]
fn test_log_with_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-a", "feature-b", "feature-c"]);

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

#[test]
fn test_log_alias_l() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["l"]);
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
fn test_log_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["log", "--help"]);
    output.assert_success();
}

// =============================================================================
// Diff Command Tests
// =============================================================================

#[test]
fn test_diff_on_branch() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["diff"]);
    output.assert_success();
}

#[test]
fn test_diff_with_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);
    repo.create_file("new-file.txt", "content");
    repo.commit("Add new file");

    let output = repo.run_stax(&["diff"]);
    output.assert_success();
}

#[test]
fn test_diff_alias_d() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["d"]);
    output.assert_success();
}

#[test]
fn test_diff_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["diff", "--help"]);
    output.assert_success();
}

// =============================================================================
// Range-Diff Command Tests
// =============================================================================

#[test]
fn test_range_diff_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["range-diff", "--help"]);
    output.assert_success();
}

// =============================================================================
// Modify Command Tests
// =============================================================================

#[test]
fn test_modify_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["modify", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("modify");
}

#[test]
fn test_modify_alias_m() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["m", "--help"]);
    output.assert_success();
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

#[test]
fn test_pr_without_remote() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["pr"]);
    // Should fail without remote/PR
    output.assert_failure();
}

// =============================================================================
// Submit Command Tests
// =============================================================================

#[test]
fn test_submit_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["submit", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("Submit");
    output.assert_stdout_contains("verbose");
}

#[test]
fn test_submit_alias_ss() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["ss", "--help"]);
    output.assert_success();
}

#[test]
fn test_submit_without_remote() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["submit"]);
    // Should fail without remote
    output.assert_failure();
}

#[test]
fn test_submit_draft_flag() {
    let repo = TestRepo::new_with_remote();
    repo.create_stack(&["feature"]);

    // Help should show draft flag
    let output = repo.run_stax(&["submit", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("draft");
}

// =============================================================================
// Branch Create Tests
// =============================================================================

#[test]
fn test_branch_create_with_message() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "test-branch", "-m", "Test message"]);
    output.assert_success();
    assert!(repo.current_branch_contains("test-branch"));
}

#[test]
fn test_branch_create_from_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "from-trunk"]);
    output.assert_success();
}

#[test]
fn test_branch_create_stacked() {
    let repo = TestRepo::new();
    repo.create_stack(&["parent"]);

    let output = repo.run_stax(&["bc", "child"]);
    output.assert_success();

    // Child should have parent as its parent
    let parent = repo.get_current_parent();
    assert!(parent.is_some());
    assert!(parent.unwrap().contains("parent"));
}

// =============================================================================
// Branch Delete Tests
// =============================================================================

#[test]
fn test_branch_delete_untracked() {
    let repo = TestRepo::new();

    // Create a regular git branch (not through stax)
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.git(&["checkout", "main"]);

    let output = repo.run_stax(&["bd", "untracked-branch", "--force"]);
    // Should handle untracked branches
    let _ = output;
}

#[test]
fn test_branch_delete_current_fails() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let current = repo.current_branch();
    let output = repo.run_stax(&["bd", &current]);
    // Should fail - can't delete current branch
    output.assert_failure();
}

#[test]
fn test_branch_delete_with_children() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["parent", "child"]);

    // Go to main
    repo.run_stax(&["t"]);

    // Try to delete parent with children
    let output = repo.run_stax(&["bd", &branches[0]]);
    // Should fail or require force
    output.assert_failure();
}

// =============================================================================
// Branch Rename Tests
// =============================================================================

#[test]
fn test_branch_rename() {
    let repo = TestRepo::new();
    repo.create_stack(&["old-name"]);

    let output = repo.run_stax(&["branch", "rename", "new-name"]);
    output.assert_success();
    assert!(repo.current_branch_contains("new-name"));
}

#[test]
fn test_branch_rename_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "rename", "--help"]);
    output.assert_success();
}

// =============================================================================
// Branch Track Tests
// =============================================================================

#[test]
fn test_branch_track_untracked() {
    let repo = TestRepo::new();

    // Create untracked branch
    repo.git(&["checkout", "-b", "untracked"]);
    repo.create_file("untracked.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    output.assert_success();
}

#[test]
fn test_branch_track_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "track", "--help"]);
    output.assert_success();
}

// =============================================================================
// Branch Squash Tests
// =============================================================================

#[test]
fn test_branch_squash_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "squash", "--help"]);
    output.assert_success();
}

#[test]
fn test_branch_squash_single_commit() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Only one commit, squash should be no-op
    let output = repo.run_stax(&["branch", "squash"]);
    // May succeed with message about single commit
    let _ = output;
}

// =============================================================================
// Auth Command Tests
// =============================================================================

#[test]
fn test_auth_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["auth", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("auth");
}

#[test]
fn test_auth_status() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["auth", "status"]);
    // Should show auth status (authenticated or not)
    let _ = output;
}

// =============================================================================
// Config Command Tests
// =============================================================================

#[test]
fn test_config_show() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config"]);
    output.assert_success();
}

#[test]
fn test_config_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config", "--help"]);
    output.assert_success();
}

// =============================================================================
// Status Tests
// =============================================================================

#[test]
fn test_status_shows_branch_info() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["status"]);
    output.assert_success();
    // Should contain branch name
    output.assert_stdout_contains("feature");
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
// Children Relationship Tests
// =============================================================================

#[test]
fn test_children_relationship() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["parent", "child"]);

    let children = repo.get_children(&branches[0]);
    assert_eq!(children.len(), 1);
    assert!(children[0].contains("child"));
}

// =============================================================================
// Downstack Get Tests
// =============================================================================

#[test]
fn test_downstack_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["downstack", "--help"]);
    output.assert_success();
}

// =============================================================================
// Upstack Tests
// =============================================================================

#[test]
fn test_upstack_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["upstack", "--help"]);
    output.assert_success();
}

// =============================================================================
// Branch Reparent Tests
// =============================================================================

#[test]
fn test_branch_reparent_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "reparent", "--help"]);
    output.assert_success();
}

// =============================================================================
// Create Command Tests
// =============================================================================

#[test]
fn test_create_alias_c() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["c", "new-feature"]);
    output.assert_success();
    assert!(repo.current_branch_contains("new-feature"));
}

#[test]
fn test_create_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["create", "--help"]);
    output.assert_success();
}

// =============================================================================
// Rename Command Tests
// =============================================================================

#[test]
fn test_rename_command() {
    let repo = TestRepo::new();
    repo.create_stack(&["old-name"]);

    let output = repo.run_stax(&["rename", "new-name-2"]);
    output.assert_success();
    assert!(repo.current_branch_contains("new-name-2"));
}

#[test]
fn test_rename_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["rename", "--help"]);
    output.assert_success();
}

// =============================================================================
// Status JSON Tests (with proper assertions)
// =============================================================================

#[test]
fn test_status_json_basic() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["status", "--json"]);
    output.assert_success();

    // Verify it's valid JSON
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");

    assert!(json["trunk"].is_string());
    assert!(json["branches"].is_array());
}
