//! Integration tests for `stax branch track --all`.

use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;

fn metadata_ref(branch: &str) -> String {
    format!("refs/branch-metadata/{branch}")
}

fn metadata_for(repo: &TestRepo, branch: &str) -> Value {
    let output = repo.git(&["show", &metadata_ref(branch)]);
    output.assert_success();
    serde_json::from_str(&TestRepo::stdout(&output)).expect("valid metadata JSON")
}

fn metadata_oid(repo: &TestRepo, branch: &str) -> String {
    let output = repo.git(&["rev-parse", "--verify", &metadata_ref(branch)]);
    output.assert_success();
    TestRepo::stdout(&output).trim().to_string()
}

fn rewrite_metadata_parent(repo: &TestRepo, branch: &str, parent: &str) {
    let mut metadata = metadata_for(repo, branch);
    metadata["parentBranchName"] = Value::String(parent.to_string());
    metadata["parentBranchRevision"] = Value::String(repo.get_commit_sha(parent));

    let file = tempfile::NamedTempFile::new().expect("metadata file");
    fs::write(file.path(), metadata.to_string()).expect("metadata contents");
    let hash = repo.git(&[
        "hash-object",
        "-w",
        file.path().to_str().expect("metadata path"),
    ]);
    hash.assert_success();
    repo.git(&[
        "update-ref",
        &metadata_ref(branch),
        TestRepo::stdout(&hash).trim(),
    ])
    .assert_success();
}

fn assert_metadata_missing(repo: &TestRepo, branch: &str) {
    let output = repo.git(&["show-ref", "--verify", &metadata_ref(branch)]);
    assert!(
        !output.status.success(),
        "{branch} should not have branch metadata"
    );
}

fn assert_parent(repo: &TestRepo, branch: &str, expected_parent: &str) {
    let metadata = metadata_for(repo, branch);
    assert_eq!(
        metadata["parentBranchName"].as_str(),
        Some(expected_parent),
        "unexpected parent metadata for {branch}: {metadata}"
    );
}

fn commit_on_new_branch(repo: &TestRepo, branch: &str) {
    repo.git(&["checkout", "-b", branch]).assert_success();
    repo.create_file(&format!("{branch}.txt"), branch);
    repo.commit(&format!("Commit {branch}"));
}

#[test]
fn track_all_tracks_raw_git_stack_under_nearest_local_ancestors() {
    let repo = TestRepo::new();
    repo.set_trunk("main");

    commit_on_new_branch(&repo, "raw-root");
    commit_on_new_branch(&repo, "raw-middle");
    commit_on_new_branch(&repo, "raw-leaf");

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "raw-root", "main");
    assert_parent(&repo, "raw-middle", "raw-root");
    assert_parent(&repo, "raw-leaf", "raw-middle");
    assert_metadata_missing(&repo, "main");
}

#[test]
fn track_all_uses_tracked_parent_without_rewriting_existing_metadata() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let tracked_parent = repo.create_stack(&["tracked-parent"]).remove(0);
    let parent_metadata_before = metadata_for(&repo, &tracked_parent);
    let parent_oid_before = metadata_oid(&repo, &tracked_parent);

    commit_on_new_branch(&repo, "raw-child");

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "raw-child", &tracked_parent);
    assert_eq!(metadata_for(&repo, &tracked_parent), parent_metadata_before);
    assert_eq!(metadata_oid(&repo, &tracked_parent), parent_oid_before);
    assert_metadata_missing(&repo, "main");
}

#[test]
fn track_all_falls_back_to_trunk_for_unrelated_history() {
    let repo = TestRepo::new();
    repo.set_trunk("main");

    repo.git(&["checkout", "--orphan", "isolated"])
        .assert_success();
    repo.git(&["rm", "-rf", "."]).assert_success();
    repo.create_file("isolated.txt", "isolated history");
    repo.git(&["add", "isolated.txt"]).assert_success();
    repo.git(&["commit", "-m", "Isolated root commit"])
        .assert_success();
    repo.git(&["checkout", "main"]).assert_success();

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "isolated", "main");
    assert_metadata_missing(&repo, "main");
}

#[test]
fn track_all_does_not_parent_equal_tip_branches_to_each_other() {
    let repo = TestRepo::new();
    repo.set_trunk("main");

    commit_on_new_branch(&repo, "same-tip-a");
    repo.git(&["branch", "same-tip-b", "same-tip-a"])
        .assert_success();

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "same-tip-a", "main");
    assert_parent(&repo, "same-tip-b", "main");
}

#[test]
fn track_all_avoids_cycle_through_existing_tracked_metadata() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let tracked_ancestor = repo.create_stack(&["tracked-ancestor"]).remove(0);
    commit_on_new_branch(&repo, "raw-descendant");
    rewrite_metadata_parent(&repo, &tracked_ancestor, "raw-descendant");

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, &tracked_ancestor, "raw-descendant");
    assert_parent(&repo, "raw-descendant", "main");
    repo.run_stax(&["validate"]).assert_success();
}

#[test]
fn track_all_breaks_equal_distance_ancestor_ties_lexically() {
    let repo = TestRepo::new();
    repo.set_trunk("main");

    commit_on_new_branch(&repo, "z-parent");
    repo.git(&["checkout", "main"]).assert_success();
    commit_on_new_branch(&repo, "a-parent");
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["checkout", "-b", "merge-target"])
        .assert_success();
    repo.git(&["merge", "--no-ff", "a-parent", "-m", "Merge a-parent"])
        .assert_success();
    repo.git(&["merge", "--no-ff", "z-parent", "-m", "Merge z-parent"])
        .assert_success();

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "merge-target", "a-parent");
}

#[test]
fn track_all_second_run_is_a_metadata_preserving_noop() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    commit_on_new_branch(&repo, "raw-branch");
    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();
    let metadata_before = metadata_for(&repo, "raw-branch");
    let oid_before = metadata_oid(&repo, "raw-branch");

    let second_run = repo.run_stax(&["branch", "track", "--all"]);

    second_run
        .assert_success()
        .assert_stdout_contains("No untracked local branches to track.");
    assert_eq!(metadata_for(&repo, "raw-branch"), metadata_before);
    assert_eq!(metadata_oid(&repo, "raw-branch"), oid_before);
}

#[test]
fn track_all_handles_branch_names_that_collide_with_tags() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    commit_on_new_branch(&repo, "shared-name");
    repo.git(&["tag", "shared-name", "main"]).assert_success();
    commit_on_new_branch(&repo, "raw-child");

    repo.run_stax(&["branch", "track", "--all"])
        .assert_success();

    assert_parent(&repo, "shared-name", "main");
    assert_parent(&repo, "raw-child", "shared-name");
}

#[test]
fn track_all_conflicts_with_other_bulk_or_explicit_parent_modes() {
    let repo = TestRepo::new();

    for conflicting_args in [
        ["branch", "track", "--all", "--parent", "main"].as_slice(),
        ["branch", "track", "--all", "--all-prs"].as_slice(),
    ] {
        let output = repo.run_stax(conflicting_args);
        output.assert_failure();
        assert!(
            TestRepo::stderr(&output).contains("cannot be used with"),
            "expected Clap conflict for {conflicting_args:?}, got: {}",
            TestRepo::stderr(&output)
        );
    }
}
