use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::Command;
#[cfg(unix)]
use tempfile::TempDir;

fn setup_promotable_worktree() -> (TestRepo, String, PathBuf) {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "promote-feature"])
        .assert_success();
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "promoted\n");
    repo.commit("Add promoted feature");
    repo.run_stax(&["checkout", "main"]).assert_success();

    let linked = PathBuf::from(repo.clean_home()).join("promote-feature-worktree");
    repo.git(&[
        "worktree",
        "add",
        linked.to_str().expect("UTF-8 worktree path"),
        &branch,
    ])
    .assert_success();

    (repo, branch, linked)
}

fn current_branch(repo: &TestRepo, path: &Path) -> String {
    let output = repo.git(&[
        "-C",
        path.to_str().expect("UTF-8 repository path"),
        "branch",
        "--show-current",
    ]);
    output.assert_success();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn worktree_list(repo: &TestRepo) -> String {
    let output = repo.git(&["worktree", "list", "--porcelain"]);
    output.assert_success();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[cfg(unix)]
fn real_git_path() -> String {
    let output = Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("resolve git path");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[cfg(unix)]
fn failing_git_path() -> (TempDir, String) {
    let bin_dir = TempDir::new().expect("create git shim directory");
    let shim = bin_dir.path().join("git");
    fs::write(
        &shim,
        r#"#!/bin/sh
set -eu
cwd=$(pwd -P)
mode=${STAX_PROMOTE_FAIL_MODE:-}
if [ "$mode" = "main-switch" ] && [ "$cwd" = "$STAX_PROMOTE_MAIN" ] && \
   [ "${1:-}" = "switch" ] && [ "${2:-}" = "$STAX_PROMOTE_BRANCH" ]; then
  echo "synthetic main switch failure" >&2
  exit 41
fi
if [ "$mode" = "remove" ] && [ "$cwd" = "$STAX_PROMOTE_MAIN" ] && \
   [ "${1:-}" = "worktree" ] && [ "${2:-}" = "remove" ]; then
  echo "synthetic worktree remove failure" >&2
  exit 42
fi
exec "$REAL_GIT" "$@"
"#,
    )
    .expect("write git shim");
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(&shim)
        .expect("git shim metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&shim, permissions).expect("make git shim executable");

    let path = format!(
        "{}:{}",
        bin_dir.path().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    (bin_dir, path)
}

#[test]
fn wt_promote_moves_current_branch_to_main_worktree() {
    let (repo, branch, linked) = setup_promotable_worktree();

    let output = repo.run_stax_in(&linked, &["wt", "promote"]);

    output.assert_success();
    assert_eq!(current_branch(&repo, &repo.path()), branch);
    assert!(repo.path().join("feature.txt").exists());
    assert!(
        !linked.exists(),
        "promoted linked worktree should be retired"
    );

    let status = repo.run_stax(&["status", "--json"]);
    status.assert_success();
    let status: Value = serde_json::from_slice(&status.stdout).expect("valid status JSON");
    let promoted = status["branches"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|candidate| candidate["name"].as_str() == Some(&branch))
        })
        .expect("promoted branch should remain tracked");
    assert_eq!(promoted["parent"].as_str(), Some("main"));
}

#[test]
fn wt_promote_works_from_nested_directory() {
    let (repo, branch, linked) = setup_promotable_worktree();
    let nested = linked.join("nested").join("directory");
    fs::create_dir_all(&nested).expect("create nested worktree directory");

    let output = repo.run_stax_in(&nested, &["wt", "promote"]);

    output.assert_success();
    assert_eq!(current_branch(&repo, &repo.path()), branch);
    assert!(!linked.exists());
}

#[test]
fn wt_promote_refuses_dirty_linked_worktree() {
    let (repo, branch, linked) = setup_promotable_worktree();
    fs::write(linked.join("dirty.txt"), "dirty\n").expect("write dirty source file");

    let output = repo.run_stax_in(&linked, &["wt", "promote"]);

    output
        .assert_failure()
        .assert_stderr_contains("current linked worktree has uncommitted changes");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
    assert_eq!(current_branch(&repo, &linked), branch);
    assert!(linked.exists());
}

#[test]
fn wt_promote_refuses_dirty_main_worktree() {
    let (repo, branch, linked) = setup_promotable_worktree();
    repo.create_file("dirty-main.txt", "dirty\n");

    let output = repo.run_stax_in(&linked, &["wt", "promote"]);

    output
        .assert_failure()
        .assert_stderr_contains("main worktree has uncommitted changes");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
    assert_eq!(current_branch(&repo, &linked), branch);
    assert!(linked.exists());
}

#[test]
fn wt_promote_refuses_detached_linked_worktree() {
    let (repo, _branch, linked) = setup_promotable_worktree();
    repo.git(&[
        "-C",
        linked.to_str().expect("UTF-8 worktree path"),
        "switch",
        "--detach",
    ])
    .assert_success();

    let output = repo.run_stax_in(&linked, &["wt", "promote"]);

    output
        .assert_failure()
        .assert_stderr_contains("Cannot promote a detached worktree");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
    assert!(linked.exists());
}

#[test]
fn wt_promote_refuses_main_worktree() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["wt", "promote"]);

    output
        .assert_failure()
        .assert_stderr_contains("Cannot promote the main worktree");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
}

#[test]
fn wt_promote_emits_shell_payload_only_after_success() {
    let (repo, branch, linked) = setup_promotable_worktree();

    let output = repo.run_stax_in(&linked, &["wt", "promote", "--shell-output"]);

    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    let canonical_main = fs::canonicalize(repo.path()).expect("canonical main path");
    assert!(
        stdout.contains(&format!("STAX_SHELL_PATH={}", canonical_main.display())),
        "expected shell path payload, got:\n{stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "STAX_SHELL_MESSAGE=Promoted '{branch}' to the main worktree"
        )),
        "expected shell message payload, got:\n{stdout}"
    );
}

#[test]
fn wt_promote_parks_eligible_managed_worktree() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    repo.run_stax_with_env(&["wt", "c", "empty-promote"], &[("HOME", &home)])
        .assert_success();
    let path_output = repo.run_stax_with_env(&["wt", "path", "empty-promote"], &[("HOME", &home)]);
    path_output.assert_success();
    let linked = PathBuf::from(TestRepo::stdout(&path_output).trim());
    let branch = current_branch(&repo, &linked);

    let output = repo.run_stax_in_with_env(&linked, &["wt", "promote"], &[("HOME", home.as_str())]);

    output.assert_success();
    assert_eq!(current_branch(&repo, &repo.path()), branch);
    assert!(
        linked.exists(),
        "eligible worktree should remain as a warm slot"
    );
    let listed = worktree_list(&repo);
    assert!(listed.contains(linked.to_string_lossy().as_ref()));
    let parked = listed
        .split("\n\n")
        .find(|entry| entry.contains(linked.to_string_lossy().as_ref()))
        .expect("parked worktree entry");
    assert!(!parked.contains(&format!("branch refs/heads/{branch}")));
    assert!(
        parked.contains("detached") || parked.contains("branch refs/heads/main"),
        "expected slot parked off the promoted branch:\n{parked}"
    );
}

#[cfg(unix)]
#[test]
fn wt_promote_rolls_back_when_main_switch_fails() {
    let (repo, branch, linked) = setup_promotable_worktree();
    let (_shim_dir, path) = failing_git_path();
    let main = fs::canonicalize(repo.path())
        .expect("canonical main path")
        .to_string_lossy()
        .into_owned();
    let real_git = real_git_path();

    let output = repo.run_stax_in_with_env(
        &linked,
        &["wt", "promote"],
        &[
            ("PATH", path.as_str()),
            ("REAL_GIT", real_git.as_str()),
            ("STAX_PROMOTE_FAIL_MODE", "main-switch"),
            ("STAX_PROMOTE_MAIN", main.as_str()),
            ("STAX_PROMOTE_BRANCH", branch.as_str()),
        ],
    );

    output
        .assert_failure()
        .assert_stderr_contains("synthetic main switch failure");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
    assert_eq!(current_branch(&repo, &linked), branch);
    assert!(linked.exists());
}

#[cfg(unix)]
#[test]
fn wt_promote_rolls_back_when_retirement_fails() {
    let (repo, branch, linked) = setup_promotable_worktree();
    let (_shim_dir, path) = failing_git_path();
    let main = fs::canonicalize(repo.path())
        .expect("canonical main path")
        .to_string_lossy()
        .into_owned();
    let real_git = real_git_path();

    let output = repo.run_stax_in_with_env(
        &linked,
        &["wt", "promote"],
        &[
            ("PATH", path.as_str()),
            ("REAL_GIT", real_git.as_str()),
            ("STAX_PROMOTE_FAIL_MODE", "remove"),
            ("STAX_PROMOTE_MAIN", main.as_str()),
            ("STAX_PROMOTE_BRANCH", branch.as_str()),
        ],
    );

    output
        .assert_failure()
        .assert_stderr_contains("synthetic worktree remove failure");
    assert_eq!(current_branch(&repo, &repo.path()), "main");
    assert_eq!(current_branch(&repo, &linked), branch);
    assert!(linked.exists());
}
