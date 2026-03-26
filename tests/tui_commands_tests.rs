//! Tests for commands that the TUI calls
//!
//! The TUI runs stax commands via subprocess. These tests verify that
//! the commands work correctly when called with the arguments the TUI uses.

mod common;

use common::{OutputAssertions, TestRepo};

/// Test that `stax create <name>` works (TUI InputAction::NewBranch)
#[test]
fn test_tui_create_branch_with_name() {
    let repo = TestRepo::new();

    // TUI calls: run_external_command(app, &["create", &input])
    let output = repo.run_stax(&["create", "feature-from-tui"]);
    output.assert_success();

    // Verify branch was created and we're on it
    assert!(repo.current_branch().contains("feature-from-tui"));

    // Verify it's tracked (has parent)
    let parent = repo.get_current_parent();
    assert_eq!(parent, Some("main".to_string()));
}

/// Test that `stax rename --literal <name>` works (TUI InputAction::Rename)
#[test]
fn test_tui_rename_branch_with_literal() {
    let repo = TestRepo::new();

    // Create a branch first
    repo.run_stax(&["create", "old-name"]).assert_success();
    let old_branch = repo.current_branch();
    assert!(old_branch.contains("old-name"));

    // TUI calls: run_external_command(app, &["rename", "--literal", &input])
    let output = repo.run_stax(&["rename", "--literal", "new-name-from-tui"]);
    output.assert_success();

    // Verify branch was renamed
    assert_eq!(repo.current_branch(), "new-name-from-tui");

    // Verify old branch no longer exists
    let branches = repo.list_branches();
    assert!(!branches.contains(&old_branch));
    assert!(branches.contains(&"new-name-from-tui".to_string()));
}

/// Test that `stax branch delete <branch> --force` works (TUI ConfirmAction::Delete)
#[test]
fn test_tui_delete_branch_force() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["create", "to-delete"]).assert_success();
    let branch_name = repo.current_branch();

    // Go back to main (can't delete current branch)
    repo.run_stax(&["checkout", "main"]).assert_success();

    // TUI calls: run_external_command(app, &["branch", "delete", branch, "--force"])
    let output = repo.run_stax(&["branch", "delete", &branch_name, "--force"]);
    output.assert_success();

    // Verify branch was deleted
    let branches = repo.list_branches();
    assert!(!branches.contains(&branch_name));
}

/// Test the actual TUI delete flow through a real pseudo-terminal.
#[test]
fn test_tui_delete_branch_via_dashboard() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "to-delete"]).assert_success();
    let branch_name = repo.current_branch();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let output = common::run_stax_in_script(&repo.path(), &[], "printf 'kdy'; sleep 1; printf 'q'");
    assert!(
        output.status.success(),
        "Scripted TUI session failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let branches = repo.list_branches();
    assert!(!branches.contains(&branch_name));
}

/// Test that `stax restack --quiet` works (TUI ConfirmAction::Restack)
#[test]
fn test_tui_restack_single_quiet() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1"]);

    // Go to main and make a change (to cause needs_restack)
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "new content");
    repo.commit("Update main");

    // Go back to feature branch
    let branches = repo.list_branches();
    let feature_branch = branches.iter().find(|b| b.contains("feature-1")).unwrap();
    repo.run_stax(&["checkout", feature_branch])
        .assert_success();

    // TUI calls: run_external_command(app, &["restack", "--quiet"])
    let output = repo.run_stax(&["restack", "--quiet"]);
    output.assert_success();

    // Verify we're still on the feature branch
    assert!(repo.current_branch().contains("feature-1"));
}

/// Test that `stax restack --all --quiet` works (TUI ConfirmAction::RestackAll)
#[test]
fn test_tui_restack_all_quiet() {
    let repo = TestRepo::new();

    // Create a stack of multiple branches
    repo.create_stack(&["feature-1", "feature-2"]);

    // Go to main and make a change
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "new content");
    repo.commit("Update main");

    // Go back to top of stack
    repo.run_stax(&["top"]).assert_success();

    // TUI calls: run_external_command(app, &["restack", "--all", "--quiet"])
    let output = repo.run_stax(&["restack", "--all", "--quiet"]);
    output.assert_success();
}

/// Test that `stax submit --no-prompt` doesn't hang waiting for input
/// (TUI KeyAction::Submit - the critical fix)
///
/// Note: This test verifies the command completes without hanging.
/// It may fail due to missing GitHub auth or invalid remote URL format,
/// but the key is that it doesn't block waiting for user input.
#[test]
fn test_tui_submit_no_prompt_does_not_hang() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["create", "submit-test"]).assert_success();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Add feature");

    // TUI calls: run_external_command(app, &["submit", "--no-prompt"])
    // The key test: this should complete quickly without hanging for input
    // It will likely fail (no remote, no auth) but should NOT hang
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let _output = repo.run_stax(&["submit", "--no-prompt"]);
    let elapsed = start.elapsed();

    // Should complete within 5 seconds (not hang waiting for input)
    assert!(
        elapsed < Duration::from_secs(5),
        "Command took too long ({:?}), may be hanging for input",
        elapsed
    );

    // The command will fail (no remote configured properly), but that's OK
    // The important thing is it didn't hang waiting for interactive prompts
}

/// Test that `stax pr` works (TUI KeyAction::OpenPr)
/// Note: This will fail if no PR exists, which is expected behavior
#[test]
fn test_tui_pr_no_pr_exists() {
    let repo = TestRepo::new();

    // Create a tracked branch
    repo.run_stax(&["create", "pr-test"]).assert_success();

    // TUI calls: run_external_command(app, &["pr"])
    // Should fail gracefully since there's no PR
    let output = repo.run_stax(&["pr"]);
    output.assert_failure();

    // Should mention that no PR exists
    let stderr = TestRepo::stderr(&output);
    assert!(stderr.contains("No PR") || stderr.contains("submit"));
}

/// Test that branch creation with empty name fails gracefully
#[test]
fn test_tui_create_empty_name_fails() {
    let repo = TestRepo::new();

    // TUI validates empty input, but test the command directly
    let output = repo.run_stax(&["create"]);
    output.assert_failure();
}

/// Test that rename with empty name fails gracefully
#[test]
fn test_tui_rename_empty_name_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["create", "test-branch"]).assert_success();

    // Rename without providing a name should fail
    let output = repo.run_stax(&["rename"]);
    // This might prompt for input or fail - either way shouldn't hang
    // In non-interactive mode it should fail
    assert!(!output.status.success() || !TestRepo::stderr(&output).is_empty());
}

/// Test checkout command works (TUI KeyAction::Enter on non-current branch)
#[test]
fn test_tui_checkout_branch() {
    let repo = TestRepo::new();

    // Create branches
    repo.create_stack(&["feature-1", "feature-2"]);

    // Get branch names
    let branches = repo.list_branches();
    let feature1 = branches.iter().find(|b| b.contains("feature-1")).unwrap();

    // Should be on feature-2
    assert!(repo.current_branch().contains("feature-2"));

    // Checkout feature-1 (simulates TUI checkout)
    let output = repo.run_stax(&["checkout", feature1]);
    output.assert_success();

    // Verify we switched
    assert!(repo.current_branch().contains("feature-1"));
}

/// Test navigation commands work (TUI uses these internally)
#[test]
fn test_tui_navigation_up_down() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["nav-1", "nav-2", "nav-3"]);

    // Should be at top (nav-3)
    assert!(repo.current_branch().contains("nav-3"));

    // Navigate down
    repo.run_stax(&["down"]).assert_success();
    assert!(repo.current_branch().contains("nav-2"));

    // Navigate down again
    repo.run_stax(&["down"]).assert_success();
    assert!(repo.current_branch().contains("nav-1"));

    // Navigate up
    repo.run_stax(&["up"]).assert_success();
    assert!(repo.current_branch().contains("nav-2"));
}

/// Test trunk navigation (TUI uses this)
#[test]
fn test_tui_navigate_to_trunk() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["create", "feature"]).assert_success();
    assert!(repo.current_branch().contains("feature"));

    // Navigate to trunk (TUI uses checkout --trunk internally)
    let output = repo.run_stax(&["trunk"]);
    output.assert_success();

    assert_eq!(repo.current_branch(), "main");
}

/// Test that status command works (TUI refreshes this frequently)
#[test]
fn test_tui_status_refresh() {
    let repo = TestRepo::new();

    // Create some branches
    repo.create_stack(&["feature-1", "feature-2"]);

    // Status should work and show branches
    let output = repo.run_stax(&["status"]);
    output
        .assert_success()
        .assert_stdout_contains("feature-1")
        .assert_stdout_contains("feature-2");

    // JSON status should also work (used for branch list)
    let output = repo.run_stax(&["status", "--json"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("\"name\""));
}
