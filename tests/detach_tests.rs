use crate::common;
use common::{OutputAssertions, TestRepo};

/// Read the newest op receipt under `.git/stax/ops` as JSON.
fn latest_receipt(repo: &TestRepo) -> serde_json::Value {
    let ops_dir = repo.path().join(".git/stax/ops");
    let mut entries: Vec<_> = std::fs::read_dir(&ops_dir)
        .expect("read ops dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    let newest = entries.last().expect("expected at least one op receipt");
    let content = std::fs::read_to_string(newest.path()).expect("read receipt");
    serde_json::from_str(&content).expect("invalid receipt JSON")
}

/// Read a branch's stax metadata blob (`refs/branch-metadata/<branch>`) as JSON.
fn metadata_json(repo: &TestRepo, branch: &str) -> serde_json::Value {
    let output = repo.git(&["show", &format!("refs/branch-metadata/{}", branch)]);
    serde_json::from_str(&TestRepo::stdout(&output))
        .unwrap_or_else(|_| panic!("metadata for {} should be JSON", branch))
}

#[test]
fn detach_records_metadata_refs_in_the_op_receipt() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    let branches = repo.create_stack(&["meta-a", "meta-b", "meta-c"]);
    let (b, c) = (&branches[1], &branches[2]);

    repo.run_stax(&["checkout", b]).assert_success();
    repo.run_stax(&["detach", "--yes"]).assert_success();

    let receipt = latest_receipt(&repo);
    assert_eq!(receipt["kind"], "detach");
    assert_eq!(receipt["status"], "success");
    let local_refs = receipt["local_refs"].as_array().expect("local_refs array");

    for branch in [b, c] {
        let label = format!("{}@meta", branch);
        let entry = local_refs
            .iter()
            .find(|e| e["branch"].as_str() == Some(label.as_str()))
            .unwrap_or_else(|| panic!("expected {} entry in receipt: {:#}", label, receipt));
        assert_eq!(
            entry["refname"].as_str(),
            Some(format!("refs/branch-metadata/{}", branch).as_str())
        );
        assert!(
            entry["oid_before"].is_string(),
            "{} needs a before-OID",
            label
        );
        assert!(
            entry["oid_after"].is_string(),
            "{} needs an after-OID",
            label
        );
        assert_ne!(
            entry["oid_before"], entry["oid_after"],
            "{}'s metadata blob changed, the receipt must show it",
            label
        );
    }
}

#[test]
fn detach_undo_restores_reparented_children_metadata() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    let branches = repo.create_stack(&["undo-a", "undo-b", "undo-c"]);
    let (a, b, c) = (&branches[0], &branches[1], &branches[2]);

    let c_meta_before = metadata_json(&repo, c);
    assert_eq!(c_meta_before["parentBranchName"].as_str(), Some(b.as_str()));

    repo.run_stax(&["checkout", b]).assert_success();
    repo.run_stax(&["detach", "--yes"]).assert_success();

    // Precondition: detach really did reparent c onto a.
    let c_meta_after = metadata_json(&repo, c);
    assert_eq!(c_meta_after["parentBranchName"].as_str(), Some(a.as_str()));

    repo.run_stax(&["undo", "--yes"]).assert_success();

    let c_meta_undone = metadata_json(&repo, c);
    assert_eq!(
        c_meta_undone["parentBranchName"].as_str(),
        Some(b.as_str()),
        "undo must restore c's parent to b, got: {:#}",
        c_meta_undone
    );
    assert_eq!(
        c_meta_undone["parentBranchRevision"], c_meta_before["parentBranchRevision"],
        "undo must restore c's recorded parent revision exactly"
    );
}

#[test]
fn detach_undo_restores_the_detached_branchs_own_metadata() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    let branches = repo.create_stack(&["self-a", "self-b"]);
    let (a, b) = (&branches[0], &branches[1]);

    let b_meta_before = metadata_json(&repo, b);
    assert_eq!(b_meta_before["parentBranchName"].as_str(), Some(a.as_str()));

    repo.run_stax(&["checkout", b]).assert_success();
    repo.run_stax(&["detach", "--yes"]).assert_success();
    assert_eq!(
        metadata_json(&repo, b)["parentBranchName"].as_str(),
        Some("main")
    );

    repo.run_stax(&["undo", "--yes"]).assert_success();

    let b_meta_undone = metadata_json(&repo, b);
    assert_eq!(
        b_meta_undone["parentBranchName"].as_str(),
        Some(a.as_str()),
        "undo must restore b's parent to a, got: {:#}",
        b_meta_undone
    );
    assert_eq!(
        b_meta_undone["parentBranchRevision"],
        b_meta_before["parentBranchRevision"]
    );
}

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
        c_parent.as_ref().is_some_and(|p| p.contains("detach-a")),
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
