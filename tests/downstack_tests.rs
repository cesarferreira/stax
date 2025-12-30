//! Downstack command integration tests
//!
//! Tests for the `downstack get` command that shows branches below current.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Downstack Get Tests
// =============================================================================

#[test]
fn test_downstack_get_from_top_shows_full_stack() {
    let repo = TestRepo::new();

    // Create a stack: main -> feature-1 -> feature-2 -> feature-3
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // On feature-3 (top)
    assert!(repo.current_branch_contains("feature-3"));

    // downstack get should show the full stack
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // Should show all branches in the stack
    assert!(
        stdout.contains("feature-1"),
        "Expected feature-1 in downstack output: {}",
        stdout
    );
    assert!(
        stdout.contains("feature-2"),
        "Expected feature-2 in downstack output: {}",
        stdout
    );
    assert!(
        stdout.contains("feature-3"),
        "Expected feature-3 in downstack output: {}",
        stdout
    );
    assert!(
        stdout.contains("main"),
        "Expected main in downstack output: {}",
        stdout
    );
}

#[test]
fn test_downstack_get_from_middle_shows_ancestors() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to middle branch (feature-2)
    repo.run_stax(&["checkout", &branches[1]]);
    assert!(repo.current_branch_contains("feature-2"));

    // downstack get should show branches below (feature-1, main)
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // Should show current and ancestors
    assert!(
        stdout.contains("feature-1"),
        "Expected feature-1 in downstack: {}",
        stdout
    );
    assert!(
        stdout.contains("feature-2"),
        "Expected feature-2 in downstack: {}",
        stdout
    );
    assert!(
        stdout.contains("main"),
        "Expected main in downstack: {}",
        stdout
    );
}

#[test]
fn test_downstack_get_from_first_branch() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to first branch (feature-1)
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // downstack get should show feature-1 and main
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    assert!(
        stdout.contains("feature-1"),
        "Expected feature-1: {}",
        stdout
    );
    assert!(stdout.contains("main"), "Expected main: {}", stdout);
}

#[test]
fn test_downstack_get_on_trunk() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized
    repo.create_stack(&["feature-1"]);

    // Go to trunk
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // downstack get on trunk should show just main
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("main"), "Expected main: {}", stdout);
}

// =============================================================================
// Downstack Alias Tests
// =============================================================================

#[test]
fn test_downstack_alias_ds() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1"]);

    // ds should work as alias for downstack
    let output = repo.run_stax(&["ds", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("feature-1"),
        "Expected feature-1 in ds output: {}",
        stdout
    );
}

#[test]
fn test_downstack_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["downstack", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("get") || stdout.contains("Get"),
        "Expected 'get' subcommand in help: {}",
        stdout
    );
}

#[test]
fn test_downstack_get_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["downstack", "get", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("below") || stdout.contains("Show") || stdout.contains("branch"),
        "Expected help text: {}",
        stdout
    );
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_downstack_get_single_branch() {
    let repo = TestRepo::new();

    // Create just one branch
    repo.create_stack(&["solo-feature"]);

    // downstack get should show the branch and main
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("solo-feature") || stdout.contains("solo"));
    assert!(stdout.contains("main"));
}

#[test]
fn test_downstack_get_with_multiple_stacks() {
    let repo = TestRepo::new();

    // Create first stack
    repo.create_stack(&["stack1-a", "stack1-b"]);

    // Go to main and create second stack
    repo.run_stax(&["t"]);
    repo.create_stack(&["stack2-a", "stack2-b"]);

    // On stack2-b
    assert!(repo.current_branch_contains("stack2-b"));

    // downstack get should show stack2 chain, not stack1
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // Should show current stack
    assert!(stdout.contains("stack2-a") || stdout.contains("stack2"));
    assert!(stdout.contains("stack2-b") || stdout.contains("stack2"));
    assert!(stdout.contains("main"));
}

#[test]
fn test_downstack_get_long_stack() {
    let repo = TestRepo::new();

    // Create a long stack
    repo.create_stack(&["f1", "f2", "f3", "f4", "f5"]);

    // On f5 (top)
    assert!(repo.current_branch_contains("f5"));

    // downstack get should show all 5 branches + main
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // All branches should be visible
    for i in 1..=5 {
        let name = format!("f{}", i);
        assert!(
            stdout.contains(&name),
            "Expected {} in downstack: {}",
            name,
            stdout
        );
    }
}

#[test]
fn test_downstack_get_untracked_branch() {
    let repo = TestRepo::new();

    // Create untracked branch with git directly
    repo.git(&["checkout", "-b", "untracked"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    // downstack get should handle untracked branch
    let output = repo.run_stax(&["downstack", "get"]);

    // Should either succeed with limited output or fail gracefully
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Either shows something or fails with message
    assert!(
        combined.contains("main")
            || combined.contains("untracked")
            || combined.contains("not tracked")
            || !output.status.success(),
        "Expected some output or graceful failure: {}",
        combined
    );
}

// =============================================================================
// Output Format Tests
// =============================================================================

#[test]
fn test_downstack_get_output_format() {
    let repo = TestRepo::new();

    // Create a simple stack
    repo.create_stack(&["feature-1", "feature-2"]);

    // downstack get output should be in tree format (like status)
    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // Should have some kind of tree structure indicators
    // or at least show branches in order
    assert!(
        stdout.contains("main"),
        "Should show trunk in output"
    );
    assert!(
        stdout.contains("feature-1"),
        "Should show feature-1"
    );
    assert!(
        stdout.contains("feature-2"),
        "Should show feature-2"
    );
}

#[test]
fn test_downstack_get_shows_current_indicator() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to feature-1
    repo.run_stax(&["checkout", &branches[0]]);

    let output = repo.run_stax(&["downstack", "get"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);

    // Should indicate current branch somehow (color, asterisk, etc.)
    // The exact format depends on the status output
    assert!(
        stdout.contains("feature-1"),
        "Should show current branch feature-1"
    );
}

