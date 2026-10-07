//! After cleanup deletes the branch the user was on, sync/refresh should continue on
//! the next branch of that stack instead of dropping the user on trunk.

use crate::common::{OutputAssertions, TestRepo};

fn push_all(repo: &TestRepo, branches: &[String]) {
    for branch in branches {
        repo.git(&["push", "-u", "origin", branch]).assert_success();
    }
}

/// Two-branch stack where the bottom was squash-merged, with the user still on it.
fn squash_merged_bottom(repo: &TestRepo, tag: &str) -> Vec<String> {
    let branches = repo.create_stack(&[&format!("{tag}-bottom"), &format!("{tag}-child")]);
    push_all(repo, &branches);
    repo.squash_merge_branch_on_remote(&branches[0]);
    repo.git(&["checkout", &branches[0]]).assert_success();
    branches
}

#[test]
fn sync_continues_on_child_after_deleting_current_bottom_branch() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx1");

    let output = repo.run_stax(&["sync", "--force"]);
    output.assert_success();

    assert_eq!(repo.current_branch(), branches[1]);
    assert!(!repo.list_branches().contains(&branches[0]));
    assert_eq!(repo.get_current_parent().as_deref(), Some("main"));
    output.assert_stdout_contains(&format!("checked out {}", branches[1]));
}

#[test]
fn refresh_delete_merged_continues_on_child_with_only_its_own_commit() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx2");

    repo.run_stax(&[
        "refresh",
        "--no-submit",
        "--force",
        "--yes",
        "--delete-merged",
    ])
    .assert_success();

    assert_eq!(repo.current_branch(), branches[1]);
    assert!(!repo.list_branches().contains(&branches[0]));
    let count = repo.git(&["rev-list", "--count", &format!("main..{}", branches[1])]);
    assert_eq!(TestRepo::stdout(&count).trim(), "1");
    assert!(!repo.has_rebase_in_progress());
}

#[test]
fn sync_picks_first_child_by_name_when_the_stack_forks() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx3-bottom", "nx3-zeta"]);
    repo.git(&["checkout", &branches[0]]).assert_success();
    repo.run_stax(&["bc", "nx3-alpha"]).assert_success();
    let alpha = repo.current_branch();
    repo.create_file("alpha.txt", "alpha");
    repo.commit("alpha commit");
    let all = vec![branches[0].clone(), branches[1].clone(), alpha.clone()];
    push_all(&repo, &all);
    repo.squash_merge_branch_on_remote(&branches[0]);
    repo.git(&["checkout", &branches[0]]).assert_success();

    repo.run_stax(&["sync", "--force"]).assert_success();

    let mut children = [branches[1].clone(), alpha];
    children.sort();
    assert_eq!(repo.current_branch(), children[0]);
}

#[test]
fn sync_skips_deleted_middle_branch_and_lands_on_the_top_of_the_stack() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx4-bottom", "nx4-middle", "nx4-top"]);
    push_all(&repo, &branches);
    // Merging the middle branch merges the bottom one with it.
    repo.merge_branch_on_remote(&branches[1]);
    repo.git(&["checkout", &branches[0]]).assert_success();

    repo.run_stax(&["sync", "--force"]).assert_success();

    let remaining = repo.list_branches();
    assert!(!remaining.contains(&branches[0]));
    assert!(!remaining.contains(&branches[1]));
    assert_eq!(repo.current_branch(), branches[2]);
}

#[test]
fn sync_stays_on_trunk_when_the_deleted_branch_has_no_children() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx5-only"]);
    push_all(&repo, &branches);
    repo.squash_merge_branch_on_remote(&branches[0]);
    repo.git(&["checkout", &branches[0]]).assert_success();

    repo.run_stax(&["sync", "--force"]).assert_success();

    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn sync_does_not_move_the_user_when_the_current_branch_survives() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx6");
    repo.git(&["checkout", &branches[1]]).assert_success();

    repo.run_stax(&["sync", "--force"]).assert_success();

    assert_eq!(repo.current_branch(), branches[1]);
}

#[test]
fn sync_falls_back_to_trunk_when_the_child_is_checked_out_in_another_worktree() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx7");
    let worktree_dir = tempfile::TempDir::new().expect("create worktree dir");
    let worktree_path = worktree_dir.path().join("child-wt");
    repo.git(&[
        "worktree",
        "add",
        worktree_path.to_str().unwrap(),
        &branches[1],
    ])
    .assert_success();

    let output = repo.run_stax(&["sync", "--force"]);
    output.assert_success();

    assert_eq!(repo.current_branch(), "main");
    assert!(!repo.list_branches().contains(&branches[0]));
    output.assert_stdout_contains("couldn't check out");
}

#[test]
fn sync_json_reports_the_child_as_the_checkout_target() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx8");

    let output = repo.run_stax(&["sync", "--json", "--force"]);
    output.assert_success();

    let parsed: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(parsed["checkout_change"]["to"], branches[1].as_str());
    assert_eq!(repo.current_branch(), branches[1]);
}
