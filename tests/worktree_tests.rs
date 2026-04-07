mod common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn setup_stack_with_worktrees(with_remote: bool) -> (TestRepo, String, String, PathBuf, PathBuf) {
    let repo = if with_remote {
        TestRepo::new_with_remote()
    } else {
        TestRepo::new()
    };

    repo.run_stax(&["create", "A"]).assert_success();
    let a = repo.current_branch();
    repo.create_file("a.txt", "A1\n");
    repo.commit("A commit");

    if with_remote {
        repo.git(&["push", "-u", "origin", &a]).assert_success();
    }

    repo.run_stax(&["create", "B"]).assert_success();
    let b = repo.current_branch();
    repo.create_file("b.txt", "B1\n");
    repo.commit("B commit");

    if with_remote {
        repo.git(&["push", "-u", "origin", &b]).assert_success();
    }

    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    let wt_b = repo.path().join("wt-b");

    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &a])
        .assert_success();
    repo.git(&["worktree", "add", wt_b.to_str().unwrap(), &b])
        .assert_success();

    (repo, a, b, wt_a, wt_b)
}

fn status_json(repo: &TestRepo, cwd: &Path) -> Value {
    let output = repo.run_stax_in(cwd, &["status", "--json"]);
    assert!(
        output.status.success(),
        "status --json failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    serde_json::from_str(&TestRepo::stdout(&output)).expect("status JSON should parse")
}

fn branch_needs_restack(status: &Value, branch: &str) -> Option<bool> {
    status["branches"].as_array().and_then(|branches| {
        branches
            .iter()
            .find(|b| b["name"].as_str() == Some(branch))
            .and_then(|b| b["needs_restack"].as_bool())
    })
}

#[test]
fn restack_all_handles_branch_checked_out_elsewhere() {
    let (repo, a, _b, _wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    let before = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&before, &a), Some(true));

    let output = repo.run_stax_in(&wt_b, &["restack", "--all", "--quiet"]);
    output.assert_success();
    assert!(
        !TestRepo::stderr(&output).contains("already used by worktree"),
        "Expected no worktree checkout error, got: {}",
        TestRepo::stderr(&output)
    );

    let after = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&after, &a), Some(false));
}

#[test]
fn restack_cleanup_skips_merged_branch_checked_out_in_worktree() {
    let (repo, a, b, _wt_a, _wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--no-ff", &a, "-m", "Merge A"])
        .assert_success();

    // Force another branch to need restack so the merged-branch cleanup path runs.
    repo.run_stax(&["create", "cleanup-trigger"])
        .assert_success();
    let trigger = repo.current_branch();
    repo.create_file("trigger.txt", "trigger\n");
    repo.commit("Trigger commit");

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");
    repo.run_stax(&["checkout", &trigger]).assert_success();

    let before = status_json(&repo, &repo.path());
    assert_eq!(branch_needs_restack(&before, &trigger), Some(true));

    let output = repo.run_stax(&["restack", "--yes"]);
    output
        .assert_success()
        .assert_stdout_contains("Kept")
        .assert_stdout_contains("checked out in another worktree")
        .assert_stdout_contains("Run to remove that worktree:")
        .assert_stdout_contains(&format!("st wt rm {}", a))
        .assert_stdout_contains("Or keep the worktree and free the branch:")
        .assert_stdout_contains("switch main");
    assert!(
        !TestRepo::stderr(&output).contains("cannot locate local branch"),
        "restack should not fail cleanup when a merged branch is checked out elsewhere\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        repo.list_branches().iter().any(|branch| branch == &a),
        "Expected merged branch checked out in another worktree to remain local"
    );

    let metadata_ref = format!("refs/branch-metadata/{}", b);
    let metadata_output = repo.git(&["show", &metadata_ref]);
    metadata_output.assert_success();
    let metadata: Value =
        serde_json::from_str(&TestRepo::stdout(&metadata_output)).expect("Invalid JSON metadata");
    assert_eq!(metadata["parentBranchName"], "main");
}

#[test]
fn sync_restack_handles_branch_checked_out_elsewhere() {
    let (repo, a, _b, _wt_a, wt_b) = setup_stack_with_worktrees(true);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");
    repo.git(&["push", "origin", "main"]).assert_success();

    let output = repo.run_stax_in(
        &wt_b,
        &[
            "sync",
            "--restack",
            "--force",
            "--safe",
            "--no-delete",
            "--quiet",
        ],
    );
    output.assert_success();

    let after = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&after, &a), Some(false));
}

#[test]
fn sync_reports_when_checkout_target_is_in_another_worktree() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "merged-feature"])
        .assert_success();
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "feature\n");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();

    let main_worktree = repo.path().join("wt-main");
    repo.git(&["worktree", "add", main_worktree.to_str().unwrap(), "main"])
        .assert_success();

    repo.merge_branch_on_remote(&branch);

    let output = repo.run_stax(&["sync", "--force"]);
    output.assert_success();
    output.assert_stdout_contains("already checked out in another worktree");

    assert_eq!(
        repo.current_branch(),
        branch,
        "sync should keep the current branch when it cannot switch to main"
    );
    assert!(
        repo.list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "Expected the merged branch to remain local when main is checked out elsewhere"
    );
}

#[test]
fn upstack_restack_handles_branch_checked_out_elsewhere() {
    let (repo, _a, b, wt_a, wt_b) = setup_stack_with_worktrees(false);

    // Advance parent branch A in its own worktree so child B needs restack.
    fs::write(wt_a.join("a2.txt"), "A2\n").expect("write a2 file");
    repo.git_in(&wt_a, &["add", "a2.txt"]).assert_success();
    repo.git_in(&wt_a, &["commit", "-m", "A second commit"])
        .assert_success();

    let before = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&before, &b), Some(true));

    let output = repo.run_stax_in(&wt_a, &["upstack", "restack"]);
    output.assert_success();

    let after = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&after, &b), Some(false));
}

#[test]
fn tui_reorder_restack_path_handles_checked_out_elsewhere() {
    let (repo, a, _b, _wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    // TUI reorder now uses the same branch-targeted rebase path as restack.
    let output = repo.run_stax_in(&wt_b, &["restack", "--all", "--quiet"]);
    output.assert_success();

    let after = status_json(&repo, &wt_b);
    assert_eq!(branch_needs_restack(&after, &a), Some(false));
}

#[test]
fn restack_fails_on_dirty_target_worktree_without_flag() {
    let (repo, _a, _b, wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    fs::write(wt_a.join("dirty.txt"), "dirty\n").expect("write dirty file");

    let output = repo.run_stax_in(&wt_b, &["restack", "--all", "--quiet"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("--auto-stash-pop") && stderr.contains("worktree"),
        "Expected clear dirty worktree error, got: {}",
        stderr
    );
}

#[test]
fn restack_auto_stash_pop_succeeds_and_restores_changes() {
    let (repo, _a, _b, wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    let dirty_file = wt_a.join("dirty.txt");
    fs::write(&dirty_file, "dirty change\n").expect("write dirty file");

    let output = repo.run_stax_in(&wt_b, &["restack", "--all", "--quiet", "--auto-stash-pop"]);
    output.assert_success();

    assert!(
        dirty_file.exists(),
        "Dirty file should still exist after auto-pop"
    );

    let status = repo.git_in(&wt_a, &["status", "--porcelain"]);
    assert!(status.status.success(), "git status should succeed in wt-a");
    assert!(
        !TestRepo::stdout(&status).trim().is_empty(),
        "Expected dirty changes to be restored after auto-pop"
    );

    let stash_list = repo.git_in(&wt_a, &["stash", "list"]);
    assert!(stash_list.status.success());
    assert!(
        TestRepo::stdout(&stash_list).trim().is_empty(),
        "Expected no leftover stash entries after auto-pop"
    );
}

#[test]
fn restack_auto_stash_pop_handles_both_worktrees_dirty() {
    let (repo, _a, _b, wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    fs::write(wt_a.join("dirty-a.txt"), "dirty in A\n").expect("write dirty file in wt_a");
    fs::write(wt_b.join("dirty-b.txt"), "dirty in B\n").expect("write dirty file in wt_b");

    let output = repo.run_stax_in(&wt_b, &["restack", "--all", "--quiet", "--auto-stash-pop"]);
    output.assert_success();

    let status_a = repo.git_in(&wt_a, &["status", "--porcelain"]);
    assert!(
        TestRepo::stdout(&status_a).contains("dirty-a.txt"),
        "Expected dirty changes restored in wt_a"
    );

    let status_b = repo.git_in(&wt_b, &["status", "--porcelain"]);
    assert!(
        TestRepo::stdout(&status_b).contains("dirty-b.txt"),
        "Expected dirty changes restored in wt_b"
    );
}

#[test]
fn restack_interactive_stash_propagates_to_target_worktree() {
    let (repo, _a, _b, wt_a, wt_b) = setup_stack_with_worktrees(false);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main-update.txt", "main update\n");
    repo.commit("Main update");

    fs::write(wt_a.join("dirty-a.txt"), "dirty in A\n").expect("write dirty file in wt_a");
    fs::write(wt_b.join("dirty-b.txt"), "dirty in B\n").expect("write dirty file in wt_b");

    // Simulate interactive "yes" to the stash prompt via a pseudo-TTY.
    // Before the fix, this failed because the interactive stash only cleaned wt_b
    // but didn't enable auto_stash_pop for the rebase layer, so wt_a's dirty state
    // caused: "Cannot restack: worktree has uncommitted changes".
    let output =
        common::run_stax_in_script(&wt_b, &["restack", "--all"], "printf 'y\\n'; sleep 10");
    assert!(
        output.status.success(),
        "Interactive restack should succeed when target worktree is also dirty.\n\
         stdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output),
    );
}

#[test]
fn sync_updates_trunk_when_trunk_checked_out_in_other_worktree() {
    let (repo, _a, _b, _wt_a, wt_b) = setup_stack_with_worktrees(true);

    // Simulate remote main advancing elsewhere.
    repo.simulate_remote_commit("remote-main.txt", "remote main\n", "Remote main update");

    let output = repo.run_stax_in(
        &wt_b,
        &["sync", "--force", "--safe", "--no-delete", "--quiet"],
    );
    output.assert_success();

    let local_main = repo.get_commit_sha("main");
    let remote_main = repo.get_commit_sha("origin/main");
    assert_eq!(
        local_main, remote_main,
        "Expected local main to be updated to origin/main after sync"
    );
}

#[test]
fn sync_keeps_metadata_when_branch_delete_blocked_by_worktree() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "A"]).assert_success();
    let a = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.git(&["push", "-u", "origin", &a]).assert_success();

    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &a])
        .assert_success();

    // Merge A into main and remove remote branch (simulates merged PR with branch still checked out in wt-a).
    repo.git(&["merge", "--no-ff", &a, "-m", "Merge A"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &a])
        .assert_success();

    let output = repo.run_stax_in(&wt_a, &["sync", "--force", "--safe", "--quiet"]);
    output.assert_success();

    let branch_ref = format!("refs/heads/{}", a);
    repo.git(&["show-ref", "--verify", "--quiet", &branch_ref])
        .assert_success();

    let metadata_ref = format!("refs/branch-metadata/{}", a);
    repo.git(&["show", &metadata_ref]).assert_success();
}

#[test]
fn branch_delete_checked_out_in_worktree_shows_fix_commands() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();

    let output = repo.run_stax(&["branch", "delete", &branch, "--force"]);
    assert!(
        !output.status.success(),
        "delete should fail while branch is checked out elsewhere"
    );
    output
        .assert_stderr_contains("linked worktree")
        .assert_stderr_contains("wt-a")
        .assert_stderr_contains(&format!(
            "run st wt rm {} to remove the linked worktree",
            branch
        ))
        .assert_stderr_contains("keep the worktree and free the branch")
        .assert_stderr_contains("switch --detach");
}

#[test]
fn checkout_branch_checked_out_in_worktree_routes_to_it() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();

    let output = repo.run_stax(&["checkout", &branch]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("routing there instead"),
        "expected routing notice, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Current shell did not move automatically."),
        "expected worktree go fallback message, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains(wt_a.to_string_lossy().as_ref()),
        "expected target worktree path in output, got:\n{}",
        stdout
    );
}

#[test]
fn checkout_branch_checked_out_in_worktree_emits_shell_route_payload() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();

    let output = repo.run_stax(&["checkout", &branch, "--shell-output"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let canonical_wt_a = std::fs::canonicalize(&wt_a).expect("canonicalize wt-a");
    assert!(
        stdout.contains(&format!("STAX_SHELL_PATH={}", canonical_wt_a.display())),
        "expected shell path payload, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("STAX_SHELL_MESSAGE=Routed checkout to worktree 'wt-a'"),
        "expected shell message payload, got:\n{}",
        stdout
    );
}

#[test]
fn interactive_checkout_selector_routes_to_worktree() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();

    let output = common::run_stax_in_script(
        &repo.path(),
        &["checkout"],
        "printf '\\033[A'; sleep 1; printf '\\r'",
    );

    assert!(
        output.status.success(),
        "interactive checkout failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("routing there instead"),
        "expected routing notice, got:\n{}",
        stdout
    );
}

#[test]
fn sync_removes_safe_linked_worktree_when_branch_delete_confirmed() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();

    repo.git(&["merge", "--no-ff", &branch, "-m", "Merge A"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branch])
        .assert_success();

    let output = repo.run_stax(&["sync", "--force"]);
    output
        .assert_success()
        .assert_stdout_contains("removed linked worktree")
        .assert_stdout_contains("deleted (local only)");

    assert!(
        !wt_a.exists(),
        "expected sync to remove the linked worktree after confirmation"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "expected merged branch to be deleted locally after linked worktree removal"
    );

    let metadata_ref = format!("refs/branch-metadata/{}", branch);
    let metadata_output = repo.git(&["show", &metadata_ref]);
    assert!(
        !metadata_output.status.success(),
        "expected metadata for deleted branch to be removed"
    );
}

#[test]
fn sync_confirmed_dirty_linked_worktree_removes_it_without_global_force() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();
    fs::write(wt_a.join("dirty.txt"), "dirty\n").expect("write dirty worktree file");

    repo.git(&["merge", "--no-ff", &branch, "-m", "Merge A"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branch])
        .assert_success();

    let output = common::run_stax_in_script(&repo.path(), &["sync"], "printf 'y\\n'");
    output
        .assert_success()
        .assert_stdout_contains("force-remove dirty linked worktree")
        .assert_stdout_contains("removed linked worktree")
        .assert_stdout_contains("deleted (local only)");

    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("sync kept linked worktree"),
        "expected interactive sync confirmation to remove the dirty linked worktree, got:\n{}",
        stdout
    );
    assert!(
        !wt_a.exists(),
        "expected interactive sync confirmation to remove the dirty linked worktree"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "expected interactive sync confirmation to delete the local branch"
    );
}

#[test]
fn sync_force_removes_dirty_linked_worktree_and_local_branch() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_a = repo.path().join("wt-a");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();
    fs::write(wt_a.join("dirty.txt"), "dirty\n").expect("write dirty worktree file");

    repo.git(&["merge", "--no-ff", &branch, "-m", "Merge A"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branch])
        .assert_success();

    let output = repo.run_stax(&["sync", "--force"]);
    output
        .assert_success()
        .assert_stdout_contains("removed linked worktree")
        .assert_stdout_contains("deleted (local only)");

    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("sync kept linked worktree"),
        "expected sync --force to remove the dirty linked worktree, got:\n{}",
        stdout
    );
    assert!(
        !wt_a.exists(),
        "expected sync --force to remove the dirty linked worktree"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "expected sync --force to delete the local branch after removing the dirty worktree"
    );
}

#[test]
fn sync_force_removes_dirty_linked_worktree_even_with_ambiguous_basename() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let duplicate_a_parent = repo.path().join("codex-a");
    let duplicate_b_parent = repo.path().join("codex-b");
    fs::create_dir_all(&duplicate_a_parent).expect("create codex-a dir");
    fs::create_dir_all(&duplicate_b_parent).expect("create codex-b dir");

    let wt_a = duplicate_a_parent.join("stax");
    repo.git(&["worktree", "add", wt_a.to_str().unwrap(), &branch])
        .assert_success();
    fs::write(wt_a.join("dirty.txt"), "dirty\n").expect("write dirty worktree file");

    repo.git(&["branch", "side"]).assert_success();
    let wt_side = duplicate_b_parent.join("stax");
    repo.git(&["worktree", "add", wt_side.to_str().unwrap(), "side"])
        .assert_success();

    repo.git(&["merge", "--no-ff", &branch, "-m", "Merge A"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branch])
        .assert_success();

    let output = repo.run_stax(&["sync", "--force"]);
    output
        .assert_success()
        .assert_stdout_contains("removed linked worktree")
        .assert_stdout_contains("deleted (local only)");

    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("Run to remove that worktree and delete the branch:"),
        "expected sync --force to remove the worktree directly instead of printing recovery hints, got:\n{}",
        stdout
    );
    assert!(
        !wt_a.exists(),
        "expected sync --force to remove the dirty linked worktree even when its basename is ambiguous"
    );
    assert!(
        wt_side.exists(),
        "expected unrelated worktrees with the same basename to remain"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "expected sync --force to delete the local branch after removing the dirty worktree"
    );
}

#[test]
fn sync_delete_upstream_gone_removes_safe_linked_worktree() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["create", "stale-lane"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("lane.txt", "lane\n");
    repo.commit("Lane commit");
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    repo.run_stax(&["checkout", "main"]).assert_success();

    let wt_lane = repo.path().join("wt-stale");
    repo.git(&["worktree", "add", wt_lane.to_str().unwrap(), &branch])
        .assert_success();
    repo.git(&["push", "origin", "--delete", &branch])
        .assert_success();

    let output = repo.run_stax(&["sync", "--force", "--delete-upstream-gone"]);
    output
        .assert_success()
        .assert_stdout_contains("removed linked worktree")
        .assert_stdout_contains("deleted (local only)");

    assert!(
        !wt_lane.exists(),
        "expected upstream-gone sync cleanup to remove the linked worktree"
    );
    assert!(
        !repo
            .list_branches()
            .iter()
            .any(|candidate| candidate == &branch),
        "expected upstream-gone branch to be deleted locally after linked worktree removal"
    );
}

#[test]
fn wt_remove_reports_branch_name_when_worktree_basename_is_generic() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "A"]).assert_success();
    let branch = repo.current_branch();
    repo.create_file("a.txt", "A\n");
    repo.commit("A commit");
    repo.run_stax(&["checkout", "main"]).assert_success();

    let generic_parent = repo.path().join("codex-lane");
    fs::create_dir_all(&generic_parent).expect("create generic worktree parent");

    let generic_worktree = generic_parent.join("stax");
    repo.git(&[
        "worktree",
        "add",
        generic_worktree.to_str().unwrap(),
        &branch,
    ])
    .assert_success();

    let output = repo.run_stax(&["wt", "rm", &branch]);
    output
        .assert_success()
        .assert_stdout_contains(&format!("worktree '{}'", branch));

    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("worktree 'stax'"),
        "expected remove output to use the branch name, got:\n{}",
        stdout
    );
}

#[test]
fn status_and_diff_worktree_smoke_reports_expected_non_empty_changes() {
    let (repo, _a, _b, _wt_a, wt_b) = setup_stack_with_worktrees(false);

    let status = status_json(&repo, &wt_b);
    let has_lines = status["branches"]
        .as_array()
        .map(|branches| {
            branches
                .iter()
                .any(|b| b["lines_added"].as_u64().unwrap_or(0) > 0)
        })
        .unwrap_or(false);
    assert!(has_lines, "Expected at least one branch with line changes");

    let diff_output = repo.run_stax_in(&wt_b, &["diff"]);
    diff_output.assert_success();
    let stdout = TestRepo::stdout(&diff_output);
    assert!(stdout.contains("Diff "), "Expected diff sections in output");
    assert!(
        !stdout.contains("No tracked branches to diff."),
        "Expected tracked branch diff output"
    );
}
