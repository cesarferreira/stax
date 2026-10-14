//! After cleanup deletes the branch the user was on, sync/refresh should continue on
//! the next branch of that stack instead of dropping the user on trunk.

use crate::common::{OutputAssertions, TestRepo, run_stax_in_script_with_env};

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

#[test]
fn sync_stashes_dirty_work_and_restores_it_on_the_child() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx9");
    repo.create_file("wip.txt", "work in progress");

    repo.run_stax(&["sync", "--stash", "--force"])
        .assert_success();

    assert_eq!(repo.current_branch(), branches[1]);
    assert!(
        repo.path().join("wip.txt").exists(),
        "dirty work is restored"
    );
    let stashes = TestRepo::stdout(&repo.git(&["stash", "list"]));
    assert!(stashes.trim().is_empty(), "no stash left behind: {stashes}");
}

#[test]
fn sync_prompt_names_the_child_and_declining_keeps_everything_in_place() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx10");
    // The sync plan only appears once local trunk has the merge (as in sync_confirm_tests).
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["pull", "origin", "main"]).assert_success();
    repo.git(&["checkout", &branches[0]]).assert_success();

    let home = repo.clean_home();
    // Per-branch mode, read the prompt, then decline the delete.
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        "wait_for_tui_text \"How should sync proceed?\"; printf '\\033[B\\n'; wait_for_tui_text \"Delete '\"; printf 'n\\n'",
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains(&format!("and checkout '{}'", branches[1])),
        "the prompt should name the child it will check out; stdout: {stdout}"
    );
    // Declined: the bottom branch survives and the user is not moved.
    assert!(repo.list_branches().contains(&branches[0]));
    assert_eq!(repo.current_branch(), branches[0]);
}

#[test]
fn undo_after_landing_on_the_child_restores_the_deleted_branch_and_parentage() {
    let repo = TestRepo::new_with_remote();
    let branches = squash_merged_bottom(&repo, "nx11");
    let bottom_tip = repo.get_commit_sha(&branches[0]);

    repo.run_stax(&["sync", "--force"]).assert_success();
    assert_eq!(repo.current_branch(), branches[1]);

    repo.run_stax(&["undo", "--yes"]).assert_success();

    assert!(repo.list_branches().contains(&branches[0]));
    assert_eq!(repo.get_commit_sha(&branches[0]), bottom_tip);
    repo.git(&["checkout", &branches[1]]).assert_success();
    assert_eq!(
        repo.get_current_parent().as_deref(),
        Some(branches[0].as_str()),
        "undo must put the child back under its original parent"
    );
    // Report where undo leaves the user so a regression is visible.
    let after = repo.current_branch();
    assert!(!repo.has_rebase_in_progress(), "left on {after}");
}

/// The upstream-gone cleanup pass (not the merged pass) deletes the current branch.
/// `--no-delete` switches the merged pass off, so only `--delete-upstream-gone` runs.
#[test]
fn upstream_gone_cleanup_also_continues_on_the_child() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx12-bottom", "nx12-child"]);
    push_all(&repo, &branches);

    // Land the bottom branch on trunk without a merge commit and publish it, so it
    // has no unique work (the local-only-work guard) and its remote branch can go.
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--ff-only", &branches[0]])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branches[0]])
        .assert_success();
    repo.git(&["checkout", &branches[0]]).assert_success();

    let output = repo.run_stax(&["sync", "--force", "--no-delete", "--delete-upstream-gone"]);
    output.assert_success();
    output.assert_stdout_contains("upstream-gone");

    assert!(!repo.list_branches().contains(&branches[0]));
    assert_eq!(repo.current_branch(), branches[1]);
    assert_eq!(repo.get_current_parent().as_deref(), Some("main"));
}

/// The gone pass must not drop the user on a branch it is leaving: with no children
/// the user stays on trunk.
#[test]
fn upstream_gone_cleanup_without_children_stays_on_trunk() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx13-only"]);
    push_all(&repo, &branches);
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--ff-only", &branches[0]])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branches[0]])
        .assert_success();
    repo.git(&["checkout", &branches[0]]).assert_success();

    repo.run_stax(&["sync", "--force", "--no-delete", "--delete-upstream-gone"])
        .assert_success();

    assert!(!repo.list_branches().contains(&branches[0]));
    assert_eq!(repo.current_branch(), "main");
}

/// `refresh` must continue with the rest of the stack: after landing on the child it
/// pushes the restacked child. (`--no-pr` keeps this offline; the push is the part of
/// the submit phase that depends on being on the child instead of trunk.)
#[test]
fn refresh_pushes_the_restacked_child_after_the_bottom_branch_is_deleted() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    let branches = squash_merged_bottom(&repo, "nx14");
    let remote_child_before =
        TestRepo::stdout(&repo.git(&["rev-parse", &format!("origin/{}", branches[1])]));

    repo.run_stax(&["refresh", "--no-pr", "--force", "--yes", "--delete-merged"])
        .assert_success();

    assert_eq!(repo.current_branch(), branches[1]);
    let local = TestRepo::stdout(&repo.git(&["rev-parse", &branches[1]]));
    let remote = TestRepo::stdout(&repo.git(&["ls-remote", "origin", &branches[1]]));
    assert!(
        remote.starts_with(local.trim()),
        "remote child should have been pushed at the restacked tip {local}; ls-remote: {remote}"
    );
    assert_ne!(
        local.trim(),
        remote_child_before.trim(),
        "the child was restacked, so its tip must have changed"
    );
}

/// Bottom and middle are merged; the user keeps the middle branch at the prompt. The
/// run must continue on the top branch, not on the merged middle branch it left alone.
#[test]
fn declined_merged_middle_branch_is_not_chosen_as_the_next_branch() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["nx15-bottom", "nx15-middle", "nx15-top"]);
    push_all(&repo, &branches);
    repo.merge_branch_on_remote(&branches[1]);
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["pull", "origin", "main"]).assert_success();
    repo.git(&["checkout", &branches[0]]).assert_success();

    let home = repo.clean_home();
    // Per-branch mode; accept the first prompt (the current bottom branch), decline the
    // second. dialoguer answers y/n immediately, so no newline: a stray one would accept
    // the next prompt's default.
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        &format!(
            "wait_for_tui_text \"How should sync proceed?\"; printf '\\033[B\\n'; \
             wait_for_tui_text \"Delete '{bottom}'\"; printf 'y'; \
             wait_for_tui_text \"Delete '{middle}'\"; printf 'n'",
            bottom = branches[0],
            middle = branches[1],
        ),
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));

    let remaining = repo.list_branches();
    let transcript = format!(
        "branches={remaining:?} current={} stdout:\n{}",
        repo.current_branch(),
        TestRepo::stdout(&out)
    );
    assert!(
        !remaining.contains(&branches[0]),
        "bottom deleted; {transcript}"
    );
    assert!(
        remaining.contains(&branches[1]),
        "declined middle kept; {transcript}"
    );
    assert_eq!(repo.current_branch(), branches[2], "{transcript}");
}
