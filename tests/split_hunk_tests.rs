mod common;

use common::{OutputAssertions, TestRepo};

#[test]
fn test_split_hunk_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["split", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--hunk"),
        "Expected --hunk in help output, got: {}",
        stdout
    );
}

#[test]
fn test_split_hunk_on_trunk_fails() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot split"),
        "Expected trunk error, got: {}",
        stderr
    );
}

#[test]
fn test_split_hunk_untracked_branch_fails() {
    let repo = TestRepo::new();
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("file1.txt", "content");
    repo.commit("commit 1");

    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("track"),
        "Expected untracked error, got: {}",
        stderr
    );
}

#[test]
fn test_split_commit_mode_single_commit_suggests_hunk() {
    let repo = TestRepo::new();
    repo.create_stack(&["single-commit"]);

    let output = repo.run_stax(&["split"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("--hunk") || stderr.contains("hunk"),
        "Expected hint about --hunk for single commit, got: {}",
        stderr
    );
}

#[test]
fn test_split_hunk_requires_terminal() {
    let repo = TestRepo::new();
    repo.create_stack(&["test-branch"]);
    repo.create_file("file1.txt", "content");
    repo.commit("commit 1");

    let output = repo.run_stax(&["split", "--hunk"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("terminal") || stderr.contains("interactive"),
        "Expected terminal requirement error, got: {}",
        stderr
    );
}
