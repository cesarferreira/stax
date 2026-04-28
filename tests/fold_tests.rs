//! Branch fold command integration tests.
//!
//! Covers the post-graphite-parity rewrite: commits are preserved (not
//! squashed), descendants of the folded branch are reparented onto the
//! surviving branch, and `--keep` lets the current branch's name survive
//! while the parent ref is deleted.

mod common;

use common::{OutputAssertions, TestRepo};

// =============================================================================
// Error / precondition tests (no confirmation required — fold bails early)
// =============================================================================

#[test]
fn test_fold_into_trunk_not_allowed() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-1"]);
    assert!(repo.current_branch_contains("feature-1"));

    let output = repo.run_stax(&["branch", "fold"]);
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        combined.contains("trunk")
            || combined.contains("Cannot fold")
            || combined.contains("submit"),
        "Expected message about trunk, got: {}",
        combined
    );
}

#[test]
fn test_fold_untracked_branch_fails() {
    let repo = TestRepo::new();

    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["branch", "fold"]);
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        combined.contains("not tracked") || combined.contains("track") || !output.status.success(),
        "Expected message about tracking or failure, got: {}",
        combined
    );
}

#[test]
fn test_fold_on_trunk_not_allowed() {
    let repo = TestRepo::new();

    repo.create_stack(&["feature-1"]);
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    let output = repo.run_stax(&["branch", "fold"]);
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        combined.contains("trunk") || combined.contains("not tracked") || !output.status.success(),
        "Expected failure on trunk, got: {}",
        combined
    );
}

#[test]
fn test_fold_no_commits_to_fold() {
    let repo = TestRepo::new();

    // main → feature-1 (with commit) → empty-branch (same SHA as feature-1)
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    repo.git(&["checkout", "-b", "empty-branch"]);
    repo.run_stax(&["branch", "track", "--parent", &feature1]);

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(
        combined.to_lowercase().contains("no commits")
            || combined.contains("0 commit")
            || combined.contains("Nothing to fold"),
        "Expected 'no commits' / 'Nothing to fold' message, got: {}",
        combined
    );
}

#[test]
fn test_fold_with_dirty_working_tree_refused() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "content");
    repo.commit("Feature 2");

    // Make the working tree dirty
    repo.create_file("dirty.txt", "uncommitted");

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    let combined = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert!(!output.status.success(), "Fold should refuse a dirty tree");
    assert!(
        combined.to_lowercase().contains("uncommitted")
            || combined.to_lowercase().contains("dirty"),
        "Expected dirty-tree error, got: {}",
        combined
    );
}

// =============================================================================
// Help / alias tests
// =============================================================================

#[test]
fn test_fold_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "fold", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--keep") || stdout.contains("-k"),
        "Expected --keep flag in help"
    );
    assert!(
        stdout.contains("fold") || stdout.contains("Fold"),
        "Expected 'fold' in help"
    );
}

#[test]
fn test_fold_alias_f() {
    let repo = TestRepo::new();
    repo.run_stax(&["b", "f", "--help"]).assert_success();
}

#[test]
fn test_fold_keep_flag_in_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "fold", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("keep") || stdout.contains("Keep"),
        "Expected 'keep' in fold help: {}",
        stdout
    );
}

#[test]
fn test_fold_top_level_alias_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["fold", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Fold") || stdout.contains("fold"));
}

// =============================================================================
// Behavioural tests (post-rewrite)
// =============================================================================

#[test]
fn test_fold_default_mode_basic_collapses_leaf() {
    let repo = TestRepo::new();

    // main → A (commits a-file) → B (commits b-file)
    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    // Fold leaf B into A
    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_success();

    // B no longer exists
    assert!(
        !repo.list_branches().iter().any(|name| name == &b),
        "Folded branch '{}' should be deleted, branches: {:?}",
        b,
        repo.list_branches()
    );

    // We are checked out on the survivor (A)
    assert_eq!(repo.current_branch(), a, "Should end up on A");

    // Both files are present (commits preserved, not squashed away)
    assert!(repo.path().join("a.txt").exists(), "a.txt should be on A");
    assert!(
        repo.path().join("b.txt").exists(),
        "b.txt (from B) should now live on A after fold"
    );
}

#[test]
fn test_fold_keep_mode_basic_keeps_current_name() {
    let repo = TestRepo::new();

    // main → A → B (leaf)
    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    // Fold with --keep: B's name survives, A is deleted
    let output = repo.run_stax(&["branch", "fold", "--keep", "--yes"]);
    output.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|name| name == &a),
        "--keep should delete the parent ref '{}', branches: {:?}",
        a,
        branches
    );
    assert!(
        branches.iter().any(|name| name == &b),
        "--keep should preserve the current branch '{}', branches: {:?}",
        b,
        branches
    );

    assert_eq!(repo.current_branch(), b, "Should still be on B");

    // B is now reparented onto trunk (since A — its old parent — was the
    // only intermediate branch and trunk is A's parent).
    assert_eq!(
        repo.get_current_parent().as_deref(),
        Some("main"),
        "B's parent should now be the grandparent (main)"
    );

    assert!(repo.path().join("a.txt").exists());
    assert!(repo.path().join("b.txt").exists());
}

#[test]
fn test_fold_with_descendants_reparents_them() {
    let repo = TestRepo::new();

    // main → A → B → C
    repo.run_stax(&["bc", "A"]);
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    repo.run_stax(&["bc", "C"]);
    let c = repo.current_branch();
    repo.create_file("c.txt", "from C");
    repo.commit("C commit");

    // Move to B and fold it (B has child C)
    repo.run_stax(&["checkout", &b]);

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_success();

    // B is gone
    assert!(
        !repo.list_branches().iter().any(|name| name == &b),
        "B should be deleted after fold"
    );

    // C is now a child of A (the survivor)
    let parent_of_c_json = repo.get_status_json();
    let c_branch = parent_of_c_json["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"].as_str() == Some(&c))
        .expect("C should still be tracked");
    let c_parent = c_branch["parent"].as_str().unwrap_or("");
    assert!(
        c_parent.ends_with('A') || c_parent == "A",
        "C's parent should be A after fold, got '{}'",
        c_parent
    );
}

#[test]
fn test_fold_top_level_alias_works() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "A"]);
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    // `stax fold` (no `branch` prefix) should match `gt fold`'s ergonomics
    let output = repo.run_stax(&["fold", "--yes"]);
    output.assert_success();

    assert!(
        !repo.list_branches().iter().any(|name| name == &b),
        "B should be deleted via top-level fold"
    );
}

#[test]
fn test_fold_undo_restores_state() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    let b_sha_before = repo.get_commit_sha(&b);
    let a_sha_before = repo.get_commit_sha(&a);

    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();

    // Undo
    let undo = repo.run_stax(&["undo", "--yes"]);
    undo.assert_success();

    // B exists again, both at original SHAs
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|name| name == &b),
        "Undo should restore '{}', branches: {:?}",
        b,
        branches
    );
    assert_eq!(
        repo.get_commit_sha(&b),
        b_sha_before,
        "B should be at original SHA after undo"
    );
    assert_eq!(
        repo.get_commit_sha(&a),
        a_sha_before,
        "A should be at original SHA after undo"
    );
}
