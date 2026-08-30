//! Tests for `st edit` command.
//!
//! Verifies interactive commit editing (reword, drop, squash, fixup)
//! within a branch's own commits.

use crate::common;

use common::{OutputAssertions, TestRepo, run_stax_in_script_with_env};

#[test]
fn edit_on_trunk_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let output = repo.run_stax(&["edit", "--yes"]);
    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk"),
        "Should mention trunk in error: {}",
        stderr
    );
}

#[test]
fn edit_on_branch_with_no_commits_shows_message() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create an empty branch (no -m, no commit)
    repo.run_stax(&["create", "empty-branch"]).assert_success();

    // edit should report no commits
    let output = repo.run_stax(&["edit", "--yes"]);
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("No commits") || stdout.contains("no commits"),
        "Should indicate no commits on branch: stdout={} stderr={}",
        stdout,
        TestRepo::stderr(&output)
    );
}

#[test]
fn edit_on_dirty_tree_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch with a commit
    repo.create_file("a.txt", "hello");
    repo.run_stax(&["create", "-a", "-m", "initial work"])
        .assert_success();

    // Make the tree dirty
    repo.create_file("b.txt", "dirty");

    let output = repo.run_stax(&["edit", "--yes"]);
    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("uncommitted") || stderr.contains("dirty"),
        "Should mention uncommitted changes: {}",
        stderr
    );
}

#[test]
fn edit_drop_removes_commit() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create branch with 2 commits
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "first");
    repo.commit("first commit");
    repo.create_file("b.txt", "second");
    repo.commit("second commit");

    // Verify 2 commits ahead of main
    let log = repo.git(&["log", "--oneline", "main..HEAD"]);
    let commit_count = TestRepo::stdout(&log).lines().count();
    assert_eq!(commit_count, 2, "Should have 2 commits before edit");

    // Build a todo that drops the first commit (keep second)
    // We need to get the commit SHAs to build the todo
    let log_output = repo.git(&["log", "--reverse", "--format=%H %s", "main..HEAD"]);
    let log_str = TestRepo::stdout(&log_output);
    let commits: Vec<&str> = log_str.lines().collect();
    assert_eq!(commits.len(), 2);

    // Write a todo file that drops first, picks second
    let todo = format!(
        "drop {}\npick {}",
        commits[0].split_whitespace().next().unwrap(),
        commits[1].split_whitespace().next().unwrap()
    );
    let todo_path = repo.path().join(".git").join("stax-edit-todo");
    std::fs::write(&todo_path, &todo).unwrap();

    // Run git rebase -i directly with our todo (simulating what st edit does)
    let rebase = repo.git(&[
        "-c",
        &format!("sequence.editor=cp {}", todo_path.to_string_lossy()),
        "rebase",
        "-i",
        "main",
    ]);
    assert!(
        rebase.status.success(),
        "Rebase should succeed: {}",
        TestRepo::stderr(&rebase)
    );

    // Verify only 1 commit remains
    let log_after = repo.git(&["log", "--oneline", "main..HEAD"]);
    let count_after = TestRepo::stdout(&log_after).lines().count();
    assert_eq!(count_after, 1, "Should have 1 commit after dropping one");

    // The remaining commit should be "second commit"
    let remaining = TestRepo::stdout(&log_after);
    assert!(
        remaining.contains("second commit"),
        "Remaining commit should be 'second commit': {}",
        remaining
    );
}

#[test]
fn edit_requires_interactive_terminal() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create branch with a commit
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "content");
    repo.commit("a commit");

    // Without --yes and in a non-interactive terminal, edit needs interaction
    // The run_stax helper runs in a non-interactive context, so this should fail
    // asking for a terminal
    let output = repo.run_stax(&["edit"]);
    // It should either fail (needing terminal) or succeed with --yes
    // In non-interactive, it should fail with a terminal error
    output.assert_failure();
}

#[test]
fn edit_yes_still_requires_terminal_for_action_selection() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "content");
    repo.commit("a commit");

    let output = repo.run_stax(&["edit", "--yes"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("--yes") && stderr.contains("Interactive terminal"),
        "Expected explicit --yes terminal guidance, got: {}",
        stderr
    );
}

/// Real end-to-end `stax edit`, driven through a pty like the interactive
/// prompts it actually uses -- not the extracted `apply_edit_plan` unit tests,
/// which bypass the terminal entirely. This is the path a previous bug in the
/// `GIT_SEQUENCE_EDITOR` command string (fixed alongside the metadata-tracking
/// fix below) went unexercised by: every other test in this file either stops
/// before the rebase runs, or drives `git rebase -i` directly instead of going
/// through `stax edit`'s own binary.
#[test]
fn edit_drop_via_real_interactive_session_updates_metadata_and_supports_undo() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("f1.txt", "f1");
    repo.commit("F1 first");
    repo.create_file("f2.txt", "f2");
    repo.commit("F2 second");

    // Advance main past the recorded boundary so a successful edit genuinely
    // moves parentBranchRevision (otherwise the metadata blob would be
    // rewritten byte-identically and the fix's own tests would be vacuous).
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("m1.txt", "m1");
    repo.commit("M1");
    let main_tip = repo.get_commit_sha("main");
    repo.run_stax(&["checkout", "feature"]).assert_success();

    let head_before = repo.get_commit_sha("feature");
    let meta_before_output = repo.git(&["show", "refs/branch-metadata/feature"]);
    let meta_before: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&meta_before_output)).expect("valid JSON");
    let parent_rev_before = meta_before["parentBranchRevision"]
        .as_str()
        .expect("parentBranchRevision missing")
        .to_string();

    let home = repo.clean_home();
    let output = run_stax_in_script_with_env(
        &repo.path(),
        &["edit", "--yes"],
        // Commit 1/2 menu (pick/reword/drop): Enter keeps the default "pick".
        // Commit 2/2 menu (pick/reword/squash/fixup/drop): four Down presses
        // reach "drop", then Enter. `--yes` skips the final confirmation.
        "wait_for_tui_text \"[1/2]\"; printf '\\n'; \
         wait_for_tui_text \"[2/2]\"; printf '\\033[B\\033[B\\033[B\\033[B\\n'",
        &[("HOME", &home)],
    );
    assert!(
        output.status.success(),
        "stax edit should succeed; stdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        TestRepo::stdout(&output).contains("Edit applied successfully"),
        "expected success message, got: {}",
        TestRepo::stdout(&output)
    );

    assert_ne!(
        repo.get_commit_sha("feature"),
        head_before,
        "the drop should have rewritten feature's head"
    );
    let log_after = repo.git(&["log", "--format=%s", "main..feature"]);
    let messages = TestRepo::stdout(&log_after);
    assert_eq!(
        messages.lines().count(),
        1,
        "expected exactly one commit after dropping F2, got: {}",
        messages
    );
    assert!(messages.contains("F1 first"));

    let meta_output = repo.git(&["show", "refs/branch-metadata/feature"]);
    let meta: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&meta_output)).expect("valid metadata JSON");
    assert_eq!(
        meta["parentBranchRevision"].as_str(),
        Some(main_tip.as_str()),
        "parentBranchRevision should have advanced to main's tip after a real edit"
    );

    // `stax undo` must restore both the branch head and the pre-edit metadata.
    let undo_output = repo.run_stax(&["undo", "--yes"]);
    assert!(
        undo_output.status.success(),
        "stax undo should succeed; stderr: {}",
        TestRepo::stderr(&undo_output)
    );
    assert_eq!(
        repo.get_commit_sha("feature"),
        head_before,
        "undo should restore feature's pre-edit head"
    );
    let restored_meta_output = repo.git(&["show", "refs/branch-metadata/feature"]);
    let restored_meta: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&restored_meta_output)).expect("valid JSON");
    assert_eq!(
        restored_meta["parentBranchRevision"].as_str(),
        Some(parent_rev_before.as_str()),
        "undo must restore the pre-edit parentBranchRevision, not leave the post-edit one"
    );
}
