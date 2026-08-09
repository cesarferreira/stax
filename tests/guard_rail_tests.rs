//! Guard-rail regression tests: dirty-working-tree checks in `undo`, `redo`,
//! and `restack`, `get`'s branch-name validation, and the main-worktree
//! removal guard.

use crate::common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Dirty-working-tree guards (`--quiet` branch)
// =============================================================================

#[test]
fn undo_quiet_refuses_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.create_stack(&["A", "B"]);

    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();
    repo.run_stax(&["undo", "--yes"]).assert_success();

    repo.create_file("dirty.txt", "uncommitted");

    repo.run_stax(&["undo", "--quiet"])
        .assert_failure()
        .assert_stderr_contains("Working tree is dirty");
}

#[test]
fn redo_quiet_refuses_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.create_stack(&["A", "B"]);

    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();
    repo.run_stax(&["undo", "--yes"]).assert_success();

    repo.create_file("dirty.txt", "uncommitted");

    repo.run_stax(&["redo", "--quiet"])
        .assert_failure()
        .assert_stderr_contains("Working tree is dirty");
}

#[test]
fn restack_quiet_refuses_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.create_stack(&["A", "B"]);

    repo.run_stax(&["checkout", "A"]).assert_success();
    repo.create_file("dirty.txt", "uncommitted");

    repo.run_stax(&["restack", "--quiet"])
        .assert_failure()
        .assert_stderr_contains("Working tree is dirty");
}

// =============================================================================
// `get` branch-name validation
// =============================================================================

#[test]
fn get_rejects_empty_branch_name() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");

    repo.run_stax(&["get", ""])
        .assert_failure()
        .assert_stderr_contains("Branch name cannot be empty");
}

#[test]
fn get_rejects_remote_prefix_with_empty_branch() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");

    repo.run_stax(&["get", "origin/"])
        .assert_failure()
        .assert_stderr_contains("Branch name cannot be empty");
}

#[test]
fn get_rejects_full_ref_argument() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");

    repo.run_stax(&["get", "refs/heads/foo"])
        .assert_failure()
        .assert_stderr_contains("Pass a branch name, not a full ref");
}

// =============================================================================
// Main-worktree removal guard
// =============================================================================

#[test]
fn worktree_remove_refuses_current_main_worktree() {
    let repo = TestRepo::new();

    repo.run_stax(&["worktree", "remove"])
        .assert_failure()
        .assert_stderr_contains("Cannot remove the main worktree");
}

// `run_with_mode` (src/commands/worktree/remove.rs:202-204) checks
// `worktree.is_main` before ever calling `retire_worktree`, for both the
// implicit-current and explicit-by-name paths — so `retire_worktree`'s own
// identical guard at remove.rs:58 is unreachable from `stax worktree remove`.
// This test therefore exercises the same observable error/site as
// `worktree_remove_refuses_current_main_worktree`, just via an explicit name.
#[test]
fn worktree_remove_by_name_refuses_main_worktree() {
    let repo = TestRepo::new();

    repo.run_stax(&["worktree", "remove", "main"])
        .assert_failure()
        .assert_stderr_contains("Cannot remove the main worktree");
}
