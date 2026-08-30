use crate::common;
use common::{OutputAssertions, TestRepo};
use serde_json::Value;

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
        b_parent.as_ref().is_some_and(|p| p.contains("insert-mid")),
        "B should be reparented to insert-mid, got parent: {:?}",
        b_parent
    );

    repo.run_stax(&["checkout", &extra[0]]);
    let c_parent = repo.get_current_parent();
    assert!(
        c_parent.as_ref().is_some_and(|p| p.contains("insert-mid")),
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
        b_parent.as_ref().is_some_and(|p| p.contains("alias-mid")),
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
        b_parent.as_ref().is_some_and(|p| p.contains("norep-a")),
        "B should still have A as parent, got parent: {:?}",
        b_parent
    );
}

#[test]
fn test_create_insert_from_trunk_reparents_direct_children() {
    let repo = TestRepo::new();

    repo.run_stax(&["status"]).assert_success();

    let trunk_a = repo.create_stack(&["trunk-a"]);
    repo.run_stax(&["checkout", "main"]).assert_success();
    let trunk_b = repo.create_stack(&["trunk-b"]);

    repo.run_stax(&["checkout", "main"]).assert_success();
    let output = repo.run_stax(&["create", "trunk-mid", "--insert"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Expected reparent message from trunk insert, got: {}",
        stdout
    );

    repo.run_stax(&["checkout", &trunk_a[0]]).assert_success();
    let a_parent = repo.get_current_parent();
    assert!(
        a_parent
            .as_ref()
            .is_some_and(|parent| parent.contains("trunk-mid")),
        "trunk-a should be reparented to trunk-mid, got parent: {:?}",
        a_parent
    );

    repo.run_stax(&["checkout", &trunk_b[0]]).assert_success();
    let b_parent = repo.get_current_parent();
    assert!(
        b_parent
            .as_ref()
            .is_some_and(|parent| parent.contains("trunk-mid")),
        "trunk-b should be reparented to trunk-mid, got parent: {:?}",
        b_parent
    );
}

/// Issue #830: `--insert` reparents pre-existing children onto the newly
/// created branch's tip with no rebase. If a child was already stale
/// relative to the parent branch *before* the insert ran, that tip isn't
/// actually in the child's ancestry -- the write must fall back to the
/// child's previously recorded (still-valid) boundary instead.
#[test]
fn test_create_insert_does_not_poison_stale_childs_boundary() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["stale-insert-a", "stale-insert-b"]);
    let branch_a = &branches[0];
    let branch_b = &branches[1];

    let metadata_ref = format!("refs/branch-metadata/{}", branch_b);
    let read_boundary = |repo: &TestRepo| -> String {
        let output = repo.git(&["show", &metadata_ref]);
        assert!(output.status.success());
        let metadata: Value = serde_json::from_str(&TestRepo::stdout(&output)).unwrap();
        metadata["parentBranchRevision"]
            .as_str()
            .expect("parentBranchRevision missing")
            .to_string()
    };
    let recorded_boundary = read_boundary(&repo);

    // Advance A without rebasing B onto it: B is now stale relative to A.
    repo.run_stax(&["checkout", branch_a]).assert_success();
    repo.create_file("a-extra.txt", "more work on A");
    repo.commit("More work on A");

    let output = repo.run_stax(&["create", "stale-insert-mid", "--insert"]);
    output.assert_success();
    assert!(TestRepo::stdout(&output).contains("Reparented"));

    let boundary = read_boundary(&repo);
    let ancestry_check = repo.git(&["merge-base", "--is-ancestor", &boundary, branch_b]);
    assert!(
        ancestry_check.status.success(),
        "Recorded boundary {} is not an ancestor of {} (issue #830)",
        boundary,
        branch_b
    );
    assert_eq!(
        boundary, recorded_boundary,
        "Expected the pre-existing boundary to be preserved when the new branch's tip isn't in B's ancestry"
    );
}
