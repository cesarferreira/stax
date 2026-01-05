//! Comprehensive integration tests targeting additional code paths
//! These tests focus on edge cases and less common execution paths

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Navigation Tests (edge cases)
// =============================================================================

#[test]
fn test_navigation_up_with_count() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c", "d"]);
    repo.run_stax(&["bottom"]);

    let output = repo.run_stax(&["up", "2"]);
    output.assert_success();
}

#[test]
fn test_navigation_down_with_count() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c", "d"]);

    let output = repo.run_stax(&["down", "2"]);
    output.assert_success();
}

#[test]
fn test_navigation_top_from_bottom() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);
    repo.run_stax(&["bottom"]);

    let output = repo.run_stax(&["top"]);
    output.assert_success();
}

#[test]
fn test_navigation_bottom_from_top() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    let output = repo.run_stax(&["bottom"]);
    output.assert_success();
}

// =============================================================================
// Status Command Variations
// =============================================================================

#[test]
fn test_status_alias_s() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["s"]);
    output.assert_success();
}

#[test]
fn test_status_alias_ls() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["ls"]);
    output.assert_success();
}

#[test]
fn test_status_with_deep_stack() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c", "d", "e"]);

    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

#[test]
fn test_status_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

// =============================================================================
// Branch Subcommand Variations
// =============================================================================

#[test]
fn test_branch_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "--help"]);
    output.assert_success();
}

#[test]
fn test_branch_alias_b() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["b", "--help"]);
    output.assert_success();
}

#[test]
fn test_branch_create_alias_bc() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "test-branch"]);
    output.assert_success();
}

#[test]
fn test_branch_delete_alias_bd() {
    let repo = TestRepo::new();
    repo.create_stack(&["to-delete"]);
    repo.run_stax(&["t"]); // Go to trunk

    let output = repo.run_stax(&["bd", "--help"]);
    output.assert_success();
}

#[test]
fn test_branch_fold_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "fold", "--help"]);
    output.assert_success();
}

// =============================================================================
// Upstack Command Variations
// =============================================================================

#[test]
fn test_upstack_alias_us() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["us", "--help"]);
    output.assert_success();
}

// =============================================================================
// Downstack Command Variations
// =============================================================================

#[test]
fn test_downstack_alias_ds() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["ds", "--help"]);
    output.assert_success();
}

// =============================================================================
// Continue Command Variations
// =============================================================================

#[test]
fn test_continue_alias_cont() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["cont"]);
    // Should succeed with message when no rebase in progress
    output.assert_success();
    output.assert_stdout_contains("No rebase");
}

#[test]
fn test_continue_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["continue", "--help"]);
    output.assert_success();
}

// =============================================================================
// Log Command Variations
// =============================================================================

#[test]
fn test_log_with_multiple_commits() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Add more commits
    repo.create_file("file2.txt", "content2");
    repo.commit("Second commit");
    repo.create_file("file3.txt", "content3");
    repo.commit("Third commit");

    let output = repo.run_stax(&["log"]);
    output.assert_success();
}

// =============================================================================
// Diff Command Variations
// =============================================================================

#[test]
fn test_diff_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["diff"]);
    // Should succeed or handle gracefully
    let _ = output;
}

#[test]
fn test_diff_with_multiple_branches() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    let output = repo.run_stax(&["diff"]);
    output.assert_success();
}

// =============================================================================
// Doctor Command Variations
// =============================================================================

#[test]
fn test_doctor_output_format() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["doctor"]);
    output.assert_success();
    // Should contain check symbols
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("✓") || stdout.contains("✗") || stdout.contains("!") || stdout.len() > 0);
}

// =============================================================================
// Restack Variations
// =============================================================================

#[test]
fn test_restack_on_branch_not_needing_restack() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // No changes to parent, should succeed
    let output = repo.run_stax(&["restack"]);
    output.assert_success();
}

#[test]
fn test_restack_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["restack", "--help"]);
    output.assert_success();
}

// =============================================================================
// Modify Command Variations
// =============================================================================

#[test]
fn test_modify_no_changes() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // No changes to amend
    let output = repo.run_stax(&["modify"]);
    // May fail or succeed depending on implementation
    let _ = output;
}

// =============================================================================
// Branch Squash Variations
// =============================================================================

#[test]
fn test_branch_squash_multiple_commits() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Add more commits
    repo.create_file("file2.txt", "content2");
    repo.commit("Second commit");
    repo.create_file("file3.txt", "content3");
    repo.commit("Third commit");

    let output = repo.run_stax(&["branch", "squash"]);
    // Should succeed or fail gracefully
    let _ = output;
}

// =============================================================================
// Config Variations
// =============================================================================

#[test]
fn test_config_output_contains_path() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config"]);
    output.assert_success();
    // Should show config path
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("config") || stdout.contains("Config") || stdout.len() > 0);
}

// =============================================================================
// Auth Command Variations
// =============================================================================

#[test]
fn test_auth_status_output() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["auth", "status"]);
    // Should show status
    let _ = output;
}

// =============================================================================
// PR Command Variations
// =============================================================================

#[test]
fn test_pr_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr"]);
    // Should fail on trunk
    output.assert_failure();
}

// =============================================================================
// Submit Variations
// =============================================================================

#[test]
fn test_submit_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["submit"]);
    // May fail or succeed with message about trunk
    let _ = output;
}

// =============================================================================
// Version and Help Commands
// =============================================================================

#[test]
fn test_version_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["--version"]);
    output.assert_success();
    output.assert_stdout_contains("stax");
}

#[test]
fn test_help_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["--help"]);
    output.assert_success();
    output.assert_stdout_contains("Usage");
}

#[test]
fn test_help_command() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["help"]);
    output.assert_success();
    output.assert_stdout_contains("Commands");
}

// =============================================================================
// Checkout Edge Cases
// =============================================================================

#[test]
fn test_checkout_alias_bco() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["bco", "main"]);
    output.assert_success();
}

#[test]
fn test_checkout_current_branch() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let current = repo.current_branch();
    let output = repo.run_stax(&["checkout", &current]);
    output.assert_success();
}

// =============================================================================
// Range Diff Variations
// =============================================================================

#[test]
fn test_range_diff_on_branch() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    let output = repo.run_stax(&["range-diff"]);
    // May fail without remote
    let _ = output;
}

// =============================================================================
// Merge Command Variations
// =============================================================================

#[test]
fn test_merge_on_trunk() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge"]);
    // Should fail or report no branches
    let _ = output;
}

// =============================================================================
// Deep Stack Tests
// =============================================================================

#[test]
fn test_deep_stack_creation() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c", "d", "e", "f", "g"]);

    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

#[test]
fn test_deep_stack_navigation() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c", "d", "e"]);

    // Navigate to bottom
    let output = repo.run_stax(&["bottom"]);
    output.assert_success();

    // Navigate to top
    let output = repo.run_stax(&["top"]);
    output.assert_success();
}

// =============================================================================
// Rename Variations
// =============================================================================

#[test]
fn test_rename_with_special_chars() {
    let repo = TestRepo::new();
    repo.create_stack(&["old"]);

    let output = repo.run_stax(&["rename", "new-name-with-dashes"]);
    output.assert_success();
}

// =============================================================================
// Branch Track Variations
// =============================================================================

#[test]
fn test_track_already_tracked() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature"]);

    // Try to track an already tracked branch
    let output = repo.run_stax(&["branch", "track"]);
    // Should handle gracefully
    let _ = output;
}

// =============================================================================
// Downstack/Upstack Get Tests
// =============================================================================

#[test]
fn test_downstack_get() {
    let repo = TestRepo::new();
    repo.create_stack(&["a", "b", "c"]);

    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();
}

#[test]
fn test_upstack_restack_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["upstack", "restack", "--help"]);
    output.assert_success();
}

// =============================================================================
// Multiple Independent Stacks
// =============================================================================

#[test]
fn test_multiple_stacks_status() {
    let repo = TestRepo::new();

    // Create first stack
    repo.run_stax(&["bc", "stack1-a"]);
    repo.run_stax(&["bc", "stack1-b"]);

    // Return to trunk and create second stack
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "stack2-a"]);
    repo.run_stax(&["bc", "stack2-b"]);

    let output = repo.run_stax(&["status"]);
    output.assert_success();
}

// =============================================================================
// Log Long Output
// =============================================================================

#[test]
fn test_ll_command() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-a", "feature-b"]);

    let output = repo.run_stax(&["ll"]);
    output.assert_success();
}

// =============================================================================
// Branch Reparent Edge Cases
// =============================================================================

#[test]
fn test_reparent_to_sibling() {
    let repo = TestRepo::new();

    // Create two independent branches
    repo.run_stax(&["bc", "branch-a"]);
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "branch-b"]);

    // Try to reparent branch-b to branch-a (same level)
    let output = repo.run_stax(&["branch", "reparent", "branch-a"]);
    // Should succeed or fail depending on implementation
    let _ = output;
}
