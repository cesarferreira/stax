//! Navigation command integration tests
//!
//! Tests for top, bottom, up, down commands that navigate the branch stack.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Top Command Tests
// =============================================================================

#[test]
fn test_top_from_trunk() {
    let repo = TestRepo::new();

    // Create a stack: main -> feature-1 -> feature-2 -> feature-3
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go back to main
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Navigate to top should go to feature-3 (the leaf)
    let output = repo.navigate_to_top();
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-3"),
        "Expected to be on feature-3, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_top_from_middle_of_stack() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to the middle branch
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // Navigate to top
    let output = repo.navigate_to_top();
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-3"),
        "Expected to be on feature-3, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_top_already_at_top() {
    let repo = TestRepo::new();

    // Create a single branch
    repo.create_stack(&["feature-1"]);
    assert!(repo.current_branch_contains("feature-1"));

    // We're already at the top
    let output = repo.navigate_to_top();
    output.assert_success();

    // Should still be on feature-1
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_top_on_trunk_with_no_children() {
    let repo = TestRepo::new();

    // No branches created, just trunk
    assert_eq!(repo.current_branch(), "main");

    let output = repo.navigate_to_top();
    // Should handle gracefully (either success with message or stay on main)
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Either stays on main or gives a message
    assert!(
        repo.current_branch() == "main"
            || combined.contains("top")
            || combined.contains("no")
            || combined.contains("Already"),
        "Unexpected behavior: {}",
        combined
    );
}

// =============================================================================
// Bottom Command Tests
// =============================================================================

#[test]
fn test_bottom_from_top_of_stack() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Should be on feature-3 (top)
    assert!(repo.current_branch_contains("feature-3"));

    // Navigate to bottom (feature-1, first branch above trunk)
    let output = repo.navigate_to_bottom();
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-1"),
        "Expected to be on feature-1, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_bottom_from_middle_of_stack() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to middle
    repo.run_stax(&["checkout", &branches[1]]);
    assert!(repo.current_branch_contains("feature-2"));

    // Navigate to bottom
    let output = repo.navigate_to_bottom();
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-1"),
        "Expected to be on feature-1, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_bottom_already_at_bottom() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to feature-1 (bottom of stack, first above trunk)
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // Navigate to bottom - should stay on feature-1
    let output = repo.navigate_to_bottom();
    output.assert_success();
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_bottom_from_trunk() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2"]);

    // Go to trunk
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Navigate to bottom - should go to feature-1 (first branch above trunk)
    let output = repo.navigate_to_bottom();
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-1"),
        "Expected feature-1, got {}",
        repo.current_branch()
    );
}

// =============================================================================
// Up Command Tests
// =============================================================================

#[test]
fn test_up_single_step() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to feature-1
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // Move up one
    let output = repo.navigate_up(None);
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-2"),
        "Expected feature-2, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_up_with_count() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to feature-1
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // Move up 2
    let output = repo.navigate_up(Some(2));
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-3"),
        "Expected feature-3, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_up_from_trunk() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2"]);

    // Go to trunk
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Move up - should go to feature-1
    let output = repo.navigate_up(None);
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-1"),
        "Expected feature-1, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_up_already_at_top() {
    let repo = TestRepo::new();

    // Create a single branch
    repo.create_stack(&["feature-1"]);
    assert!(repo.current_branch_contains("feature-1"));

    // Move up when already at top
    let output = repo.navigate_up(None);
    output.assert_success();

    // Should still be on feature-1 with appropriate message
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_up_count_exceeds_stack() {
    let repo = TestRepo::new();

    // Create a small stack
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to feature-1
    repo.run_stax(&["checkout", &branches[0]]);

    // Try to move up 10 (more than stack has)
    let output = repo.navigate_up(Some(10));
    output.assert_success();

    // Should be at the top (feature-2)
    assert!(
        repo.current_branch_contains("feature-2"),
        "Expected feature-2 (top), got {}",
        repo.current_branch()
    );
}

// =============================================================================
// Down Command Tests
// =============================================================================

#[test]
fn test_down_single_step() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Already on feature-3 (top)
    assert!(repo.current_branch_contains("feature-3"));

    // Move down one
    let output = repo.navigate_down(None);
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-2"),
        "Expected feature-2, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_down_with_count() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Already on feature-3 (top)
    assert!(repo.current_branch_contains("feature-3"));

    // Move down 2
    let output = repo.navigate_down(Some(2));
    output.assert_success();
    assert!(
        repo.current_branch_contains("feature-1"),
        "Expected feature-1, got {}",
        repo.current_branch()
    );
}

#[test]
fn test_down_to_trunk() {
    let repo = TestRepo::new();

    // Create a single branch
    let _branches = repo.create_stack(&["feature-1"]);

    // On feature-1
    assert!(repo.current_branch_contains("feature-1"));

    // Move down should go to main
    let output = repo.navigate_down(None);
    output.assert_success();
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_down_already_at_trunk() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized
    repo.create_stack(&["feature-1"]);
    repo.run_stax(&["t"]);

    // On trunk
    assert_eq!(repo.current_branch(), "main");

    // Move down should stay on trunk with message
    let output = repo.navigate_down(None);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        repo.current_branch() == "main",
        "Should still be on main"
    );
    assert!(
        stdout.contains("bottom") || stdout.contains("trunk") || stdout.contains("Already"),
        "Expected message about being at bottom, got: {}",
        stdout
    );
}

#[test]
fn test_down_count_exceeds_stack() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2"]);

    // On feature-2
    assert!(repo.current_branch_contains("feature-2"));

    // Try to move down 10
    let output = repo.navigate_down(Some(10));
    output.assert_success();

    // Should be at trunk
    assert_eq!(
        repo.current_branch(),
        "main",
        "Expected main (bottom), got {}",
        repo.current_branch()
    );
}

// =============================================================================
// Combined Navigation Tests
// =============================================================================

#[test]
fn test_navigation_roundtrip() {
    let repo = TestRepo::new();

    // Create a stack
    repo.create_stack(&["feature-1", "feature-2", "feature-3"]);

    // Go to trunk
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // top -> feature-3
    repo.navigate_to_top();
    assert!(repo.current_branch_contains("feature-3"));

    // bottom -> feature-1
    repo.navigate_to_bottom();
    assert!(repo.current_branch_contains("feature-1"));

    // up -> feature-2
    repo.navigate_up(None);
    assert!(repo.current_branch_contains("feature-2"));

    // down -> feature-1
    repo.navigate_down(None);
    assert!(repo.current_branch_contains("feature-1"));

    // down -> main
    repo.navigate_down(None);
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_navigation_with_multiple_stacks() {
    let repo = TestRepo::new();

    // Create first stack
    repo.create_stack(&["stack1-a", "stack1-b"]);

    // Go back to main and create second independent stack
    repo.run_stax(&["t"]);
    repo.create_stack(&["stack2-a", "stack2-b"]);

    // On stack2-b
    assert!(repo.current_branch_contains("stack2-b"));

    // bottom should go to stack2-a (bottom of current stack)
    repo.navigate_to_bottom();
    assert!(repo.current_branch_contains("stack2-a"));

    // top should go back to stack2-b
    repo.navigate_to_top();
    assert!(repo.current_branch_contains("stack2-b"));

    // Switch to stack1
    repo.run_stax(&["checkout", "stack1-b"]);
    assert!(repo.current_branch_contains("stack1-b"));

    // bottom in this stack should be stack1-a
    repo.navigate_to_bottom();
    assert!(repo.current_branch_contains("stack1-a"));
}

#[test]
fn test_bu_and_bd_shortcuts_equivalent() {
    let repo = TestRepo::new();

    // Create a stack
    let branches = repo.create_stack(&["feature-1", "feature-2"]);

    // Go to feature-1
    repo.run_stax(&["checkout", &branches[0]]);
    assert!(repo.current_branch_contains("feature-1"));

    // bu should work same as up
    let output = repo.run_stax(&["bu"]);
    output.assert_success();
    assert!(repo.current_branch_contains("feature-2"));

    // bd should work same as down
    let output = repo.run_stax(&["bd"]);
    output.assert_success();
    assert!(repo.current_branch_contains("feature-1"));
}

