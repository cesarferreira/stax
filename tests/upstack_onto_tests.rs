//! Tests for `st upstack onto` -- mass reparent current + descendants onto new parent.

mod common;

use common::{OutputAssertions, TestRepo};

/// Helper: find a branch entry in status JSON by exact suffix match.
fn find_branch<'a>(
    branches: &'a [serde_json::Value],
    suffix: &str,
) -> Option<&'a serde_json::Value> {
    branches
        .iter()
        .find(|b| b["name"].as_str().map(|n| n.ends_with(suffix)).unwrap_or(false))
}

#[test]
fn upstack_onto_moves_branch_and_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Build: main -> a -> b -> c
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    repo.run_stax(&["create", "c"]).assert_success();
    repo.create_file("c.txt", "c");
    repo.commit("commit c");

    // Go to b, run upstack onto main
    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Reparented"), "Should mention reparenting: {}", stdout);
    assert!(stdout.contains("descendant"), "Should mention descendants: {}", stdout);

    // Verify b's parent is now main
    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");

    let b_entry = find_branch(branches, "b").expect("should find branch b");
    assert_eq!(b_entry["parent"].as_str().unwrap(), "main");

    // c should still be a child of b (subtree preserved)
    let c_entry = find_branch(branches, "c").expect("should find branch c");
    let c_parent = c_entry["parent"].as_str().unwrap();
    assert!(
        c_parent.ends_with("b"),
        "c's parent should still be b, got: {}",
        c_parent
    );
}

#[test]
fn upstack_onto_from_trunk_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_failure();
    output.assert_stderr_contains("trunk");
}

#[test]
fn upstack_onto_circular_dependency_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Go to a, try to reparent onto b (descendant)
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_failure();
    output.assert_stderr_contains("circular");
}

#[test]
fn upstack_onto_single_branch_no_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Build: main -> a, main -> b (siblings)
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["trunk"]).assert_success();
    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Move a onto b
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Reparented"), "Should reparent: {}", stdout);
    assert!(!stdout.contains("descendant"), "Leaf branch should have no descendants: {}", stdout);
}

#[test]
fn upstack_onto_same_parent_is_noop() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    // Try to reparent onto the current parent (main)
    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("already parented") || stdout.contains("Nothing to do"),
        "Should detect no-op: {}",
        stdout
    );
}

#[test]
fn upstack_onto_nonexistent_target_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    let output = repo.run_stax(&["upstack", "onto", "nonexistent"]);
    output.assert_failure();
    output.assert_stderr_contains("does not exist");
}

#[test]
fn upstack_onto_self_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    let output = repo.run_stax(&["upstack", "onto", "a"]);
    output.assert_failure();
    output.assert_stderr_contains("itself");
}
