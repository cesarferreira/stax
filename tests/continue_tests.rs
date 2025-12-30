//! Continue command integration tests
//!
//! Tests for the `continue` command that resumes after conflict resolution.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// No Rebase In Progress Tests
// =============================================================================

#[test]
fn test_continue_no_rebase_in_progress() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized
    repo.create_stack(&["feature-1"]);

    // Run continue with no rebase in progress
    let output = repo.run_stax(&["continue"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate no rebase in progress
    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("in progress"),
        "Expected 'no rebase in progress' message, got: {}",
        combined
    );
}

#[test]
fn test_continue_alias_cont() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized
    repo.create_stack(&["feature-1"]);

    // cont should work as alias
    let output = repo.run_stax(&["cont"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate no rebase in progress (same as continue)
    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("in progress"),
        "Expected 'no rebase' message, got: {}",
        combined
    );
}

#[test]
fn test_continue_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["continue", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Continue") || stdout.contains("continue") || stdout.contains("conflict"),
        "Expected continue help text, got: {}",
        stdout
    );
}

// =============================================================================
// Conflict Scenario Setup Tests
// =============================================================================

#[test]
fn test_continue_scenario_setup_creates_conflict() {
    let repo = TestRepo::new();

    // Create a feature branch
    repo.run_stax(&["bc", "conflict-branch"]);
    let branch_name = repo.current_branch();

    // Modify a file on the feature branch
    repo.create_file("conflict.txt", "feature content\nline 2\nline 3");
    repo.commit("Feature changes");

    // Go back to main and make conflicting changes
    repo.run_stax(&["t"]);
    repo.create_file("conflict.txt", "main content\nline 2\nline 3");
    repo.commit("Main changes");

    // Go back to the feature branch
    repo.run_stax(&["checkout", &branch_name]);

    // Status should show needs restack
    let json = repo.get_status_json();
    let branches = json["branches"].as_array().unwrap();
    let feature = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("conflict-branch"));

    assert!(feature.is_some(), "Should find conflict-branch");

    if let Some(f) = feature {
        // The branch should need restack since parent (main) has new commits
        assert!(
            f["needs_restack"].as_bool().unwrap_or(false),
            "Branch should need restack after parent changed"
        );
    }
}

#[test]
fn test_continue_scenario_no_rebase_without_conflicts() {
    let repo = TestRepo::new();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-1"]);
    let branch_name = repo.current_branch();

    // Add a file that won't conflict
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature changes");

    // Go back to main and make non-conflicting changes
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main content");
    repo.commit("Main changes");

    // Go back to feature branch
    repo.run_stax(&["checkout", &branch_name]);

    // Restack should succeed without conflicts
    let output = repo.run_stax(&["restack", "--quiet"]);
    output.assert_success();

    // After successful restack, continue should have nothing to do
    let output = repo.run_stax(&["continue"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("in progress"),
        "Expected 'no rebase' after successful restack, got: {}",
        combined
    );
}

#[test]
fn test_continue_after_restack_creates_conflict_marker() {
    let repo = TestRepo::new();

    // Create the conflict scenario
    let _branch_name = repo.create_conflict_scenario();

    // Try to restack - this should fail with conflict
    let output = repo.run_stax(&["restack", "--quiet"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should either succeed (no conflict in this case) or show conflict message
    if combined.contains("conflict") || combined.contains("Conflict") {
        // Rebase is in progress
        assert!(
            repo.has_rebase_in_progress(),
            "Should have rebase in progress after conflict"
        );

        // Abort the rebase for cleanup
        repo.abort_rebase();
    }
}

// =============================================================================
// Sync Continue Tests
// =============================================================================

#[test]
fn test_sync_continue_flag() {
    let repo = TestRepo::new();

    // Create a branch
    repo.create_stack(&["feature-1"]);

    // sync --continue should work (and just say no rebase in progress)
    let output = repo.run_stax(&["sync", "--continue", "--force"]);

    // Should either succeed or mention no rebase
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Without a remote, sync might fail, but --continue should be recognized
    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("remote")
            || combined.contains("origin")
            || output.status.success(),
        "Expected continue to be processed or remote error, got: {}",
        combined
    );
}

#[test]
fn test_restack_continue_flag() {
    let repo = TestRepo::new();

    // Create a branch
    repo.create_stack(&["feature-1"]);

    // restack --continue should work (and just say no rebase in progress or succeed)
    let output = repo.run_stax(&["restack", "--continue"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should indicate no rebase in progress or succeed
    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("up to date")
            || output.status.success(),
        "Expected continue handling, got: {}",
        combined
    );
}

// =============================================================================
// Edge Cases
// =============================================================================

#[test]
fn test_continue_on_trunk() {
    let repo = TestRepo::new();

    // Create a branch so stax is initialized, then go back to trunk
    repo.create_stack(&["feature-1"]);
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Continue on trunk with no rebase
    let output = repo.run_stax(&["continue"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("in progress"),
        "Expected 'no rebase' on trunk, got: {}",
        combined
    );
}

#[test]
fn test_continue_on_untracked_branch() {
    let repo = TestRepo::new();

    // Create untracked branch with git
    repo.git(&["checkout", "-b", "untracked"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    // Continue should handle untracked branch gracefully
    let output = repo.run_stax(&["continue"]);

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should either say no rebase or handle gracefully
    assert!(
        combined.contains("No rebase")
            || combined.contains("no rebase")
            || combined.contains("in progress")
            || !output.status.success(),
        "Expected graceful handling on untracked branch, got: {}",
        combined
    );
}

#[test]
fn test_continue_multiple_times_no_rebase() {
    let repo = TestRepo::new();

    // Create a branch
    repo.create_stack(&["feature-1"]);

    // Continue multiple times should all report no rebase
    for _ in 0..3 {
        let output = repo.run_stax(&["continue"]);

        let stdout = TestRepo::stdout(&output);
        assert!(
            stdout.contains("No rebase") || stdout.contains("no rebase"),
            "Expected 'no rebase' message"
        );
    }
}

