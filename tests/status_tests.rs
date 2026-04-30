mod common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;

fn create_missing_parent_stack(repo: &TestRepo) -> (String, String) {
    repo.run_stax(&["bc", "missing-parent"]).assert_success();
    let parent = repo.current_branch();

    repo.run_stax(&["bc", "missing-child"]).assert_success();
    let child = repo.current_branch();

    repo.run_stax(&["t"]).assert_success();
    let delete_parent = repo.git(&["branch", "-D", &parent]);
    assert!(
        delete_parent.status.success(),
        "Failed to delete parent branch: {}",
        TestRepo::stderr(&delete_parent)
    );

    (parent, child)
}

#[test]
fn status_json_reports_missing_parent_branch_once() {
    let repo = TestRepo::new();
    let (parent, child) = create_missing_parent_stack(&repo);

    let output = repo.run_stax(&["status", "--json"]);
    output.assert_success();

    let status: Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("status JSON should parse");
    let branches = status["branches"]
        .as_array()
        .expect("status JSON should include branches");
    let child_entries: Vec<_> = branches
        .iter()
        .filter(|entry| entry["name"].as_str() == Some(child.as_str()))
        .collect();

    assert_eq!(
        child_entries.len(),
        1,
        "missing-parent child should appear once in status JSON: {}",
        TestRepo::stdout(&output)
    );
    assert_eq!(child_entries[0]["missing_parent"], parent);
}

#[test]
fn status_labels_missing_parent_instead_of_needs_restack() {
    let repo = TestRepo::new();
    let (parent, child) = create_missing_parent_stack(&repo);

    let output = repo.run_stax(&["status"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let child_lines: Vec<_> = stdout
        .lines()
        .filter(|line| line.contains(&child))
        .collect();

    assert_eq!(
        child_lines.len(),
        1,
        "missing-parent child should appear once in status output: {stdout}"
    );
    assert!(
        child_lines[0].contains("missing parent") && child_lines[0].contains(&parent),
        "expected missing-parent label for child branch, got: {}",
        child_lines[0]
    );
    assert!(
        !child_lines[0].contains("needs restack"),
        "missing-parent branch should not be labeled as ordinary restack work: {}",
        child_lines[0]
    );
    assert!(
        stdout.contains("stax fix --yes"),
        "status should point users at metadata repair, got: {stdout}"
    );
    assert!(
        !stdout.contains("stax rs --restack"),
        "missing-parent-only status should not suggest restack, got: {stdout}"
    );
}
