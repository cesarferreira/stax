mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_detach_middle_of_stack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a 3-branch stack: A -> B -> C
    let branches = repo.create_stack(&["detach-a", "detach-b", "detach-c"]);

    // Checkout B (middle branch)
    repo.run_stax(&["checkout", &branches[1]]);
    assert!(repo.current_branch_contains("detach-b"));

    // Detach B
    let output = repo.run_stax(&["detach", "--yes"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Detached") || stdout.contains("detach"),
        "Expected detach confirmation, got: {}",
        stdout
    );

    // Verify B's parent is now trunk
    let parent = repo.get_current_parent();
    assert_eq!(
        parent,
        Some("main".to_string()),
        "Detached branch should have trunk as parent"
    );

    // Verify C was reparented to A
    repo.run_stax(&["checkout", &branches[2]]);
    let c_parent = repo.get_current_parent();
    assert!(
        c_parent.as_ref().map_or(false, |p| p.contains("detach-a")),
        "C should be reparented to A, got parent: {:?}",
        c_parent
    );
}

#[test]
fn test_detach_leaf_branch() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    let branches = repo.create_stack(&["leaf-a", "leaf-b"]);

    // Detach the leaf (top) branch
    repo.run_stax(&["checkout", &branches[1]]);
    let output = repo.run_stax(&["detach", "--yes"]);
    output.assert_success();

    // Leaf branch should now be off trunk
    let parent = repo.get_current_parent();
    assert_eq!(
        parent,
        Some("main".to_string()),
        "Detached leaf should have trunk as parent"
    );
}

#[test]
fn test_detach_trunk_fails() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack so stax is initialized
    repo.create_stack(&["trunk-test"]);
    repo.run_stax(&["t"]); // go to trunk

    // Try to detach trunk
    let output = repo.run_stax(&["detach", "--yes"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot"),
        "Expected trunk error, got stderr: {}",
        stderr
    );
}

#[test]
fn test_detach_preserves_pr_info() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a 3-branch stack
    let branches = repo.create_stack(&["pr-a", "pr-b", "pr-c"]);

    // Detach B, C should be reparented to A
    repo.run_stax(&["checkout", &branches[1]]);
    let output = repo.run_stax(&["detach", "--yes"]);
    output.assert_success();

    // Verify C is still tracked
    repo.run_stax(&["checkout", &branches[2]]);
    let parent = repo.get_current_parent();
    assert!(parent.is_some(), "C should still be tracked after detach");
}

#[test]
fn test_detach_specific_branch() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    let branches = repo.create_stack(&["spec-a", "spec-b"]);

    // Go to trunk and detach spec-a by name
    repo.run_stax(&["t"]);
    let output = repo.run_stax(&["detach", &branches[0], "--yes"]);
    output.assert_success();
}
