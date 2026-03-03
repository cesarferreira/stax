mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_abort_no_rebase_in_progress() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Abort when no rebase is in progress should succeed cleanly
    let output = repo.run_stax(&["abort"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Nothing to abort"),
        "Expected 'Nothing to abort' message, got: {}",
        stdout
    );
}

#[test]
fn test_abort_during_rebase_conflict() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a conflict scenario
    let branch_name = repo.create_conflict_scenario();

    // Attempt restack which should cause a conflict
    let _output = repo.run_stax(&["restack"]);
    // The restack may fail or leave a conflict

    // Verify rebase is in progress
    if repo.has_rebase_in_progress() {
        // Abort the rebase
        let output = repo.run_stax(&["abort"]);
        output.assert_success();

        let stdout = TestRepo::stdout(&output);
        assert!(
            stdout.contains("aborted"),
            "Expected abort confirmation, got: {}",
            stdout
        );

        // Verify rebase is no longer in progress
        assert!(
            !repo.has_rebase_in_progress(),
            "Rebase should no longer be in progress after abort"
        );
    } else {
        // If restack didn't leave a conflict (e.g. auto-resolved), verify abort is clean
        let output = repo.run_stax(&["abort"]);
        output.assert_success();
    }

    // Verify we're still on the branch
    assert!(
        repo.current_branch().contains("conflict") || repo.current_branch() == branch_name,
        "Expected to still be on the conflict branch"
    );
}

#[test]
fn test_abort_returns_clean_state() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a simple stack
    repo.create_stack(&["feature-a"]);

    // Abort when clean should be no-op
    let output = repo.run_stax(&["abort"]);
    output.assert_success();
    output.assert_stdout_contains("Nothing to abort");
}
