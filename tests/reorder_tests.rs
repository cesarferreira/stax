mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_reorder_single_branch_is_noop() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a single branch
    repo.create_stack(&["only-branch"]);

    // Reorder with --yes should report nothing to reorder
    let output = repo.run_stax(&["reorder", "--yes"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("one branch") || stdout.contains("Nothing to reorder"),
        "Expected single-branch message, got: {}",
        stdout
    );
}

#[test]
fn test_reorder_on_trunk_fails() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack but go to trunk
    repo.create_stack(&["reorder-a"]);
    repo.run_stax(&["t"]);

    // Reorder from trunk should fail
    let output = repo.run_stax(&["reorder", "--yes"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot"),
        "Expected trunk error, got stderr: {}",
        stderr
    );
}

#[test]
fn test_reorder_help_shows_flag() {
    let repo = TestRepo::new();

    // Verify --yes flag is accepted
    let output = repo.run_stax(&["reorder", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("--yes");
}
