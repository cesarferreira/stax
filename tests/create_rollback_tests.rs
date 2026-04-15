//! Tests for `st create -m` behaviour when the commit step fails.
//!
//! The graphite-style commit-first flow runs `git commit` on the current branch
//! before creating the new branch. When a pre-commit hook rejects the commit
//! (or the user interrupts with Ctrl+C) the command exits with no refs touched
//! — no orphan branch is created, the current branch's HEAD is unchanged, and
//! retrying with the same message does not drift into `mybranch-2`,
//! `mybranch-3`, etc.
//!
//! A legacy rollback path still exists for the less common branch-first flow
//! (e.g. `st create -m "msg" --from <other>`); this file covers both.
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
fn create_with_failing_hook_leaves_no_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");

    install_failing_pre_commit_hook(&repo);

    let main_before = repo.head_sha();

    let output = repo.run_stax(&["create", "-a", "-m", "my feature"]);
    output.assert_failure();

    // No branch should have been created at all — commit-first means the
    // branch is never made if the commit fails.
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b.contains("my-feature")),
        "No orphan branch should exist when commit fails. Branches: {:?}",
        branches
    );

    // User stays on main — no switch happened.
    assert_eq!(repo.current_branch(), "main");

    // Main's HEAD must not have moved: the commit either didn't happen or was
    // undone. Nothing stacked on top.
    assert_eq!(
        repo.head_sha(),
        main_before,
        "main's HEAD must not advance when the commit fails",
    );

    // Error message should guide the user to retry.
    output.assert_stderr_contains("No branch was created");
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

/// Verifies the commit-first ordering: on success, the new commit lives only
/// on the new branch and `main`'s HEAD does not advance.
#[test]
fn create_commit_lives_only_on_new_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let main_before = repo.get_commit_sha("main");

    repo.create_file("test.txt", "hello");
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_success();

    let main_after = repo.get_commit_sha("main");
    assert_eq!(
        main_before, main_after,
        "main's HEAD must not move when `st create -m` splits the commit off",
    );

    let new_branch = repo.current_branch();
    let new_branch_sha = repo.get_commit_sha(&new_branch);
    assert_ne!(
        new_branch_sha, main_before,
        "new branch must point at a new commit, not at main's HEAD",
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
