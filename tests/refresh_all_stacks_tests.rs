//! Integration tests for `st refresh --all-stacks`.

use crate::common::{OutputAssertions, TestRepo};

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
