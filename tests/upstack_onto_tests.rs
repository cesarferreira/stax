//! Tests for `st upstack onto` -- mass reparent current + descendants onto new parent.

mod common;

use common::{OutputAssertions, TestRepo};

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
    assert!(
        stdout.contains("Reparented"),
        "Should mention reparenting: {}",
        stdout
    );
    assert!(
        stdout.contains("descendant"),
        "Should mention descendants: {}",
        stdout
    );

    // Verify b's parent is now main (via status --json)
    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");

    let b_entry = branches.iter().find(|b| {
        b["name"]
            .as_str()
            .map(|n| n.contains("b"))
            .unwrap_or(false)
    });
    assert!(b_entry.is_some(), "Should find branch b in status");
    let b_parent = b_entry.unwrap()["parent"].as_str().unwrap_or("");
    assert_eq!(b_parent, "main", "b's parent should be main, got: {}", b_parent);

    // c should still be a descendant of b (not moved to main)
    let c_entry = branches.iter().find(|b| {
        b["name"]
            .as_str()
            .map(|n| n.contains("c"))
            .unwrap_or(false)
    });
    assert!(c_entry.is_some(), "Should find branch c in status");
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

    // Build: main -> a -> b
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Go to a, try to reparent onto b (descendant) -- should fail
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_failure();
    output.assert_stderr_contains("circular");
}

#[test]
fn upstack_onto_single_branch_no_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Build: main -> a, main -> b (siblings, no parent-child)
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["trunk"]).assert_success();
    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Move a onto b (no descendants)
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Should reparent: {}",
        stdout
    );
    // Should NOT mention descendants (none exist)
    assert!(
        !stdout.contains("descendant"),
        "Should not mention descendants for leaf branch: {}",
        stdout
    );
}
