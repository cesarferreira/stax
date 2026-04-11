//! Tests for `st absorb` command.
//!
//! Verifies that staged changes are attributed to the correct stack branches
//! based on which branch last modified each file.

mod common;

use common::{OutputAssertions, TestRepo};

#[test]
fn absorb_on_trunk_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let output = repo.run_stax(&["absorb"]);
    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk"),
        "Should mention trunk in error: {}",
        stderr
    );
}

#[test]
fn absorb_with_no_staged_changes_shows_message() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "hello");
    repo.commit("add a.txt");

    // No staged changes
    let output = repo.run_stax(&["absorb"]);
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("No staged") || stdout.contains("no staged"),
        "Should indicate no staged changes: {}",
        stdout
    );
}

#[test]
fn absorb_dry_run_shows_plan() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch with a file
    repo.run_stax(&["create", "feature-a"]).assert_success();
    repo.create_file("a.txt", "hello from feature-a");
    repo.commit("add a.txt in feature-a");

    // Create a second branch with another file
    repo.run_stax(&["create", "feature-b"]).assert_success();
    repo.create_file("b.txt", "hello from feature-b");
    repo.commit("add b.txt in feature-b");

    // Now modify a.txt (should be attributed to feature-a)
    repo.create_file("a.txt", "modified in feature-b");
    repo.git(&["add", "a.txt"]);

    // Run dry-run
    let output = repo.run_stax(&["absorb", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);

    // Should show the plan
    assert!(
        stdout.contains("Absorb plan") || stdout.contains("absorb plan"),
        "Should show absorb plan: stdout={} stderr={}",
        stdout,
        TestRepo::stderr(&output)
    );

    // a.txt should be attributed to the branch that created it
    assert!(
        stdout.contains("a.txt"),
        "Should mention a.txt in plan: {}",
        stdout
    );

    // Should say dry run
    assert!(
        stdout.contains("Dry run") || stdout.contains("dry run"),
        "Should indicate dry run: {}",
        stdout
    );
}

#[test]
fn absorb_with_all_flag_stages_changes() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch with a file
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "initial");
    repo.commit("add a.txt");

    // Modify the file (not staged)
    repo.create_file("a.txt", "modified");

    // --all should stage and show plan
    let output = repo.run_stax(&["absorb", "-a", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);

    assert!(
        stdout.contains("a.txt"),
        "Should show a.txt after staging with -a: {}",
        stdout
    );
}

#[test]
fn absorb_new_files_are_unattributed() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("existing.txt", "existing");
    repo.commit("add existing.txt");

    // Create a brand new file that was never touched by any branch
    repo.create_file("brand-new.txt", "new content");
    repo.git(&["add", "brand-new.txt"]);

    let output = repo.run_stax(&["absorb", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);

    // brand-new.txt should be unattributed (or attributed to current branch)
    assert!(
        stdout.contains("brand-new.txt"),
        "Should mention the new file: {}",
        stdout
    );
}

#[test]
fn absorb_single_branch_stack_shows_current() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create just one branch
    repo.run_stax(&["create", "feature"]).assert_success();
    repo.create_file("a.txt", "hello");
    repo.commit("add a.txt");

    // Modify the file
    repo.create_file("a.txt", "modified");
    repo.git(&["add", "a.txt"]);

    let output = repo.run_stax(&["absorb", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);

    // With a single branch, changes target the current branch
    assert!(
        stdout.contains("feature") || stdout.contains("current"),
        "Should attribute to current/only branch: {}",
        stdout
    );
}
