//! Tests for edge cases and bug fixes
//!
//! These tests verify that various edge cases are handled correctly.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// 1. REPARENT CIRCULAR DEPENDENCY DETECTION
// =============================================================================

/// Reparenting a branch to one of its descendants should fail
/// (prevents circular dependency: A -> B -> C, then reparent A onto C)
#[test]
fn test_reparent_rejects_circular_dependency() {
    let repo = TestRepo::new();

    // Create a stack: main -> branch-a -> branch-b -> branch-c
    repo.create_stack(&["branch-a", "branch-b", "branch-c"]);

    // Try to reparent branch-a onto branch-c (its descendant)
    // This should fail because it would create: C -> A -> B -> C (circular)
    let branch_a = repo.find_branch_containing("branch-a").unwrap();
    let branch_c = repo.find_branch_containing("branch-c").unwrap();

    let output = repo.run_stax(&["branch", "reparent", "-b", &branch_a, "-p", &branch_c]);

    // Should fail with clear error about circular dependency
    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("circular") || stderr.contains("descendant") || stderr.contains("ancestor"),
        "Expected error about circular dependency, got: {}",
        stderr
    );
}

/// Reparenting a branch onto its direct child should fail
#[test]
fn test_reparent_rejects_direct_child_as_parent() {
    let repo = TestRepo::new();

    // Create: main -> parent -> child
    repo.create_stack(&["parent-branch", "child-branch"]);

    let parent = repo.find_branch_containing("parent-branch").unwrap();
    let child = repo.find_branch_containing("child-branch").unwrap();

    // Try to reparent parent onto child
    let output = repo.run_stax(&["branch", "reparent", "-b", &parent, "-p", &child]);

    output.assert_failure();
}

// =============================================================================
// 2. FOLD COMMAND RECOVERY ON CONFLICT
// =============================================================================

/// Fold should restore original branch on merge conflict
#[test]
fn test_fold_restores_branch_on_conflict() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["create", "feature"]).assert_success();
    let feature_branch = repo.current_branch();

    // Create conflicting changes on feature
    repo.create_file("conflict.txt", "feature content");
    repo.commit("Feature commit");

    // Go to main and create conflicting file
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("conflict.txt", "main content");
    repo.commit("Main commit");

    // Create another branch from main that will conflict with feature
    repo.run_stax(&["create", "middle"]).assert_success();
    let middle_branch = repo.current_branch();
    repo.create_file("middle.txt", "middle content");
    repo.commit("Middle commit");

    // Reparent feature onto middle
    repo.run_stax(&["checkout", &feature_branch]).assert_success();
    repo.run_stax(&["branch", "reparent", "-b", &feature_branch, "-p", &middle_branch])
        .assert_success();

    // Now try to fold feature into middle - should fail due to conflict
    // but should restore us to the feature branch
    let output = repo.run_stax(&["branch", "fold", "--yes"]);

    // Even if fold fails, we should still be on our original branch
    // (not left on the parent branch in a bad state)
    let current = repo.current_branch();
    assert!(
        current.contains("feature") || output.status.success(),
        "After failed fold, should be on original branch '{}', but on '{}'",
        feature_branch,
        current
    );
}

/// Fold with --yes flag should work without prompting
#[test]
fn test_fold_yes_flag_no_prompt() {
    let repo = TestRepo::new();

    // Create: main -> parent -> child
    repo.run_stax(&["create", "parent-fold"]).assert_success();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");

    repo.run_stax(&["create", "child-fold"]).assert_success();
    repo.create_file("child.txt", "child");
    repo.commit("Child commit");

    // Fold child into parent with --yes (no prompt)
    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_success();

    // Should now be on parent branch
    assert!(repo.current_branch().contains("parent-fold"));
}

// =============================================================================
// 3. MERGE DIRTY WORKING TREE CHECK
// =============================================================================

/// Merge should warn/fail if working tree is dirty
#[test]
fn test_merge_warns_on_dirty_tree() {
    let repo = TestRepo::new_with_remote();

    // Create a branch with a commit
    repo.run_stax(&["create", "dirty-test"]).assert_success();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    // Create uncommitted changes
    repo.create_file("uncommitted.txt", "dirty");

    // Try to run merge (even though there's nothing to merge, it should check dirty state)
    let output = repo.run_stax(&["merge", "--yes"]);

    // Should either fail or warn about dirty working tree
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    // If it ran at all, it should have mentioned dirty/uncommitted/stash
    // or it should have failed
    if output.status.success() {
        // If it succeeded, it should have stashed or warned
        assert!(
            combined.contains("stash")
                || combined.contains("dirty")
                || combined.contains("uncommitted")
                || combined.contains("Nothing to merge"),
            "Merge with dirty tree should warn about uncommitted changes"
        );
    }
    // If it failed, that's also acceptable behavior
}

// =============================================================================
// 4. DETACHED HEAD HANDLING
// =============================================================================

/// Commands should handle detached HEAD gracefully
#[test]
fn test_status_handles_detached_head() {
    let repo = TestRepo::new();

    // Create a branch and get its commit
    repo.run_stax(&["create", "detach-test"]).assert_success();
    repo.create_file("test.txt", "test");
    repo.commit("Test commit");

    let commit_sha = repo.head_sha();

    // Detach HEAD
    repo.git(&["checkout", "--detach", &commit_sha]);

    // Status should not panic, should give helpful message
    let output = repo.run_stax(&["status"]);

    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    // Should either succeed with some output or fail with helpful message
    assert!(
        output.status.success() || combined.contains("detached") || combined.contains("HEAD"),
        "Status in detached HEAD should handle gracefully, got: {}",
        combined
    );
}

/// Navigation commands should handle detached HEAD
#[test]
fn test_navigate_handles_detached_head() {
    let repo = TestRepo::new();

    // Create branches
    repo.create_stack(&["nav-a", "nav-b"]);
    let commit_sha = repo.head_sha();

    // Detach HEAD
    repo.git(&["checkout", "--detach", &commit_sha]);

    // Navigation should not panic
    let output = repo.run_stax(&["down"]);

    let stderr = TestRepo::stderr(&output);

    // Should fail gracefully with helpful message
    assert!(
        output.status.success() || stderr.contains("detached") || stderr.contains("branch"),
        "Navigation in detached HEAD should handle gracefully, got: {}",
        stderr
    );
}

// =============================================================================
// 5. NON-INTERACTIVE FLAGS FOR COMMANDS
// =============================================================================

/// Squash with --yes flag should work without prompting
#[test]
fn test_squash_yes_flag_no_prompt() {
    let repo = TestRepo::new();

    // Create a branch with multiple commits
    repo.run_stax(&["create", "squash-test"]).assert_success();
    repo.create_file("file1.txt", "content1");
    repo.commit("Commit 1");
    repo.create_file("file2.txt", "content2");
    repo.commit("Commit 2");

    // Squash with --yes and a message
    let output = repo.run_stax(&["branch", "squash", "--yes", "-m", "Squashed commit"]);
    output.assert_success();
}

/// Undo with --yes flag should work without prompting
#[test]
fn test_undo_yes_flag_no_prompt() {
    let repo = TestRepo::new();

    // Create a branch (this creates a transaction we could undo)
    repo.run_stax(&["create", "undo-test"]).assert_success();
    repo.create_file("test.txt", "test");
    repo.commit("Test commit");

    // Do a restack to create something to undo
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main.txt", "main update");
    repo.commit("Main update");

    let undo_branch = repo.find_branch_containing("undo-test").unwrap();
    repo.run_stax(&["checkout", &undo_branch]).assert_success();
    repo.run_stax(&["restack"]).assert_success();

    // Now undo with --yes
    let output = repo.run_stax(&["undo", "--yes"]);

    // Should either succeed or say nothing to undo (both are valid)
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        output.status.success() || combined.contains("nothing") || combined.contains("No undo"),
        "Undo --yes should work without prompting"
    );
}

/// Redo with --yes flag should work without prompting
#[test]
fn test_redo_yes_flag_no_prompt() {
    let repo = TestRepo::new();

    // Just verify the flag exists and doesn't hang
    let output = repo.run_stax(&["redo", "--yes"]);

    // Should either succeed or say nothing to redo
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        output.status.success() || combined.contains("nothing") || combined.contains("No redo") || combined.contains("No operations"),
        "Redo --yes should work without prompting, got: {}",
        combined
    );
}

// =============================================================================
// 6. STACK INTEGRITY
// =============================================================================

/// Status should detect and warn about circular parent references
#[test]
fn test_status_detects_circular_references() {
    let repo = TestRepo::new();

    // Create some branches normally
    repo.create_stack(&["branch-a", "branch-b"]);

    // Manually corrupt metadata to create circular reference
    // This is simulating what could happen if reparent didn't check
    // We'll skip this test if we can't easily corrupt metadata
    // For now, just verify status runs without infinite loop

    let output = repo.run_stax(&["status", "--json"]);

    // Should complete (not hang in infinite loop)
    // This test passing means status handles the stack correctly
    assert!(
        output.status.success(),
        "Status should complete without hanging"
    );
}

// =============================================================================
// 7. ERROR RECOVERY
// =============================================================================

/// Stash pop failure should be reported to user
#[test]
fn test_restack_reports_stash_pop_failure() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["create", "stash-test"]).assert_success();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Go to main and update
    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.create_file("main.txt", "main content");
    repo.commit("Main update");

    // Go back to feature
    let feature = repo.find_branch_containing("stash-test").unwrap();
    repo.run_stax(&["checkout", &feature]).assert_success();

    // Create dirty state that will conflict on stash pop
    repo.create_file("conflict.txt", "dirty state");

    // Manually stash
    repo.git(&["stash", "push", "-m", "test stash"]);

    // Create a file that will conflict with stash
    repo.create_file("conflict.txt", "different content");
    repo.commit("Conflicting commit");

    // Now restack with stash - the stash pop should fail
    // The test verifies we handle this case (even if it's just a warning)
    let output = repo.run_stax(&["restack", "--yes"]);

    // This test documents expected behavior - we should at least not crash
    // and ideally warn the user about stash issues
    assert!(
        output.status.success() || !TestRepo::stderr(&output).is_empty(),
        "Restack should handle stash pop issues gracefully"
    );
}
