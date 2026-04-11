//! Tests for `st create` rollback behavior when commit fails.
//!
//! Verifies that when a pre-commit hook (or other commit failure) occurs during
//! `st create -m`, the branch and metadata are cleaned up so the user can retry
//! without accumulating orphaned branches (mybranch, mybranch-2, mybranch-3, ...).
//!
//! Most tests require Unix (pre-commit hooks use `#!/bin/sh` and chmod).

mod common;

use common::{OutputAssertions, TestRepo};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Install a pre-commit hook that always fails.
#[cfg(unix)]
fn install_failing_pre_commit_hook(repo: &TestRepo) {
    let hooks_dir = repo.path().join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    fs::write(&hook_path, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Remove the pre-commit hook.
#[cfg(unix)]
fn remove_pre_commit_hook(repo: &TestRepo) {
    let hook_path = repo.path().join(".git").join("hooks").join("pre-commit");
    let _ = std::fs::remove_file(hook_path);
}

#[test]
#[cfg(unix)]
fn create_with_failing_hook_rolls_back_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");

    install_failing_pre_commit_hook(&repo);

    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    output.assert_failure();

    // Branch should NOT exist after rollback
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b.contains("my-feature")),
        "Orphaned branch should not exist after rollback. Branches: {:?}",
        branches
    );

    // Should be back on main
    assert_eq!(repo.current_branch(), "main");

    // Error message should mention rollback
    output.assert_stderr_contains("rolled back");
}

#[test]
#[cfg(unix)]
fn create_retry_after_hook_failure_uses_same_name() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");

    install_failing_pre_commit_hook(&repo);

    // First attempt: should fail and roll back
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_failure();

    // Second attempt: should also fail and roll back (no -2 suffix)
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_failure();

    // No branches with suffixes should exist
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b.contains("my-feature-2")),
        "No suffixed branch should exist. Branches: {:?}",
        branches
    );
}

#[test]
fn create_with_successful_commit_still_works() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");

    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_success();

    assert!(
        repo.current_branch().contains("my-feature"),
        "Should be on the new branch. Current: {}",
        repo.current_branch()
    );
}

#[test]
#[cfg(unix)]
fn create_without_message_unaffected_by_hook() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    install_failing_pre_commit_hook(&repo);

    // Create with just a name (no -m, no commit attempt)
    repo.run_stax(&["create", "my-branch"]).assert_success();

    assert!(
        repo.current_branch().contains("my-branch"),
        "Branch should be created. Current: {}",
        repo.current_branch()
    );
}

#[test]
#[cfg(unix)]
fn create_rollback_then_succeed_after_hook_removed() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");

    // Install failing hook -> create fails
    install_failing_pre_commit_hook(&repo);
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_failure();

    // Remove hook -> retry should succeed with the same branch name
    remove_pre_commit_hook(&repo);
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_success();

    let branch = repo.current_branch();
    assert!(branch.contains("my-feature"), "Should be on the feature branch: {}", branch);
    assert!(!branch.contains("my-feature-2"), "Should not have a -2 suffix: {}", branch);
}
