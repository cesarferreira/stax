//! Tests for `st create` rollback behavior when commit fails.
//!
//! Verifies that when a pre-commit hook (or other commit failure) occurs during
//! `st create -m`, the branch and metadata are cleaned up so the user can retry
//! without accumulating orphaned branches (mybranch, mybranch-2, mybranch-3, ...).
//!
//! These tests require Unix (pre-commit hooks use `#!/bin/sh` and chmod).

mod common;

use common::{OutputAssertions, TestRepo};
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(unix)]
/// Install a pre-commit hook that always fails.
fn install_failing_pre_commit_hook(repo: &TestRepo) {
    let hooks_dir = repo.path().join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    fs::write(&hook_path, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
/// Remove the pre-commit hook.
fn remove_pre_commit_hook(repo: &TestRepo) {
    let hook_path = repo.path().join(".git").join("hooks").join("pre-commit");
    let _ = fs::remove_file(hook_path);
}

#[test]
#[cfg(unix)]
fn create_with_failing_hook_rolls_back_branch() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["init"]).assert_success();

    // Create a file to commit
    repo.create_file("test.txt", "hello");

    // Install a hook that always fails
    install_failing_pre_commit_hook(&repo);

    // Try to create a branch with -a -m (should fail due to hook)
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    assert!(
        !output.status.success(),
        "Expected create to fail due to pre-commit hook"
    );

    // Branch should NOT exist after rollback
    let branches = repo.list_branches();
    let orphan = branches.iter().any(|b| b.contains("my-feature"));
    assert!(
        !orphan,
        "Orphaned branch should not exist after rollback. Branches: {:?}",
        branches
    );

    // Should be back on main
    assert_eq!(
        repo.current_branch(),
        "main",
        "Should be back on original branch after rollback"
    );
}

#[test]
#[cfg(unix)]
fn create_retry_after_hook_failure_uses_same_name() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["init"]).assert_success();

    // Create a file to commit
    repo.create_file("test.txt", "hello");

    // Install a hook that always fails
    install_failing_pre_commit_hook(&repo);

    // First attempt: should fail and roll back
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    assert!(!output.status.success());

    // Second attempt: should also fail and roll back (no -2 suffix)
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    assert!(!output.status.success());

    // No branches with suffixes should exist
    let branches = repo.list_branches();
    let suffixed = branches.iter().any(|b| b.contains("my-feature-2"));
    assert!(
        !suffixed,
        "No suffixed branch should exist. Branches: {:?}",
        branches
    );
}

#[test]
#[cfg(unix)]
fn create_with_successful_commit_still_works() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["init"]).assert_success();

    // Create a file to commit
    repo.create_file("test.txt", "hello");

    // Create should succeed normally (no hook)
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    output.assert_success();

    // Branch should exist and be checked out
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

    // Initialize stax
    repo.run_stax(&["init"]).assert_success();

    // Install a hook that always fails
    install_failing_pre_commit_hook(&repo);

    // Create with just a name (no -m, no commit attempt)
    let output = repo.run_stax(&["create", "my-branch"]);
    output.assert_success();

    // Branch should exist (no commit was attempted)
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

    // Initialize stax
    repo.run_stax(&["init"]).assert_success();

    // Create a file
    repo.create_file("test.txt", "hello");

    // Install failing hook -> create fails
    install_failing_pre_commit_hook(&repo);
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    assert!(!output.status.success());

    // Remove hook -> retry should succeed with the same branch name (no -2 suffix)
    remove_pre_commit_hook(&repo);
    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    output.assert_success();

    let branch = repo.current_branch();
    assert!(
        branch.contains("my-feature"),
        "Should be on the feature branch: {}",
        branch
    );
    assert!(
        !branch.contains("my-feature-2"),
        "Should not have a -2 suffix: {}",
        branch
    );
}
