//! Tests for `st create -m` behaviour when the commit step fails.
//!
//! The graphite-style commit-first flow runs `git commit` on the current branch
//! before creating the new branch. When a pre-commit hook rejects the commit
//! (or the user interrupts with Ctrl+C) the command exits with no refs touched
//! — no orphan branch is created, the current branch's HEAD is unchanged, and
//! retrying with the same message does not drift into `mybranch-2`,
//! `mybranch-3`, etc.
//!
//! The same safety property applies when `-m` commits from a non-current
//! parent (`--from <other>` / `--below`): stax commits before creating the
//! destination branch, then writes refs and metadata only after success.
//!
//! Most tests require Unix (pre-commit hooks use `#!/bin/sh` and chmod).

mod common;

#[cfg(unix)]
use common::stax_bin;
use common::{OutputAssertions, TestRepo};
#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::process::{Command, Stdio};
#[cfg(unix)]
use std::thread;
#[cfg(unix)]
use std::time::{Duration, Instant};

/// Install a pre-commit hook that always fails.
#[cfg(unix)]
fn install_failing_pre_commit_hook(repo: &TestRepo) {
    let hooks_dir = repo.path().join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    fs::write(&hook_path, "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn install_waiting_pre_commit_hook(repo: &TestRepo) {
    let hooks_dir = repo.path().join(".git").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    let hook_path = hooks_dir.join("pre-commit");
    fs::write(
        &hook_path,
        "#!/bin/sh\n\
         touch .git/hooks/pre-commit-started\n\
         while [ ! -f .git/hooks/pre-commit-release ]; do sleep 1; done\n",
    )
    .unwrap();
    fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Remove the pre-commit hook.
#[cfg(unix)]
fn remove_pre_commit_hook(repo: &TestRepo) {
    let hook_path = repo.path().join(".git").join("hooks").join("pre-commit");
    let _ = std::fs::remove_file(hook_path);
}

#[cfg(unix)]
fn wait_for_hook_start(repo: &TestRepo) {
    let marker = repo
        .path()
        .join(".git")
        .join("hooks")
        .join("pre-commit-started");
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if marker.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    panic!("pre-commit hook did not start");
}

#[cfg(unix)]
fn run_stax_until_hook_then_interrupt(repo: &TestRepo, args: &[&str]) -> std::process::Output {
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let child = Command::new(stax_bin())
        .args(args)
        .current_dir(repo.path())
        .env_remove("GITHUB_TOKEN")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("STAX_SHELL_INTEGRATION")
        .env_remove("GH_TOKEN")
        .env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stax");

    wait_for_hook_start(repo);

    let pid = child.id().to_string();
    let status = Command::new("kill")
        .args(["-INT", &pid])
        .status()
        .expect("send SIGINT to stax process");
    assert!(status.success(), "failed to interrupt stax process {pid}");

    fs::write(
        repo.path()
            .join(".git")
            .join("hooks")
            .join("pre-commit-release"),
        "",
    )
    .expect("release waiting pre-commit hook");

    child.wait_with_output().expect("wait for interrupted stax")
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

#[test]
#[cfg(unix)]
fn create_no_verify_skips_hook_in_commit_first_flow() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("test.txt", "hello");
    install_failing_pre_commit_hook(&repo);

    let output = repo.run_stax(&["create", "-a", "-m", "skip hook", "--no-verify"]);
    output.assert_success();

    assert!(
        repo.current_branch().contains("skip-hook"),
        "Should be on the created branch. Current: {}",
        repo.current_branch()
    );
    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "skip hook");
}

#[test]
#[cfg(unix)]
fn create_no_verify_works_via_bc_alias() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("alias.txt", "hello");
    install_failing_pre_commit_hook(&repo);

    repo.run_stax(&["bc", "-a", "-m", "alias skip hook", "-n"])
        .assert_success();

    assert!(
        repo.current_branch().contains("alias-skip-hook"),
        "Should be on the created branch. Current: {}",
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

/// `--insert` + `-m` exercises the commit-first flow AND the reparenting path.
/// Verifies children get reparented to the newly-created (and now committed)
/// branch, and the split-off commit still only lives on that branch.
#[test]
fn create_commit_first_with_insert_reparents_children_onto_new_commit() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    // main -> insert-a -> insert-b
    let branches = repo.create_stack(&["insert-a", "insert-b"]);

    // Back on insert-a, create a new branch WITH a commit, inserting above A.
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("insert_mid.txt", "new work");

    let insert_a_before = repo.get_commit_sha(&branches[0]);

    let output = repo.run_stax(&["create", "-a", "-m", "mid feature", "--insert"]);
    output.assert_success();

    // insert-a (the parent) must not have moved — the new commit lives only on
    // the inserted branch.
    assert_eq!(
        repo.get_commit_sha(&branches[0]),
        insert_a_before,
        "parent branch must not advance in commit-first + --insert",
    );

    // The previously-direct child of insert-a should now be reparented.
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Expected reparent message, got: {}",
        stdout
    );

    // insert-b's parent should now be the inserted branch.
    repo.run_stax(&["checkout", &branches[1]]).assert_success();
    let b_parent = repo.get_current_parent();
    assert!(
        b_parent
            .as_deref()
            .is_some_and(|p| p.contains("mid-feature")),
        "insert-b should be reparented onto the mid-feature branch, got: {:?}",
        b_parent,
    );
}

/// After a failing pre-commit hook, the user's working-tree changes must still
/// be present so they can fix the hook and retry with the same command — the
/// whole point of commit-first is that retrying is a clean operation.
#[test]
#[cfg(unix)]
fn create_failing_hook_preserves_working_tree_changes() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("notes.txt", "draft idea");

    install_failing_pre_commit_hook(&repo);

    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_failure();

    // The file the user wanted committed must still be on disk, unchanged.
    let contents = std::fs::read_to_string(repo.path().join("notes.txt"))
        .expect("notes.txt should still exist");
    assert_eq!(contents, "draft idea");

    // Remove hook and retry — must succeed with the original branch name (no
    // `-2` suffix) because nothing was left behind.
    remove_pre_commit_hook(&repo);
    repo.run_stax(&["create", "-a", "-m", "my feature"])
        .assert_success();

    let branch = repo.current_branch();
    assert!(
        branch.contains("my-feature") && !branch.contains("my-feature-2"),
        "Retry should use the original branch name, got: {}",
        branch,
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

/// `-m` is passed without a positional name so the branch name is derived
/// from the message — when both are given, stax uses the positional name and
/// discards the message, which would suppress the commit entirely.
#[test]
#[cfg(unix)]
fn create_from_other_branch_with_failing_hook_leaves_no_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Seed a second branch so we have somewhere to point --from at.
    let branches = repo.create_stack(&["sibling"]);

    // Back on main, install a failing hook and drop an uncommitted file that
    // `-a` will pick up after the branch switch.
    repo.run_stax(&["checkout", "main"]).assert_success();
    install_failing_pre_commit_hook(&repo);
    repo.create_file("wip.txt", "wip");

    let output = repo.run_stax(&[
        "create",
        "--from",
        &branches[0],
        "-a",
        "-m",
        "off sibling feature",
    ]);
    output.assert_failure();

    // The branch must never be created — no orphan, and retries do not drift
    // into a suffixed name.
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|b| b.contains("off-sibling-feature")),
        "new branch must be rolled back after hook failure. Branches: {:?}",
        repo.list_branches(),
    );
    output.assert_stderr_contains("No branch was created");

    repo.run_stax(&[
        "create",
        "--from",
        &branches[0],
        "-a",
        "-m",
        "off sibling feature",
    ])
    .assert_failure();
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|b| b.contains("off-sibling-feature-2")),
        "retry must not drift into a suffixed branch. Branches: {:?}",
        repo.list_branches(),
    );
}

#[test]
fn create_from_other_branch_with_message_commits_on_requested_parent() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let branches = repo.create_stack(&["sibling-parent"]);
    let parent = &branches[0];
    let parent_before = repo.get_commit_sha(parent);

    repo.run_stax(&["checkout", "main"]).assert_success();
    let main_before = repo.get_commit_sha("main");
    repo.create_file("from-parent.txt", "work for explicit parent\n");

    let output = repo.run_stax(&["create", "--from", parent, "-a", "-m", "off sibling parent"]);
    output.assert_success();
    output.assert_stdout_contains("Committed: off sibling parent");

    assert_eq!(
        repo.get_commit_sha("main"),
        main_before,
        "original branch should not advance when creating from another parent"
    );

    let commit_parent = repo.git(&["rev-parse", "HEAD^"]);
    assert!(
        commit_parent.status.success(),
        "{}",
        TestRepo::stderr(&commit_parent)
    );
    assert_eq!(TestRepo::stdout(&commit_parent).trim(), parent_before);

    let stax_parent = repo.get_current_parent();
    assert!(
        stax_parent.as_deref().is_some_and(|p| p == parent),
        "new branch metadata should point at requested parent, got: {:?}",
        stax_parent
    );
}

#[test]
#[cfg(unix)]
fn create_from_other_branch_interrupted_commit_leaves_no_branch_and_retries_cleanly() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let branches = repo.create_stack(&["interrupt-parent"]);
    let parent = &branches[0];

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("interrupt.txt", "work interrupted during commit\n");
    install_waiting_pre_commit_hook(&repo);

    let output = run_stax_until_hook_then_interrupt(
        &repo,
        &[
            "create",
            "--from",
            parent,
            "-a",
            "-m",
            "interrupted from parent",
        ],
    );
    assert!(
        !output.status.success(),
        "interrupted create should fail\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert_eq!(
        repo.current_branch(),
        "main",
        "interrupted create should restore the original branch"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|b| b.contains("interrupted-from-parent")),
        "interrupted create must not leave a destination branch. Branches: {:?}",
        repo.list_branches(),
    );

    remove_pre_commit_hook(&repo);
    repo.run_stax(&[
        "create",
        "--from",
        parent,
        "-a",
        "-m",
        "interrupted from parent",
    ])
    .assert_success();

    let branch = repo.current_branch();
    assert!(
        branch.contains("interrupted-from-parent") && !branch.contains("interrupted-from-parent-2"),
        "retry should use the original branch name, got: {}",
        branch
    );
}

#[test]
#[cfg(unix)]
fn create_below_interrupted_commit_leaves_no_branch_and_retries_cleanly() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["interrupt-below-parent", "interrupt-below-current"]);
    let parent = &branches[0];
    let current = &branches[1];
    let current_before = repo.get_commit_sha(current);

    repo.run_stax(&["checkout", current]).assert_success();
    repo.create_file(
        "interrupt-below.txt",
        "work interrupted during below commit\n",
    );
    install_waiting_pre_commit_hook(&repo);

    let output = run_stax_until_hook_then_interrupt(
        &repo,
        &["create", "--below", "-a", "-m", "interrupted below parent"],
    );
    assert!(
        !output.status.success(),
        "interrupted create --below should fail\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert_eq!(
        repo.current_branch(),
        *current,
        "interrupted create --below should restore the original branch"
    );
    assert_eq!(
        repo.get_commit_sha(current),
        current_before,
        "interrupted create --below should not advance the original branch"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|b| b.contains("interrupted-below-parent")),
        "interrupted create --below must not leave a destination branch. Branches: {:?}",
        repo.list_branches(),
    );

    remove_pre_commit_hook(&repo);
    repo.run_stax(&["create", "--below", "-a", "-m", "interrupted below parent"])
        .assert_success();

    let branch = repo.current_branch();
    assert!(
        branch.contains("interrupted-below-parent")
            && !branch.contains("interrupted-below-parent-2"),
        "retry should use the original branch name, got: {}",
        branch
    );

    repo.run_stax(&["checkout", current]).assert_success();
    let stax_parent = repo.get_current_parent();
    assert!(
        stax_parent
            .as_deref()
            .is_some_and(|p| p.contains("interrupted-below-parent")),
        "original branch should be reparented onto retry branch, got: {:?}",
        stax_parent
    );

    repo.run_stax(&["checkout", parent]).assert_success();
}

#[test]
#[cfg(unix)]
fn branch_create_no_verify_skips_hook_in_branch_first_flow() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let branches = repo.create_stack(&["sibling"]);

    repo.run_stax(&["checkout", "main"]).assert_success();
    install_failing_pre_commit_hook(&repo);
    repo.create_file("wip.txt", "wip");

    let output = repo.run_stax(&[
        "branch",
        "create",
        "--from",
        &branches[0],
        "-a",
        "-m",
        "off sibling no verify",
        "-n",
    ]);
    output.assert_success();

    assert!(
        repo.current_branch().contains("off-sibling-no-verify"),
        "Should be on the created branch. Current: {}",
        repo.current_branch()
    );
    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "off sibling no verify");
}
