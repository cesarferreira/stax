mod common;

use common::{OutputAssertions, TestRepo};

#[cfg(unix)]
fn install_failing_pre_commit_hook(repo: &TestRepo) {
    use std::os::unix::fs::PermissionsExt;

    let hooks_dir = repo.path().join(".git").join("hooks");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    let hook = hooks_dir.join("pre-commit");
    std::fs::write(&hook, "#!/bin/sh\necho hook failed >&2\nexit 1\n").expect("write failing hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755))
        .expect("chmod failing hook");
}

fn assert_current_parent_contains(repo: &TestRepo, expected: &str) {
    let parent = repo.get_current_parent();
    let expected_lower = expected.to_lowercase();
    assert!(
        parent
            .as_ref()
            .is_some_and(|p| p.to_lowercase().contains(&expected_lower)),
        "Expected current parent to contain '{}', got: {:?}",
        expected,
        parent
    );
}

fn branch_needs_restack(repo: &TestRepo, branch: &str) -> bool {
    repo.get_status_json()["branches"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|entry| entry["name"].as_str() == Some(branch))
        })
        .and_then(|entry| entry["needs_restack"].as_bool())
        .unwrap_or(false)
}

fn create_shared_file_stack(
    repo: &TestRepo,
    parent_name: &str,
    current_name: &str,
) -> (String, String) {
    repo.run_stax(&["bc", parent_name]).assert_success();
    repo.create_file("shared.txt", "safe line\nmiddle\nfeature no\n");
    repo.commit(&format!("Add {}", parent_name));
    let parent = repo.current_branch();

    repo.run_stax(&["bc", current_name]).assert_success();
    repo.create_file("shared.txt", "safe line\nmiddle\nfeature yes\n");
    repo.commit(&format!("Add {}", current_name));
    let current = repo.current_branch();

    (parent, current)
}

fn parent_for_branch(repo: &TestRepo, branch: &str) -> Option<String> {
    repo.get_status_json()["branches"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|entry| entry["name"].as_str() == Some(branch))
        })
        .and_then(|entry| entry["parent"].as_str())
        .map(ToString::to_string)
}

#[test]
fn test_create_below_reparents_current_branch_and_preserves_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    // main -> below-parent -> below-current -> below-child
    let branches = repo.create_stack(&["below-parent", "below-current", "below-child"]);
    let parent = &branches[0];
    let current = &branches[1];
    let child = &branches[2];

    repo.run_stax(&["checkout", current]).assert_success();
    let output = repo.run_stax(&["create", "below-mid", "--below"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented") && stdout.contains("restack"),
        "Expected reparent summary and restack hint, got: {}",
        stdout
    );

    let below_mid = repo.current_branch();
    assert!(
        below_mid.contains("below-mid"),
        "Expected to switch to new below branch, got: {}",
        below_mid
    );
    assert_current_parent_contains(&repo, "below-parent");

    repo.run_stax(&["checkout", current]).assert_success();
    assert_current_parent_contains(&repo, "below-mid");

    repo.run_stax(&["checkout", child]).assert_success();
    assert_current_parent_contains(&repo, "below-current");

    repo.run_stax(&["checkout", parent]).assert_success();
    let children = repo.get_children(parent);
    assert!(
        children.iter().any(|name| name.contains("below-mid")),
        "Expected parent to have below-mid as a direct child, got: {:?}",
        children
    );
}

#[test]
fn test_create_below_works_for_direct_child_of_trunk_via_bc_alias() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["below-root"]);
    let original = &branches[0];

    repo.run_stax(&["checkout", original]).assert_success();
    repo.run_stax(&["bc", "below-root-mid", "--below"])
        .assert_success();

    let below_root_mid = repo.current_branch();
    assert!(
        below_root_mid.contains("below-root-mid"),
        "Expected to switch to new below branch, got: {}",
        below_root_mid
    );
    assert_current_parent_contains(&repo, "main");

    repo.run_stax(&["checkout", original]).assert_success();
    assert_current_parent_contains(&repo, "below-root-mid");
}

#[test]
fn test_create_below_works_via_branch_create() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["below-branch-create"]);
    let original = &branches[0];

    repo.run_stax(&["branch", "create", "below-via-branch", "--below"])
        .assert_success();

    let new_branch = repo.current_branch();
    assert!(
        new_branch.contains("below-via-branch"),
        "Expected branch create --below to switch to new branch, got: {}",
        new_branch
    );
    assert_current_parent_contains(&repo, "main");

    repo.run_stax(&["checkout", original]).assert_success();
    assert_current_parent_contains(&repo, "below-via-branch");
}

#[test]
fn test_create_below_with_message_commits_on_new_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["below-message-parent", "below-message-current"]);
    let parent = &branches[0];
    let current = &branches[1];
    let parent_before = repo.get_commit_sha(parent);
    let current_before = repo.get_commit_sha(current);

    repo.run_stax(&["checkout", current]).assert_success();
    repo.create_file("prep.txt", "prep work\n");

    let output = repo.run_stax(&["create", "-a", "-m", "Prep below", "--below"]);
    output.assert_success();
    output.assert_stdout_contains("Committed: Prep below");

    let prep_branch = repo.current_branch();
    assert!(
        prep_branch.to_lowercase().contains("prep-below"),
        "Expected generated prep-below branch, got: {}",
        prep_branch
    );

    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "Prep below");
    let commit_parent = repo.git(&["rev-parse", "HEAD^"]);
    assert!(
        commit_parent.status.success(),
        "{}",
        TestRepo::stderr(&commit_parent)
    );
    assert_eq!(
        TestRepo::stdout(&commit_parent).trim(),
        parent_before,
        "below commit should be based on the original parent"
    );
    assert_eq!(
        repo.get_commit_sha(current),
        current_before,
        "original branch should not advance when committing below it"
    );

    repo.run_stax(&["checkout", current]).assert_success();
    assert_current_parent_contains(&repo, "prep-below");
    assert!(
        branch_needs_restack(&repo, current),
        "original branch should need restack after the below branch gets a commit"
    );
}

#[test]
fn test_create_below_auto_stashes_dirty_worktree_onto_new_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let (_parent, current) =
        create_shared_file_stack(&repo, "below-stash-parent", "below-stash-current");

    repo.create_file("shared.txt", "hotfix line\nmiddle\nfeature yes\n");
    repo.create_file("cve-notes.txt", "untracked hotfix notes\n");

    let output = repo.run_stax(&["create", "below-hotfix", "--below"]);
    output.assert_success();
    output.assert_stdout_contains("Stashed working tree changes");
    output.assert_stdout_contains("Restored stashed changes on new branch");

    let new_branch = repo.current_branch();
    assert!(
        new_branch.contains("below-hotfix"),
        "Expected to switch to below-hotfix branch, got: {}",
        new_branch
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("shared.txt")).expect("read shared.txt"),
        "hotfix line\nmiddle\nfeature no\n",
        "prepared changes should be reapplied to the lower branch base"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("cve-notes.txt")).expect("read cve-notes.txt"),
        "untracked hotfix notes\n",
        "untracked prepared files should be carried to the new lower branch"
    );
    let status = TestRepo::stdout(&repo.git(&["status", "--porcelain"]));
    assert!(
        status.contains(" M shared.txt"),
        "prepared change should remain uncommitted on the new lower branch"
    );
    assert!(
        status.contains("?? cve-notes.txt"),
        "untracked prepared file should remain uncommitted on the new lower branch"
    );
    assert_eq!(
        parent_for_branch(&repo, &current).as_deref(),
        Some(new_branch.as_str()),
        "original current branch should be reparented onto the new below branch"
    );
    assert!(
        TestRepo::stdout(&repo.git(&["stash", "list"]))
            .trim()
            .is_empty(),
        "auto-stash should be dropped after a clean restore"
    );
}

#[test]
fn test_create_below_with_message_auto_stashes_staged_worktree_before_detach() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let (parent, current) = create_shared_file_stack(
        &repo,
        "below-message-stash-parent",
        "below-message-stash-current",
    );
    let parent_before = repo.get_commit_sha(&parent);
    let current_before = repo.get_commit_sha(&current);

    repo.create_file("shared.txt", "hotfix line\nmiddle\nfeature yes\n");
    assert!(repo.git(&["add", "shared.txt"]).status.success());

    let output = repo.run_stax(&["create", "--below", "-m", "Fix staged CVE"]);
    output.assert_success();
    output.assert_stdout_contains("Stashed working tree changes");
    output.assert_stdout_contains("Committed: Fix staged CVE");

    let new_branch = repo.current_branch();
    assert!(
        new_branch.to_lowercase().contains("fix-staged-cve"),
        "Expected generated fix-staged-cve branch, got: {}",
        new_branch
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("shared.txt")).expect("read shared.txt"),
        "hotfix line\nmiddle\nfeature no\n",
        "commit should be made from the lower branch base"
    );
    assert!(
        TestRepo::stdout(&repo.git(&["status", "--porcelain"]))
            .trim()
            .is_empty(),
        "staged prepared change should be committed"
    );

    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "Fix staged CVE");
    let commit_parent = repo.git(&["rev-parse", "HEAD^"]);
    assert!(
        commit_parent.status.success(),
        "{}",
        TestRepo::stderr(&commit_parent)
    );
    assert_eq!(
        TestRepo::stdout(&commit_parent).trim(),
        parent_before,
        "below commit should be based on the original parent"
    );
    assert_eq!(
        repo.get_commit_sha(&current),
        current_before,
        "original branch should not advance when committing below it"
    );
    assert_eq!(
        parent_for_branch(&repo, &current).as_deref(),
        Some(new_branch.as_str()),
        "original current branch should be reparented onto the generated below branch"
    );
    assert!(
        TestRepo::stdout(&repo.git(&["stash", "list"]))
            .trim()
            .is_empty(),
        "auto-stash should be dropped after a successful commit"
    );
}

#[test]
fn test_create_below_with_message_restores_original_branch_when_auto_stash_conflicts() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let (_parent, current) =
        create_shared_file_stack(&repo, "below-conflict-parent", "below-conflict-current");
    let current_before = repo.get_commit_sha(&current);

    repo.create_file("shared.txt", "safe line\nmiddle\nfeature hotfix\n");
    assert!(repo.git(&["add", "shared.txt"]).status.success());

    let output = repo.run_stax(&["create", "--below", "-m", "Conflicting hotfix"]);
    output.assert_failure();
    output.assert_stderr_contains("Could not apply stashed changes");

    assert_eq!(
        repo.current_branch(),
        current,
        "auto-stash conflict should restore the original branch"
    );
    assert_eq!(
        repo.get_commit_sha(&current),
        current_before,
        "failed below create should not advance the original branch"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("shared.txt")).expect("read shared.txt"),
        "safe line\nmiddle\nfeature hotfix\n",
        "prepared changes should be restored on the original branch"
    );
    assert!(
        TestRepo::stdout(&repo.git(&["status", "--porcelain"])).contains("M  shared.txt"),
        "original staged state should be restored"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|branch| branch.to_lowercase().contains("conflicting-hotfix")),
        "failed below create must not leave the destination branch"
    );
    assert!(
        TestRepo::stdout(&repo.git(&["stash", "list"]))
            .trim()
            .is_empty(),
        "restored auto-stash should be dropped"
    );
}

#[test]
#[cfg(unix)]
fn test_create_below_restores_original_metadata_when_commit_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["below-fail-parent", "below-fail-current"]);
    let parent = &branches[0];
    let current = &branches[1];
    let current_before = repo.get_commit_sha(current);

    repo.run_stax(&["checkout", current]).assert_success();
    repo.create_file("fail-below.txt", "work that should stay\n");
    install_failing_pre_commit_hook(&repo);

    let output = repo.run_stax(&["create", "-a", "-m", "Fail below", "--below"]);
    output.assert_failure();
    output.assert_stderr_contains("No branch was created");

    assert_eq!(
        repo.current_branch(),
        *current,
        "rollback should return to the original branch"
    );
    assert_eq!(
        repo.get_commit_sha(current),
        current_before,
        "original branch should not advance after failed below commit"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|branch| branch.to_lowercase().contains("fail-below")),
        "failed below branch should be deleted"
    );
    assert_current_parent_contains(&repo, parent);
    assert!(
        repo.path().join("fail-below.txt").exists(),
        "rollback should preserve the user's working-tree file"
    );

    repo.run_stax(&["create", "-a", "-m", "Fail below", "--below"])
        .assert_failure();
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|branch| branch.to_lowercase().contains("fail-below-2")),
        "retry should not drift into a suffixed branch"
    );
}

#[test]
#[cfg(unix)]
fn test_create_below_no_verify_skips_hook() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let branches = repo.create_stack(&["below-no-verify-parent", "below-no-verify-current"]);
    let current = &branches[1];
    let current_before = repo.get_commit_sha(current);

    repo.run_stax(&["checkout", current]).assert_success();
    repo.create_file("skip-below-hook.txt", "work that should commit\n");
    install_failing_pre_commit_hook(&repo);

    let output = repo.run_stax(&[
        "create",
        "-a",
        "-m",
        "Skip below hook",
        "--below",
        "--no-verify",
    ]);
    output.assert_success();
    output.assert_stdout_contains("Committed: Skip below hook");

    let new_branch = repo.current_branch();
    assert!(
        new_branch.to_lowercase().contains("skip-below-hook"),
        "Expected generated skip-below-hook branch, got: {}",
        new_branch
    );

    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "Skip below hook");
    assert_eq!(
        repo.get_commit_sha(current),
        current_before,
        "original branch should not advance when committing below it"
    );

    repo.run_stax(&["checkout", current]).assert_success();
    assert_current_parent_contains(&repo, "skip-below-hook");
}

#[test]
fn test_create_below_rejects_from() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    repo.create_stack(&["below-from-conflict"]);

    let output = repo.run_stax(&["create", "bad", "--below", "--from", "main"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("cannot be used") || stderr.contains("conflict"),
        "Expected conflicting --from error, got: {}",
        stderr
    );
}

#[test]
fn test_create_below_untracked_branch_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    repo.git(&["checkout", "-b", "manual-untracked"]);
    let output = repo.run_stax(&["create", "bad", "--below"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("branch track"),
        "Expected untracked branch guidance, got: {}",
        stderr
    );
}

#[test]
fn test_create_below_from_trunk_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let output = repo.run_stax(&["create", "below-main", "--below"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("below trunk") || stderr.contains("Checkout a stacked branch"),
        "Expected below-trunk guidance, got: {}",
        stderr
    );
}

#[test]
fn test_create_below_rejects_conflicting_placement_flags() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();
    repo.create_stack(&["below-conflict"]);

    let output = repo.run_stax(&["create", "bad", "--below", "--insert"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("cannot be used") || stderr.contains("conflict"),
        "Expected conflicting flag error, got: {}",
        stderr
    );
}
