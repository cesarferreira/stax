//! Integration tests for `stax sync --get` / `stax rs --get`.

use crate::common::{OutputAssertions, TestRepo};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn run_git_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

/// Check out `branch` in a fresh clone of `repo`'s remote, add a new commit, and push it —
/// simulating another machine advancing the branch independently.
fn update_remote_branch_in_clone(
    repo: &TestRepo,
    branch: &str,
    file: &str,
    content: &str,
) -> String {
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = TempDir::new().expect("temp clone");

    run_git_in(
        clone_dir.path(),
        &["clone", remote_path.to_str().unwrap(), "."],
    );
    run_git_in(
        clone_dir.path(),
        &["checkout", "-B", branch, &format!("origin/{}", branch)],
    );
    run_git_in(
        clone_dir.path(),
        &["config", "user.email", "other@test.com"],
    );
    run_git_in(clone_dir.path(), &["config", "user.name", "Other User"]);
    std::fs::write(clone_dir.path().join(file), content).expect("write remote update");
    run_git_in(clone_dir.path(), &["add", "-A"]);
    run_git_in(
        clone_dir.path(),
        &["commit", "-m", "Update from other machine"],
    );
    let sha = run_git_in(clone_dir.path(), &["rev-parse", "HEAD"]);
    run_git_in(clone_dir.path(), &["push", "origin", branch]);

    String::from_utf8_lossy(&sha.stdout).trim().to_string()
}

#[test]
fn sync_get_reconciles_diverged_branch_without_force() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-get"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    // Diverge: another machine advances the remote branch...
    let remote_sha =
        update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    // ...while this machine also makes its own unrelated local commit.
    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");
    let local_sha_before = repo.head_sha();
    assert_ne!(local_sha_before, remote_sha, "test setup must diverge");

    let out = repo.run_stax(&["sync", "--get"]);
    assert!(
        out.status.success(),
        "sync --get failed: {}",
        TestRepo::stderr(&out)
    );

    // The remote commit must now be part of the local branch's history (fast-forward or
    // rebase — either way the reconciliation must have happened), without --force.
    let contains_remote = repo
        .git(&["merge-base", "--is-ancestor", &remote_sha, &branch])
        .status
        .success();
    assert!(
        contains_remote,
        "local branch must contain the remote commit after `sync --get`"
    );

    // The local-only commit's content must still be present (proves a rebase, not a hard reset).
    assert!(
        repo.path().join("local-only.txt").exists(),
        "local-only work must survive reconciliation without --force"
    );
}

#[test]
fn sync_get_json_reports_reconciled_branches() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-get-json"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --json failed: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"));

    let reconciled = parsed["reconciled_branches"]
        .as_array()
        .unwrap_or_else(|| panic!("reconciled_branches missing or not an array; got:\n{parsed}"));
    let entry = reconciled
        .iter()
        .find(|entry| entry["branch"] == branch.as_str())
        .unwrap_or_else(|| panic!("{branch} missing from reconciled_branches; got:\n{parsed}"));
    assert_eq!(entry["action"], "rebased");
}

#[test]
fn sync_get_json_reports_branch_without_remote() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-get-no-remote"]);
    let branch = repo.current_branch();

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --json failed: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"));
    assert!(
        parsed["skipped_branches"]
            .as_array()
            .is_some_and(|skipped| {
                skipped.iter().any(|entry| {
                    entry["name"] == branch.as_str() && entry["reason"] == "no remote branch"
                })
            }),
        "branch without a remote missing from skipped_branches: {parsed}"
    );
}

#[test]
fn sync_get_dry_run_previews_without_mutating() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-get-dry-run"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");
    let local_sha_before = repo.head_sha();

    let out = repo.run_stax(&["sync", "--get", "--dry-run"]);
    assert!(
        out.status.success(),
        "sync --get --dry-run failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("would reconcile"),
        "expected dry-run preview to mention 'would reconcile'; got: {stdout}"
    );

    let local_sha_after = repo.head_sha();
    assert_eq!(
        local_sha_before, local_sha_after,
        "sync --get --dry-run must not mutate the local branch"
    );
}

#[test]
fn sync_get_skips_frozen_branch() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["frozen-get-parent", "frozen-get-child"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();

    let child = &branches[1];
    repo.run_stax(&["freeze", child]).assert_success();
    let child_sha_before = repo.get_commit_sha(child);

    update_remote_branch_in_clone(&repo, child, "other-machine.txt", "other machine content");

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --json failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"));
    assert!(
        parsed["skipped_branches"]
            .as_array()
            .is_some_and(|skipped| {
                skipped
                    .iter()
                    .any(|entry| entry["name"] == child.as_str() && entry["reason"] == "frozen")
            }),
        "frozen branch {child} missing from skipped_branches: {parsed}"
    );

    assert_eq!(
        repo.get_commit_sha(child),
        child_sha_before,
        "frozen branch must not be reconciled by --get"
    );
}

#[test]
fn sync_get_skips_branch_checked_out_in_other_worktree() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["wt-get-parent", "wt-get-child"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();

    let parent = &branches[0];
    let child = &branches[1];
    let parent_sha_before = repo.get_commit_sha(parent);

    let linked_worktree = TempDir::new().expect("linked worktree");
    repo.git(&[
        "worktree",
        "add",
        linked_worktree.path().to_str().unwrap(),
        parent,
    ])
    .assert_success();

    update_remote_branch_in_clone(&repo, parent, "other-machine.txt", "other machine content");

    repo.run_stax(&["checkout", child]).assert_success();
    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --json failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"));
    assert!(
        parsed["skipped_branches"]
            .as_array()
            .is_some_and(|skipped| {
                skipped.iter().any(|entry| {
                    entry["name"] == parent.as_str()
                        && entry["reason"] == "checked out in another worktree"
                })
            }),
        "worktree branch {parent} missing from skipped_branches: {parsed}"
    );

    assert_eq!(
        repo.get_commit_sha(parent),
        parent_sha_before,
        "branch checked out in another worktree must not be reconciled by --get"
    );
}

#[test]
fn sync_get_force_resets_diverged_branch() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-get-force"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    let remote_sha =
        update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");

    let out = repo.run_stax(&["sync", "--get", "--force"]);
    assert!(
        out.status.success(),
        "sync --get --force failed: {}",
        TestRepo::stderr(&out)
    );

    assert_eq!(
        repo.get_commit_sha(&branch),
        remote_sha,
        "sync --get --force must hard-reset the local branch to the remote tip"
    );
    assert!(
        !repo.path().join("local-only.txt").exists(),
        "local-only commit must be discarded by a force reset"
    );
}

#[test]
fn sync_without_get_leaves_branch_untouched() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-no-get"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");
    let local_sha_before = repo.head_sha();

    let out = repo.run_stax(&["sync"]);
    assert!(
        out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&out)
    );

    let local_sha_after = repo.head_sha();
    assert_eq!(
        local_sha_before, local_sha_after,
        "plain sync (without --get) must not reconcile the branch against its remote ref"
    );
}

#[test]
fn refresh_get_reconciles_diverged_branch() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-refresh-get"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    let remote_sha =
        update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");
    let local_sha_before = repo.head_sha();
    assert_ne!(local_sha_before, remote_sha, "test setup must diverge");

    let out = repo.run_stax(&["refresh", "--get", "--no-submit"]);
    assert!(
        out.status.success(),
        "refresh --get failed: {}",
        TestRepo::stderr(&out)
    );

    let local_sha_after = repo.head_sha();
    assert_ne!(
        local_sha_before, local_sha_after,
        "refresh --get must reconcile the branch tip"
    );

    let contains_remote = repo
        .git(&["merge-base", "--is-ancestor", &remote_sha, &branch])
        .status
        .success();
    assert!(
        contains_remote,
        "local branch must contain the remote commit after `refresh --get`"
    );
    assert!(
        repo.path().join("other-machine.txt").exists(),
        "remote content must be present after reconciliation"
    );
    assert!(
        repo.path().join("local-only.txt").exists(),
        "local-only work must survive reconciliation without --force"
    );
}

#[test]
fn refresh_get_force_resets_diverged_branch() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-refresh-get-force"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    let remote_sha =
        update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");

    let out = repo.run_stax(&["refresh", "--get", "--force", "--no-submit", "--yes"]);
    assert!(
        out.status.success(),
        "refresh --get --force failed: {}",
        TestRepo::stderr(&out)
    );

    assert_eq!(
        repo.get_commit_sha(&branch),
        remote_sha,
        "refresh --get --force must hard-reset the local branch to the remote tip"
    );
    assert!(
        !repo.path().join("local-only.txt").exists(),
        "local-only commit must be discarded by a force reset"
    );
}

#[test]
fn refresh_without_get_leaves_branch_untouched() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.run_stax(&["bc", "feature-refresh-no-get"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "local content");
    repo.commit("Local feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    update_remote_branch_in_clone(&repo, &branch, "other-machine.txt", "other machine content");

    repo.create_file("local-only.txt", "still local");
    repo.commit("Another local commit");
    let local_sha_before = repo.head_sha();

    let out = repo.run_stax(&["refresh", "--no-submit", "--force", "--yes"]);
    assert!(
        out.status.success(),
        "refresh failed: {}",
        TestRepo::stderr(&out)
    );

    let local_sha_after = repo.head_sha();
    assert_eq!(
        local_sha_before, local_sha_after,
        "refresh without --get must not reconcile the branch against its remote ref"
    );
}

fn push_all(repo: &TestRepo, branches: &[String]) {
    for branch in branches {
        repo.git(&["push", "-u", "origin", branch]).assert_success();
    }
}

/// Two independent pushed stacks off trunk; leaves `main` checked out.
fn two_pushed_stacks(repo: &TestRepo, a: &[&str], b: &[&str]) -> (Vec<String>, Vec<String>) {
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    let stack_a = repo.create_stack(a);
    repo.git(&["checkout", "main"]).assert_success();
    let stack_b = repo.create_stack(b);
    repo.git(&["checkout", "main"]).assert_success();
    push_all(repo, &stack_a);
    push_all(repo, &stack_b);
    (stack_a, stack_b)
}

fn parse_json(out: &std::process::Output) -> serde_json::Value {
    let stdout = TestRepo::stdout(out);
    serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"))
}

fn reconciled_names(parsed: &serde_json::Value) -> Vec<String> {
    parsed["reconciled_branches"]
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| entry["branch"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn assert_skipped(parsed: &serde_json::Value, branch: &str, reason: &str) {
    assert!(
        parsed["skipped_branches"]
            .as_array()
            .is_some_and(|skipped| skipped
                .iter()
                .any(|entry| entry["name"] == branch && entry["reason"] == reason)),
        "{branch} missing from skipped_branches with reason {reason:?}: {parsed}"
    );
}

#[test]
fn sync_get_all_stacks_reconciles_every_stack() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["gas-a"], &["gas-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let remote_a = update_remote_branch_in_clone(&repo, &stack_a[0], "a.txt", "a remote");
    let remote_b = update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");

    let out = repo.run_stax(&["sync", "--get", "--all-stacks", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --all-stacks failed: {}",
        TestRepo::stderr(&out)
    );
    let parsed = parse_json(&out);
    for branch in [&stack_a[0], &stack_b[0]] {
        let entry = parsed["reconciled_branches"]
            .as_array()
            .and_then(|entries| entries.iter().find(|e| e["branch"] == branch.as_str()))
            .unwrap_or_else(|| panic!("{branch} missing from reconciled_branches: {parsed}"));
        assert_eq!(entry["action"], "fast_forwarded");
    }
    assert_eq!(repo.get_commit_sha(&stack_a[0]), remote_a);
    assert_eq!(repo.get_commit_sha(&stack_b[0]), remote_b);
}

#[test]
fn rs_get_all_stacks_alias_reconciles_other_stack() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["rsas-a"], &["rsas-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let remote_b = update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");

    let out = repo.run_stax(&["rs", "--get", "--all-stacks"]);
    assert!(
        out.status.success(),
        "rs --get --all-stacks failed: {}",
        TestRepo::stderr(&out)
    );
    assert_eq!(repo.get_commit_sha(&stack_b[0]), remote_b);
}

#[test]
fn sync_get_on_trunk_reconciles_all_tracked_branches() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["gtrunk-a"], &["gtrunk-b"]);
    assert_eq!(repo.current_branch(), "main");

    let remote_a = update_remote_branch_in_clone(&repo, &stack_a[0], "a.txt", "a remote");
    let remote_b = update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get on trunk failed: {}",
        TestRepo::stderr(&out)
    );
    let names = reconciled_names(&parse_json(&out));
    assert!(names.contains(&stack_a[0]), "got {names:?}");
    assert!(names.contains(&stack_b[0]), "got {names:?}");
    assert_eq!(repo.get_commit_sha(&stack_a[0]), remote_a);
    assert_eq!(repo.get_commit_sha(&stack_b[0]), remote_b);
}

#[test]
fn sync_get_all_stacks_reconciles_parents_before_children() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["gord-parent", "gord-child"], &["gord-b"]);
    repo.git(&["checkout", &stack_b[0]]).assert_success();

    update_remote_branch_in_clone(&repo, &stack_a[0], "parent.txt", "parent remote");
    update_remote_branch_in_clone(&repo, &stack_a[1], "child.txt", "child remote");

    let out = repo.run_stax(&["sync", "--get", "--all-stacks", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --all-stacks failed: {}",
        TestRepo::stderr(&out)
    );
    let names = reconciled_names(&parse_json(&out));
    let parent_idx = names.iter().position(|n| n == &stack_a[0]);
    let child_idx = names.iter().position(|n| n == &stack_a[1]);
    assert!(
        parent_idx.is_some() && child_idx.is_some() && parent_idx < child_idx,
        "parent must be reconciled before child; got {names:?}"
    );
}

#[test]
fn sync_get_all_stacks_dry_run_previews_every_stack() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["gdry-a"], &["gdry-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();
    repo.create_file("local.txt", "local");
    repo.commit("Local commit on A");

    update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");
    let head_before = repo.head_sha();
    let a_before = repo.get_commit_sha(&stack_a[0]);
    let b_before = repo.get_commit_sha(&stack_b[0]);

    let out = repo.run_stax(&["sync", "--get", "--all-stacks", "--dry-run"]);
    assert!(
        out.status.success(),
        "dry-run failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Get (all tracked branches vs remote)"),
        "missing all-branches heading: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains(&stack_b[0]) && l.contains("would reconcile")),
        "preview must list {}: {stdout}",
        stack_b[0]
    );
    assert_eq!(repo.head_sha(), head_before);
    assert_eq!(repo.get_commit_sha(&stack_a[0]), a_before);
    assert_eq!(repo.get_commit_sha(&stack_b[0]), b_before);
}

#[test]
fn sync_get_on_trunk_dry_run_previews_all_branches() {
    let repo = TestRepo::new_with_remote();
    let (_stack_a, stack_b) = two_pushed_stacks(&repo, &["gtdry-a"], &["gtdry-b"]);
    update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");

    let out = repo.run_stax(&["sync", "--get", "--dry-run"]);
    assert!(
        out.status.success(),
        "dry-run failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("No stack branches in scope."),
        "unexpected empty scope: {stdout}"
    );
    assert!(
        stdout
            .lines()
            .any(|l| l.contains(&stack_b[0]) && l.contains("would reconcile")),
        "preview must list {}: {stdout}",
        stack_b[0]
    );
}

#[test]
fn sync_all_stacks_requires_get() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    let stack = repo.create_stack(&["greq-a"]);
    let head_before = repo.head_sha();

    let out = repo.run_stax(&["sync", "--all-stacks"]);
    out.assert_failure();
    assert!(
        TestRepo::stderr(&out).contains("--get"),
        "stderr must name --get: {}",
        TestRepo::stderr(&out)
    );
    assert_eq!(repo.head_sha(), head_before);
    assert_eq!(repo.current_branch(), stack[0]);
}

#[test]
fn sync_get_off_trunk_without_all_stacks_leaves_other_stacks_untouched() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["goff-a"], &["goff-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let remote_a = update_remote_branch_in_clone(&repo, &stack_a[0], "a.txt", "a remote");
    update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");
    let b_before = repo.get_commit_sha(&stack_b[0]);

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get failed: {}",
        TestRepo::stderr(&out)
    );
    let names = reconciled_names(&parse_json(&out));
    assert!(names.contains(&stack_a[0]), "got {names:?}");
    assert!(!names.contains(&stack_b[0]), "got {names:?}");
    assert_eq!(repo.get_commit_sha(&stack_a[0]), remote_a);
    assert_eq!(repo.get_commit_sha(&stack_b[0]), b_before);
}

#[test]
fn sync_get_all_stacks_skips_frozen_and_worktree_branches() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = two_pushed_stacks(&repo, &["gskip-a"], &["gskip-b"]);
    let stack_c = repo.create_stack(&["gskip-c"]);
    repo.git(&["checkout", "main"]).assert_success();
    push_all(&repo, &stack_c);

    repo.run_stax(&["freeze", &stack_b[0]]).assert_success();
    let linked = TempDir::new().expect("linked worktree");
    repo.git(&[
        "worktree",
        "add",
        linked.path().to_str().unwrap(),
        &stack_c[0],
    ])
    .assert_success();

    update_remote_branch_in_clone(&repo, &stack_b[0], "b.txt", "b remote");
    update_remote_branch_in_clone(&repo, &stack_c[0], "c.txt", "c remote");
    let b_before = repo.get_commit_sha(&stack_b[0]);
    let c_before = repo.get_commit_sha(&stack_c[0]);

    repo.git(&["checkout", &stack_a[0]]).assert_success();
    let out = repo.run_stax(&["sync", "--get", "--all-stacks", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --all-stacks failed: {}",
        TestRepo::stderr(&out)
    );
    let parsed = parse_json(&out);
    assert_skipped(&parsed, &stack_b[0], "frozen");
    assert_skipped(&parsed, &stack_c[0], "checked out in another worktree");
    assert_eq!(repo.get_commit_sha(&stack_b[0]), b_before);
    assert_eq!(repo.get_commit_sha(&stack_c[0]), c_before);
}

#[test]
fn sync_get_all_stacks_skips_unpushed_branch_and_succeeds() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, _stack_b) = two_pushed_stacks(&repo, &["gnp-a"], &["gnp-b"]);
    let stack_c = repo.create_stack(&["gnp-c"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let out = repo.run_stax(&["sync", "--get", "--all-stacks", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --all-stacks failed: {}",
        TestRepo::stderr(&out)
    );
    assert_skipped(&parse_json(&out), &stack_c[0], "no remote branch");
}

#[test]
fn sync_get_on_trunk_with_no_tracked_branches_is_noop() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();

    let out = repo.run_stax(&["sync", "--get", "--json"]);
    assert!(
        out.status.success(),
        "sync --get failed: {}",
        TestRepo::stderr(&out)
    );
    assert!(reconciled_names(&parse_json(&out)).is_empty());
}

fn merged_branch_on_trunk_sync_get(deny_deletes: bool) -> (TestRepo, String, serde_json::Value) {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    let stack = repo.create_stack(&["gmerged-a"]);
    repo.git(&["checkout", "main"]).assert_success();
    push_all(&repo, &stack);
    repo.merge_branch_on_remote(&stack[0]);
    if deny_deletes {
        let remote = repo.remote_path().expect("remote path");
        run_git_in(&remote, &["config", "receive.denyDeletes", "true"]);
    }

    let out = repo.run_stax(&["sync", "--get", "--force", "--json"]);
    assert!(
        out.status.success(),
        "sync --get --force failed: {}",
        TestRepo::stderr(&out)
    );
    let parsed = parse_json(&out);
    (repo, stack[0].clone(), parsed)
}

fn local_branch_listed(repo: &TestRepo, branch: &str) -> bool {
    !TestRepo::stdout(&repo.git(&["branch", "--list", branch]))
        .trim()
        .is_empty()
}

#[test]
fn sync_get_on_trunk_does_not_recreate_merged_branch_when_remote_ref_survives() {
    let (repo, branch, parsed) = merged_branch_on_trunk_sync_get(true);

    assert!(
        !local_branch_listed(&repo, &branch),
        "merged branch was recreated locally by --get"
    );
    assert!(!reconciled_names(&parsed).contains(&branch));
}

#[test]
fn sync_get_on_trunk_does_not_report_deleted_merged_branch_as_skipped() {
    let (repo, branch, parsed) = merged_branch_on_trunk_sync_get(false);

    assert!(!local_branch_listed(&repo, &branch));
    assert!(!reconciled_names(&parsed).contains(&branch));
    let skipped_mentions = parsed["skipped_branches"]
        .as_array()
        .is_some_and(|skipped| skipped.iter().any(|entry| entry["name"] == branch.as_str()));
    assert!(
        !skipped_mentions,
        "deleted branch reported as skipped: {parsed}"
    );
}

#[test]
fn sync_get_on_trunk_dry_run_omits_merged_branch_it_would_delete() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    let stack = repo.create_stack(&["gmerged-dry"]);
    repo.git(&["checkout", "main"]).assert_success();
    push_all(&repo, &stack);
    repo.merge_branch_on_remote(&stack[0]);
    repo.git(&["fetch", "origin"]).assert_success();

    let out = repo.run_stax(&["sync", "--get", "--force", "--dry-run"]);
    assert!(
        out.status.success(),
        "sync --get --dry-run failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    let get_section = stdout.split("Get (").nth(1).unwrap_or_default().to_string();
    assert!(
        !get_section.contains(&stack[0]),
        "dry-run get preview lists a branch the sync would delete:\n{stdout}"
    );
}
