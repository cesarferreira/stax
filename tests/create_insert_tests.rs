mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_create_insert_reparents_children() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack: main -> A -> B, main -> A -> C
    let branches = repo.create_stack(&["insert-a", "insert-b"]);

    // Go back to A to create another child
    repo.run_stax(&["checkout", &branches[0]]);
    let extra = repo.create_stack(&["insert-c"]);

    // Now A has children: B and C
    // Go back to A and create a new branch with --insert
    repo.run_stax(&["checkout", &branches[0]]);
    let output = repo.run_stax(&["create", "insert-mid", "--insert"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Expected reparent message, got: {}",
        stdout
    );
    assert!(
        stdout.contains("restack"),
        "Expected restack hint, got: {}",
        stdout
    );

    // The new branch should be the current branch
    assert!(repo.current_branch_contains("insert-mid"));

    // B and C should now have insert-mid as parent
    repo.run_stax(&["checkout", &branches[1]]);
    let b_parent = repo.get_current_parent();
    assert!(
        b_parent
            .as_ref()
            .map_or(false, |p| p.contains("insert-mid")),
        "B should be reparented to insert-mid, got parent: {:?}",
        b_parent
    );

    repo.run_stax(&["checkout", &extra[0]]);
    let c_parent = repo.get_current_parent();
    assert!(
        c_parent
            .as_ref()
            .map_or(false, |p| p.contains("insert-mid")),
        "C should be reparented to insert-mid, got parent: {:?}",
        c_parent
    );
}

#[test]
fn test_create_insert_no_children_noop() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a single branch (leaf with no children)
    let _branches = repo.create_stack(&["leaf-only"]);

    // Use --insert on a leaf branch (no children to reparent)
    let output = repo.run_stax(&["create", "after-leaf", "--insert"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    // Should NOT contain reparent message since there were no children
    assert!(
        !stdout.contains("Reparented"),
        "Should not reparent when there are no children, got: {}",
        stdout
    );

    // The new branch should be current and stacked on the leaf
    assert!(repo.current_branch_contains("after-leaf"));
}

#[test]
fn test_create_insert_via_bc_alias() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack: main -> A -> B
    let branches = repo.create_stack(&["alias-a", "alias-b"]);

    // Go back to A and use bc (alias) with --insert
    repo.run_stax(&["checkout", &branches[0]]);
    let output = repo.run_stax(&["bc", "alias-mid", "--insert"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Expected reparent message via bc alias, got: {}",
        stdout
    );

    // B should now have alias-mid as parent
    repo.run_stax(&["checkout", &branches[1]]);
    let b_parent = repo.get_current_parent();
    assert!(
        b_parent
            .as_ref()
            .map_or(false, |p| p.contains("alias-mid")),
        "B should be reparented to alias-mid, got parent: {:?}",
        b_parent
    );
}

#[test]
fn test_create_without_insert_does_not_reparent() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack: main -> A -> B
    let branches = repo.create_stack(&["norep-a", "norep-b"]);

    // Go back to A and create a branch WITHOUT --insert
    repo.run_stax(&["checkout", &branches[0]]);
    let output = repo.run_stax(&["create", "norep-sibling"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("Reparented"),
        "Should not reparent without --insert, got: {}",
        stdout
    );

    // B should still have A as parent (not norep-sibling)
    repo.run_stax(&["checkout", &branches[1]]);
    let b_parent = repo.get_current_parent();
    assert!(
        b_parent
            .as_ref()
            .map_or(false, |p| p.contains("norep-a")),
        "B should still have A as parent, got parent: {:?}",
        b_parent
    );
}
