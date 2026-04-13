//! Tests for conflict handling behavior (TDD: written before fixes).
//!
//! Bug 1: Conflict-stop should return non-zero exit code
//! Bug 2: Non-rebase-aware commands during active rebase should give clear error
//! Bug 3: restack --continue should resume from checkpoint, not restart

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Bug 1: Conflict-stop must exit non-zero
// =============================================================================

#[test]
fn test_restack_conflict_exits_nonzero() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();

    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);

    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress after conflict"
    );
    output.assert_failure();

    repo.abort_rebase();
}

#[test]
fn test_sync_conflict_exits_nonzero() {
    let repo = TestRepo::new_with_remote();
    repo.create_conflict_scenario();

    let output = repo.run_stax(&["sync", "--force", "--quiet"]);

    if repo.has_rebase_in_progress() {
        output.assert_failure();
        repo.abort_rebase();
    }
}

#[test]
fn test_upstack_restack_conflict_exits_nonzero() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();

    let output = repo.run_stax(&["upstack", "restack", "--yes", "--quiet"]);

    if repo.has_rebase_in_progress() {
        output.assert_failure();
        repo.abort_rebase();
    }
}

#[test]
fn test_restack_conflict_still_prints_conflict_info() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();

    let output = repo.run_stax(&["restack", "--yes"]);

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("conflict") || stdout.contains("Conflict"),
        "Expected conflict info in output, got:\n{}",
        stdout
    );

    repo.abort_rebase();
}

#[test]
fn test_restack_no_conflict_exits_zero() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["feature-a", "feature-b"]);

    repo.run_stax(&["checkout", &branches[0]]);
    repo.create_file("extra.txt", "extra content");
    repo.commit("Extra commit on feature-a");

    repo.run_stax(&["checkout", &branches[1]]);
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);

    output.assert_success();
}

// =============================================================================
// Bug 2: Commands mid-rebase should give clear error
// =============================================================================

#[test]
fn test_status_during_rebase_gives_clear_error() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);

    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress"
    );

    let output = repo.run_stax(&["status"]);

    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        combined.contains("rebase is in progress")
            || combined.contains("rebase in progress"),
        "Expected 'rebase in progress' message, got:\n{}",
        combined
    );
    output.assert_failure();

    repo.abort_rebase();
}

#[test]
fn test_log_during_rebase_gives_clear_error() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);

    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress"
    );

    let output = repo.run_stax(&["log"]);

    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        combined.contains("rebase is in progress")
            || combined.contains("rebase in progress"),
        "Expected 'rebase in progress' message, got:\n{}",
        combined
    );
    output.assert_failure();

    repo.abort_rebase();
}

#[test]
fn test_continue_during_rebase_still_works() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);

    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress"
    );

    // `continue` should NOT be blocked by the rebase guard.
    // It will report "more conflicts" since we haven't resolved, but it
    // should not say "rebase is in progress" as a blocking error.
    let output = repo.run_stax(&["continue"]);

    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        !combined.contains("A rebase is in progress. Resolve"),
        "continue should not be blocked by rebase guard, got:\n{}",
        combined
    );

    repo.abort_rebase();
}

#[test]
fn test_abort_during_rebase_clears_state() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);

    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress"
    );

    let output = repo.run_stax(&["abort"]);

    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "Expected rebase to be cleared after abort"
    );
}

// =============================================================================
// Bug 3: restack --continue should resume, not restart
// =============================================================================

/// Helper: create a 3-branch stack where branch C conflicts but B does not.
/// Returns (branch_a, branch_b, branch_c).
fn create_multi_branch_conflict_scenario(repo: &TestRepo) -> (String, String, String) {
    // branch A: clean change
    repo.run_stax(&["bc", "stack-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "content for a");
    repo.commit("Commit for stack-a");

    // branch B: clean change
    repo.run_stax(&["bc", "stack-b"]);
    let branch_b = repo.current_branch();
    repo.create_file("b.txt", "content for b");
    repo.commit("Commit for stack-b");

    // branch C: will conflict
    repo.run_stax(&["bc", "stack-c"]);
    let branch_c = repo.current_branch();
    repo.create_file("conflict.txt", "child content\n");
    repo.commit("Commit for stack-c");

    // Go to main and create conflicting change
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update\n");
    repo.create_file("conflict.txt", "main content\n");
    repo.commit("Main conflicting commit");

    // Go back to C
    repo.run_stax(&["checkout", &branch_c]);

    (branch_a, branch_b, branch_c)
}

#[test]
fn test_restack_continue_does_not_restack_already_completed_branches() {
    let repo = TestRepo::new();
    let (_branch_a, branch_b, _branch_c) = create_multi_branch_conflict_scenario(&repo);

    // Restack — A and B succeed, C conflicts
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(
        repo.has_rebase_in_progress(),
        "Expected conflict on stack-c"
    );

    // Record B's SHA before continue
    let b_sha_before = {
        let output = repo.git(&["rev-parse", &branch_b]);
        TestRepo::stdout(&output).trim().to_string()
    };

    // Resolve conflict and continue
    repo.resolve_conflicts_ours();
    let output = repo.run_stax(&["restack", "--continue"]);

    // B should NOT have been rebased again — its SHA should be unchanged
    let b_sha_after = {
        let output = repo.git(&["rev-parse", &branch_b]);
        TestRepo::stdout(&output).trim().to_string()
    };

    assert_eq!(
        b_sha_before, b_sha_after,
        "Branch B was rebased again during --continue (SHA changed from {} to {}). \
         Expected it to be skipped since it was already completed.",
        b_sha_before, b_sha_after
    );

    // The continue should have finished successfully
    assert!(
        !repo.has_rebase_in_progress(),
        "Expected rebase to be finished after continue. Output: {}",
        TestRepo::stdout(&output)
    );
}

#[test]
fn test_restack_continue_after_git_rebase_continue() {
    let repo = TestRepo::new();
    repo.create_conflict_scenario();

    // Drive into conflict
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(
        repo.has_rebase_in_progress(),
        "Expected rebase in progress"
    );

    // Resolve via git directly (bypassing stax)
    repo.resolve_conflicts_ours();
    let git_continue = repo.git_with_env(
        &["rebase", "--continue"],
        &[("GIT_EDITOR", "true")],
    );
    assert!(
        git_continue.status.success(),
        "git rebase --continue failed: {}",
        TestRepo::stderr(&git_continue)
    );
    assert!(
        !repo.has_rebase_in_progress(),
        "Expected rebase to be finished after git rebase --continue"
    );

    // Now stax restack --continue should NOT re-conflict
    let output = repo.run_stax(&["restack", "--continue"]);

    assert!(
        !repo.has_rebase_in_progress(),
        "Expected no rebase in progress after stax restack --continue. Output: {}",
        TestRepo::stdout(&output)
    );
}

#[test]
fn test_restack_continue_completes_remaining_branches() {
    let repo = TestRepo::new();

    // branch A: clean
    repo.run_stax(&["bc", "remain-a"]);
    let _branch_a = repo.current_branch();
    repo.create_file("a.txt", "content for a");
    repo.commit("Commit for remain-a");

    // branch B: will conflict
    repo.run_stax(&["bc", "remain-b"]);
    let _branch_b = repo.current_branch();
    repo.create_file("conflict.txt", "branch b content\n");
    repo.commit("Commit for remain-b");

    // branch C: clean (downstream of B)
    repo.run_stax(&["bc", "remain-c"]);
    let _branch_c = repo.current_branch();
    repo.create_file("c.txt", "content for c");
    repo.commit("Commit for remain-c");

    // branch D: clean (downstream of C)
    repo.run_stax(&["bc", "remain-d"]);
    let branch_d = repo.current_branch();
    repo.create_file("d.txt", "content for d");
    repo.commit("Commit for remain-d");

    // Go to main and create conflicting change
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update\n");
    repo.create_file("conflict.txt", "main content\n");
    repo.commit("Main conflicting commit");

    // Go back to D and restack entire stack
    repo.run_stax(&["checkout", &branch_d]);
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(
        repo.has_rebase_in_progress(),
        "Expected conflict on remain-b"
    );

    // Resolve and continue
    repo.resolve_conflicts_ours();
    let output = repo.run_stax(&["restack", "--continue"]);

    assert!(
        !repo.has_rebase_in_progress(),
        "Expected rebase to complete after continue. Output: {}",
        TestRepo::stdout(&output)
    );

    // All branches should now be up-to-date (no longer needing restack)
    // Verify by checking that a clean restack reports nothing to do
    repo.run_stax(&["checkout", &branch_d]);
    let restack_output = repo.run_stax(&["restack", "--quiet"]);
    let stdout = TestRepo::stdout(&restack_output);
    assert!(
        stdout.contains("up to date") || stdout.contains("nothing to restack") || stdout.is_empty(),
        "Expected stack to be up-to-date after continue, got:\n{}",
        stdout
    );
}
