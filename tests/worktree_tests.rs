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
