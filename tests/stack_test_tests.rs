mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_stack_test_true_passes() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack with multiple branches
    repo.create_stack(&["test-a", "test-b"]);

    // Run `true` on all branches - should pass
    let output = repo.run_stax(&["test", "true"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("PASS"),
        "Expected PASS in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("passed"),
        "Expected 'passed' summary, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_false_fails() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["fail-a", "fail-b"]);

    // Run `false` on all branches - should fail
    let output = repo.run_stax(&["test", "false"]);
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("FAIL"),
        "Expected FAIL in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("failed"),
        "Expected 'failed' summary, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_returns_to_original_branch() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    let branches = repo.create_stack(&["orig-a", "orig-b"]);

    // Go to the first branch
    repo.run_stax(&["checkout", &branches[0]]);
    let original = repo.current_branch();

    // Run test
    let _ = repo.run_stax(&["test", "true"]);

    // Should be back on original branch
    assert_eq!(
        repo.current_branch(),
        original,
        "Should return to original branch after stack test"
    );
}

#[test]
fn test_stack_test_fail_fast() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack with multiple branches
    repo.create_stack(&["ff-a", "ff-b", "ff-c"]);

    // Run `false` with --fail-fast
    let output = repo.run_stax(&["test", "--fail-fast", "false"]);
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("fail-fast") || stdout.contains("Stopping"),
        "Expected fail-fast message, got: {}",
        stdout
    );
}

#[test]
fn test_stack_test_with_command() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["cmd-a"]);

    // Run a command that checks for a file
    let output = repo.run_stax(&["test", "test", "-f", "README.md"]);
    output.assert_success();
}
