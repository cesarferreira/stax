mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_fix_healthy_repo() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a healthy stack
    repo.create_stack(&["feature-a", "feature-b"]);

    // Fix should be a no-op
    let output = repo.run_stax(&["fix", "--yes"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("No issues found"),
        "Expected no issues, got: {}",
        stdout
    );
}

#[test]
fn test_fix_dry_run() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack
    repo.create_stack(&["feature-a"]);

    // Dry run should not change anything
    let output = repo.run_stax(&["fix", "--dry-run"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("No issues found") || stdout.contains("dry run"),
        "Expected dry run output, got: {}",
        stdout
    );
}

#[test]
fn test_fix_after_stack_operations() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create and manipulate stack
    repo.create_stack(&["fix-a", "fix-b", "fix-c"]);

    // Fix should handle clean state
    let output = repo.run_stax(&["fix", "--yes"]);
    output.assert_success();
}

#[test]
fn test_fix_orphaned_metadata() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a branch
    repo.create_stack(&["orphan-test"]);
    let branch_name = repo.current_branch();
    repo.run_stax(&["t"]); // go to trunk

    // Delete branch with raw git (leaving metadata)
    repo.git(&["branch", "-D", &branch_name]);

    // Fix should clean up the orphaned metadata
    let output = repo.run_stax(&["fix", "--yes"]);
    output.assert_success();
}
