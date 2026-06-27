use crate::common::{OutputAssertions, TestRepo};

struct BranchSnapshot {
    name: String,
    before: String,
}

struct RemoteSyncedParentStack {
    submitted: BranchSnapshot,
    leaf: Option<BranchSnapshot>,
    parent_after: String,
}

fn remote_synced_parent_stack(repo: &TestRepo, names: &[&str]) -> RemoteSyncedParentStack {
    assert!(
        names.len() >= 2,
        "fixture requires at least a parent and submitted branch"
    );

    let branches = repo.create_stack(names);
    let parent = branches[0].clone();
    let submitted = BranchSnapshot {
        name: branches[1].clone(),
        before: repo.get_commit_sha(&branches[1]),
    };
    let leaf = branches.get(2).map(|branch| BranchSnapshot {
        name: branch.clone(),
        before: repo.get_commit_sha(branch),
    });

    repo.run_stax(&["checkout", &parent]).assert_success();
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.create_file(&format!("{}-remote-update.txt", names[0]), "parent v2\n");
    repo.commit("Parent update");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();
    let parent_after = repo.get_commit_sha(&parent);

    repo.run_stax(&["checkout", &submitted.name])
        .assert_success();

    RemoteSyncedParentStack {
        submitted,
        leaf,
        parent_after,
    }
}

fn fetch_origin(repo: &TestRepo) {
    repo.git(&["fetch", "origin"]).assert_success();
}

fn assert_contains_commit(repo: &TestRepo, ancestor: &str, descendant: &str, message: &str) {
    let output = repo.git(&["merge-base", "--is-ancestor", ancestor, descendant]);
    assert!(
        output.status.success(),
        "{message}\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
}

fn assert_does_not_contain_commit(
    repo: &TestRepo,
    ancestor: &str,
    descendant: &str,
    message: &str,
) {
    let output = repo.git(&["merge-base", "--is-ancestor", ancestor, descendant]);
    assert!(
        !output.status.success(),
        "{message}\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
}

fn assert_no_temporary_submit_refs(repo: &TestRepo) {
    let temp_refs = repo.git(&["for-each-ref", "--format=%(refname)", "refs/stax/submit"]);
    assert!(
        TestRepo::stdout(&temp_refs).trim().is_empty(),
        "temporary submit refs should be cleaned up"
    );
}

fn assert_no_temporary_submit_worktrees(repo: &TestRepo) {
    let worktrees = repo.git(&["worktree", "list", "--porcelain"]);
    worktrees.assert_success();
    assert!(
        !TestRepo::stdout(&worktrees).contains("stax-submit-"),
        "temporary submit worktrees should be cleaned up"
    );
}

#[test]
fn branch_submit_temporarily_restacks_when_parent_is_remote_synced() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let stack = remote_synced_parent_stack(&repo, &["temp-parent", "temp-child"]);
    repo.create_file("child-extra.txt", "manual extra child commit\n");
    repo.commit("Manual extra child commit");
    let local_child_before = repo.get_commit_sha(&stack.submitted.name);

    repo.run_stax(&["branch", "submit", "--no-pr", "--yes"])
        .assert_success();

    assert_eq!(
        repo.get_commit_sha(&stack.submitted.name),
        local_child_before,
        "scoped submit should not move the local stale child"
    );

    fetch_origin(&repo);
    let remote_child = format!("origin/{}", stack.submitted.name);
    assert_ne!(
        repo.get_commit_sha(&remote_child),
        local_child_before,
        "remote child should be the temporary rebased head"
    );
    repo.git(&["show", &format!("{}:child-extra.txt", remote_child)])
        .assert_success();

    assert_contains_commit(
        &repo,
        &stack.parent_after,
        &remote_child,
        "remote child should include the synced parent update",
    );
    assert_does_not_contain_commit(
        &repo,
        &stack.parent_after,
        &stack.submitted.name,
        "local child should still need restack",
    );
    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}

#[test]
fn upstack_submit_temporarily_restacks_descendants() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let stack =
        remote_synced_parent_stack(&repo, &["temp-us-parent", "temp-us-middle", "temp-us-leaf"]);
    let leaf = stack.leaf.as_ref().expect("fixture should include a leaf");

    repo.run_stax(&["upstack", "submit", "--no-pr", "--yes"])
        .assert_success();

    assert_eq!(
        repo.get_commit_sha(&stack.submitted.name),
        stack.submitted.before
    );
    assert_eq!(repo.get_commit_sha(&leaf.name), leaf.before);

    fetch_origin(&repo);
    let remote_middle = format!("origin/{}", stack.submitted.name);
    let remote_leaf = format!("origin/{}", leaf.name);
    assert_ne!(repo.get_commit_sha(&remote_middle), stack.submitted.before);
    assert_ne!(repo.get_commit_sha(&remote_leaf), leaf.before);

    assert_contains_commit(
        &repo,
        &stack.parent_after,
        &remote_middle,
        "remote middle should include the synced parent update",
    );
    assert_contains_commit(
        &repo,
        &remote_middle,
        &remote_leaf,
        "remote leaf should be pushed on top of the temporary middle head",
    );
    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}

#[test]
fn branch_submit_cleans_up_temporary_state_when_temporary_restack_conflicts() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    repo.run_stax(&["bc", "temp-conflict-parent"])
        .assert_success();
    let parent = repo.current_branch();
    repo.create_file("conflict.txt", "parent v1\n");
    repo.commit("Parent commit");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.run_stax(&["bc", "temp-conflict-child"])
        .assert_success();
    repo.create_file("conflict.txt", "child edit\n");
    repo.commit("Child conflicting commit");
    let child = repo.current_branch();

    repo.run_stax(&["checkout", &parent]).assert_success();
    repo.create_file("conflict.txt", "parent v2\n");
    repo.commit("Parent update");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.run_stax(&["checkout", &child]).assert_success();
    repo.run_stax(&["branch", "submit", "--no-pr", "--yes"])
        .assert_failure();

    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}
