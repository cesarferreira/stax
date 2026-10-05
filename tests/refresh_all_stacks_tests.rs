//! Integration tests for `st refresh --all-stacks`.

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

/// Advance `branch` from another clone, simulating a second machine.
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
        &["checkout", "-B", branch, &format!("origin/{branch}")],
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

/// Build two independent stacks off trunk: `feat-a -> feat-a1` and `feat-b`.
/// Returns the actual (possibly-prefixed) branch names for each stack.
fn build_two_stacks(repo: &TestRepo) -> (Vec<String>, Vec<String>) {
    let stack_a = repo.create_stack(&["feat-a", "feat-a1"]);
    repo.git(&["checkout", "main"]).assert_success();
    let stack_b = repo.create_stack(&["feat-b"]);
    (stack_a, stack_b)
}

#[test]
fn all_stacks_plan_lists_every_root_and_branch() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);

    let output = repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"]);
    let stdout = TestRepo::stdout(&output);

    assert!(
        stdout.contains("Updating all stacks"),
        "expected header, got: {stdout}"
    );
    assert!(
        stdout.contains(&stack_a[0]),
        "expected root of stack a, got: {stdout}"
    );
    assert!(
        stdout.contains(&stack_a[1]),
        "expected descendant of stack a, got: {stdout}"
    );
    assert!(
        stdout.contains(&stack_b[0]),
        "expected root of stack b, got: {stdout}"
    );
}

#[test]
fn all_stacks_restacks_every_stack() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);

    repo.simulate_remote_commit("trunk-update.txt", "updated", "Update trunk");

    repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"])
        .assert_success();

    for branch in stack_a.iter().chain(stack_b.iter()) {
        repo.git(&["merge-base", "--is-ancestor", "main", branch])
            .assert_success();
    }
}

#[test]
fn all_stacks_get_reconciles_only_the_current_stack() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);
    let current_stack = repo.create_stack(&["get-scope-current"]);
    repo.git(&["checkout", "main"]).assert_success();
    let other_stack = repo.create_stack(&["get-scope-other"]);

    let current_branch = &current_stack[0];
    let other_branch = &other_stack[0];
    for branch in [current_branch, other_branch] {
        repo.git(&["push", "-u", "origin", branch]).assert_success();
    }
    update_remote_branch_in_clone(
        &repo,
        current_branch,
        "current-stack-remote.txt",
        "current stack remote update",
    );
    let other_remote_sha = update_remote_branch_in_clone(
        &repo,
        other_branch,
        "other-stack-remote.txt",
        "other stack remote update",
    );

    repo.simulate_remote_commit("trunk-update.txt", "updated", "Update trunk");
    repo.git(&["checkout", current_branch]).assert_success();

    let output = repo.run_stax(&[
        "refresh",
        "--all-stacks",
        "--get",
        "--no-submit",
        "--force",
        "--yes",
    ]);
    assert!(
        output.status.success(),
        "refresh --all-stacks --get failed:\nstdout:\n{}\nstderr:\n{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        TestRepo::stdout(&output).contains(&format!("Reset {current_branch} to remote tip.")),
        "--get must reconcile the current stack; stdout:\n{}",
        TestRepo::stdout(&output)
    );
    assert!(
        !repo
            .git(&[
                "merge-base",
                "--is-ancestor",
                &other_remote_sha,
                other_branch,
            ])
            .status
            .success(),
        "the other stack's remote update must remain unreconciled"
    );
    for branch in current_stack.iter().chain(other_stack.iter()) {
        repo.git(&["merge-base", "--is-ancestor", "main", branch])
            .assert_success();
    }
}

#[test]
fn plain_refresh_leaves_other_stacks_untouched() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);

    repo.simulate_remote_commit("trunk-update.txt", "updated", "Update trunk");

    repo.git(&["checkout", &stack_a[1]]).assert_success();
    repo.run_stax(&["refresh", "--no-submit", "--force"])
        .assert_success();

    for branch in &stack_a {
        repo.git(&["merge-base", "--is-ancestor", "main", branch])
            .assert_success();
    }
    repo.git(&["merge-base", "--is-ancestor", "main", &stack_b[0]])
        .assert_failure();
}

#[test]
fn all_stacks_submits_every_independent_stack_without_prs() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);

    repo.run_stax(&["refresh", "--all-stacks", "--no-pr", "--force", "--yes"])
        .assert_success();

    let remote_branches = repo.list_remote_branches();
    for branch in stack_a.iter().chain(stack_b.iter()) {
        assert!(
            remote_branches.contains(branch),
            "expected {branch} to be submitted; remote branches: {remote_branches:?}"
        );
    }
}

#[test]
fn all_stacks_restores_original_branch() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, _stack_b) = build_two_stacks(&repo);

    repo.git(&["checkout", &stack_a[1]]).assert_success();
    repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"])
        .assert_success();

    assert_eq!(repo.current_branch(), stack_a[1]);
}

#[test]
fn all_stacks_rejects_dirty_tree_without_auto_stash_pop() {
    let repo = TestRepo::new_with_remote();
    build_two_stacks(&repo);

    repo.create_file("dirty.txt", "x");

    let output = repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"]);
    output.assert_failure();
    output.assert_stderr_contains("--auto-stash-pop");
}

#[test]
fn all_stacks_delete_merged_removes_squash_merged_branch_and_preserves_other_stack() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);
    let parent = &stack_a[0];
    let child = &stack_a[1];
    let unrelated = &stack_b[0];

    for branch in stack_a.iter().chain(stack_b.iter()) {
        repo.git(&["push", "-u", "origin", branch]).assert_success();
    }
    repo.squash_merge_branch_on_remote(parent);
    repo.git(&["checkout", child]).assert_success();

    repo.run_stax(&[
        "refresh",
        "--all-stacks",
        "--no-submit",
        "--force",
        "--yes",
        "--delete-merged",
    ])
    .assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(parent),
        "--delete-merged should remove the squash-merged parent; branches: {branches:?}"
    );
    for branch in [child, unrelated] {
        assert!(
            branches.contains(branch),
            "expected surviving branch {branch} to remain tracked locally; branches: {branches:?}"
        );
        repo.git(&["merge-base", "--is-ancestor", "main", branch])
            .assert_success();
    }
    let novel_commit_count = repo.git(&["rev-list", "--count", &format!("main..{child}")]);
    novel_commit_count.assert_success();
    assert_eq!(
        TestRepo::stdout(&novel_commit_count).trim(),
        "1",
        "the surviving child should retain only its novel commit"
    );
}

#[test]
fn all_stacks_delete_merged_is_a_noop_when_no_branches_are_merged() {
    let repo = TestRepo::new_with_remote();
    let (stack_a, stack_b) = build_two_stacks(&repo);
    let before = stack_a
        .iter()
        .chain(stack_b.iter())
        .map(|branch| (branch.clone(), repo.get_commit_sha(branch)))
        .collect::<Vec<_>>();

    repo.git(&["checkout", &stack_a[1]]).assert_success();
    repo.run_stax(&[
        "refresh",
        "--all-stacks",
        "--no-submit",
        "--force",
        "--yes",
        "--delete-merged",
    ])
    .assert_success();

    for (branch, sha) in before {
        assert!(
            repo.list_branches().contains(&branch),
            "unmerged branch {branch} should not be deleted"
        );
        assert_eq!(
            repo.get_commit_sha(&branch),
            sha,
            "{branch} should be a no-op"
        );
    }
    assert_eq!(repo.current_branch(), stack_a[1]);
}

#[test]
fn all_stacks_stops_at_conflict_and_retry_finishes_remaining_stacks() {
    let repo = TestRepo::new_with_remote();

    let stack_a = repo.create_stack(&["refresh-a"]);
    repo.git(&["checkout", "main"]).assert_success();

    let stack_b = repo.create_stack(&["refresh-b"]);
    repo.create_file("conflict.txt", "branch version\n");
    repo.commit("Prepare refresh conflict");

    repo.git(&["checkout", "main"]).assert_success();
    let stack_c = repo.create_stack(&["refresh-c"]);
    let stack_c_before = repo.get_commit_sha(&stack_c[0]);

    repo.simulate_remote_commit(
        "conflict.txt",
        "trunk version\n",
        "Advance trunk with conflict",
    );
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let output = repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"]);
    output.assert_failure();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("Stopped at '{}'", stack_b[0])),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("Done: {}", stack_a[0])),
        "{stdout}"
    );
    assert!(
        stdout.contains(&format!("Remaining: {}", stack_c[0])),
        "{stdout}"
    );
    assert!(
        repo.has_rebase_in_progress(),
        "expected conflict after refresh: {stdout}"
    );
    assert_eq!(
        repo.get_commit_sha(&stack_c[0]),
        stack_c_before,
        "the stack after the conflict must not be restacked"
    );
    let stack_a_after_first_run = repo.get_commit_sha(&stack_a[0]);

    repo.resolve_conflicts_ours();
    repo.run_stax(&["continue"]).assert_success();
    repo.run_stax(&["refresh", "--all-stacks", "--no-submit", "--force", "--yes"])
        .assert_success();

    assert_eq!(repo.current_branch(), stack_b[0]);
    assert_eq!(
        repo.get_commit_sha(&stack_a[0]),
        stack_a_after_first_run,
        "the completed stack must be a no-op on retry"
    );
    for branch in stack_a.iter().chain(stack_b.iter()).chain(stack_c.iter()) {
        repo.git(&["merge-base", "--is-ancestor", "main", branch])
            .assert_success();
    }
    assert_ne!(
        repo.get_commit_sha(&stack_c[0]),
        stack_c_before,
        "the remaining stack should be restacked on retry"
    );
}
