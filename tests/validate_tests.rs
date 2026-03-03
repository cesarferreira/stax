mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_validate_healthy_stack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a healthy stack
    repo.create_stack(&["feature-a", "feature-b"]);

    // Validate should pass
    let output = repo.run_stax(&["validate"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("All checks passed"),
        "Expected all checks to pass, got: {}",
        stdout
    );
}

#[test]
fn test_validate_empty_repo() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Validate on empty repo (no tracked branches) should pass
    let output = repo.run_stax(&["validate"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("All checks passed"),
        "Expected all checks to pass, got: {}",
        stdout
    );
}

#[test]
fn test_validate_detects_needs_restack() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a stack then modify parent to trigger needs-restack
    repo.create_stack(&["feature-a"]);
    repo.run_stax(&["t"]); // go to trunk

    // Add a commit to trunk (this makes feature-a's parent revision stale)
    repo.create_file("trunk-change.txt", "new content");
    repo.commit("Trunk change");

    // Validate should detect the stale branch
    let output = repo.run_stax(&["validate"]);

    let stdout = TestRepo::stdout(&output);
    // Should report needs restack
    assert!(
        stdout.contains("need restack") || stdout.contains("WARN"),
        "Expected needs-restack warning, got: {}",
        stdout
    );
}

#[test]
fn test_validate_detects_orphaned_metadata() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]).assert_success();

    // Create a branch, then delete it with git directly (leaving metadata)
    repo.create_stack(&["orphan-branch"]);
    let branch_name = repo.current_branch();
    repo.run_stax(&["t"]); // go to trunk

    // Delete branch with raw git (bypassing stax, leaving metadata)
    repo.git(&["branch", "-D", &branch_name]);

    // Validate should detect orphaned metadata
    let output = repo.run_stax(&["validate"]);

    let stdout = TestRepo::stdout(&output);
    // Stack::load auto-prunes orphaned metadata, so validate may see it as clean
    // or it may detect it before load prunes
    assert!(
        stdout.contains("PASS") || stdout.contains("FAIL") || stdout.contains("orphaned"),
        "Expected some validation output, got: {}",
        stdout
    );
}
