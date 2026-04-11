//! Tests for `st edit` command.
//!
//! Verifies interactive commit editing (reword, drop, squash, fixup)
//! within a branch's own commits.

mod common;

use common::{OutputAssertions, TestRepo};

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
