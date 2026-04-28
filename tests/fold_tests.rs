//! Branch fold command integration tests.
//!
//! Covers the post-graphite-parity rewrite: commits are preserved (not
//! squashed), descendants of the folded branch are reparented onto the
//! surviving branch, siblings are rebased, and `--keep` lets the current
//! branch's name survive while the parent ref is deleted.

mod common;

use common::{OutputAssertions, TestRepo};

fn combined(output: &std::process::Output) -> String {
    format!("{}{}", TestRepo::stdout(output), TestRepo::stderr(output))
}

// =============================================================================
// Preconditions — fold bails before the confirmation prompt
// =============================================================================

#[test]
fn test_fold_into_trunk_not_allowed() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-1"]);

    let output = repo.run_stax(&["branch", "fold"]);
    assert!(
        combined(&output).contains("Cannot fold into trunk"),
        "expected 'Cannot fold into trunk' message, got: {}",
        combined(&output)
    );
}

#[test]
fn test_fold_untracked_branch_fails() {
    let repo = TestRepo::new();

    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["branch", "fold"]);
    output.assert_failure();
    assert!(
        combined(&output).contains("not tracked"),
        "expected 'not tracked' in: {}",
        combined(&output)
    );
}

#[test]
fn test_fold_when_on_trunk_bails() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-1"]);
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    let output = repo.run_stax(&["branch", "fold"]);
    output.assert_failure();
    assert!(
        combined(&output).contains("Cannot fold trunk"),
        "expected 'Cannot fold trunk' in: {}",
        combined(&output)
    );
}

#[test]
fn test_fold_with_no_changes_is_a_noop() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    // Empty branch off feature-1 (same SHA, no kids, no siblings)
    repo.git(&["checkout", "-b", "empty-branch"]);
    repo.run_stax(&["branch", "track", "--parent", &feature1]);

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_success();
    assert!(
        combined(&output).contains("Nothing to fold"),
        "expected 'Nothing to fold' in: {}",
        combined(&output)
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

    repo.create_file("dirty.txt", "uncommitted");

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_failure();
    assert!(
        combined(&output).contains("uncommitted"),
        "expected 'uncommitted' in: {}",
        combined(&output)
    );
}

// =============================================================================
// Help / alias surface
// =============================================================================

#[test]
fn test_fold_help_surfaces() {
    let repo = TestRepo::new();

    for args in [
        &["branch", "fold", "--help"][..],
        &["b", "f", "--help"][..],
        &["fold", "--help"][..],
    ] {
        let output = repo.run_stax(args);
        output.assert_success();
        let stdout = TestRepo::stdout(&output);
        assert!(
            stdout.contains("--keep") || stdout.contains("-k"),
            "expected --keep in `{:?}` help: {}",
            args,
            stdout
        );
    }
}

// =============================================================================
// Behaviour
// =============================================================================

#[test]
fn test_fold_default_mode_basic_collapses_leaf() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|n| n == &b),
        "B should be deleted, branches: {:?}",
        branches
    );
    assert_eq!(repo.current_branch(), a, "should end up on A");
    assert!(repo.path().join("a.txt").exists(), "a.txt should be on A");
    assert!(
        repo.path().join("b.txt").exists(),
        "b.txt (from B) should now live on A — commits preserved, not squashed"
    );
}

#[test]
fn test_fold_keep_mode_basic_keeps_current_name() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    repo.run_stax(&["branch", "fold", "--keep", "--yes"])
        .assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|n| n == &a),
        "--keep should delete the parent ref '{}', branches: {:?}",
        a,
        branches
    );
    assert!(
        branches.iter().any(|n| n == &b),
        "--keep should preserve '{}', branches: {:?}",
        b,
        branches
    );
    assert_eq!(repo.current_branch(), b);
    assert_eq!(
        repo.get_current_parent().as_deref(),
        Some("main"),
        "B's parent should now be the grandparent (main)"
    );
}

#[test]
fn test_fold_with_descendants_reparents_them() {
    let repo = TestRepo::new();

    // main → A → B → C
    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
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

    repo.run_stax(&["checkout", &b]);
    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();

    assert!(
        !repo.list_branches().iter().any(|n| n == &b),
        "B should be deleted"
    );

    let json = repo.get_status_json();
    let c_parent = json["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|br| br["name"].as_str() == Some(&c))
        .expect("C should still be tracked")["parent"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert_eq!(c_parent, a, "C's parent should be A after fold");
}

#[test]
fn test_fold_keep_mode_with_descendants_keeps_kid_pointing_at_survivor() {
    let repo = TestRepo::new();

    // main → A → B → C, then `fold --keep` from B.
    // Survivor=B; A is deleted. C's parent_branch_name was B (now survivor)
    // and stays B — kids of `current` need no metadata change in --keep mode.
    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
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

    repo.run_stax(&["checkout", &b]);
    repo.run_stax(&["branch", "fold", "--keep", "--yes"])
        .assert_success();

    let branches = repo.list_branches();
    assert!(branches.iter().any(|n| n == &b), "B should survive --keep");
    assert!(
        !branches.iter().any(|n| n == &a),
        "A ('{}') should be deleted, branches: {:?}",
        a,
        branches
    );

    let json = repo.get_status_json();
    let c_parent = json["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|br| br["name"].as_str() == Some(&c))
        .expect("C should still be tracked")["parent"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert_eq!(c_parent, b, "C's parent should still be B (the survivor)");
}

#[test]
fn test_fold_with_sibling_rebases_it_onto_survivor() {
    let repo = TestRepo::new();

    // main → A → B  (target of fold)
    //          → S  (sibling — touches different files so rebase is conflict-free)
    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    repo.run_stax(&["checkout", &a]);
    repo.run_stax(&["bc", "S"]);
    let s = repo.current_branch();
    repo.create_file("s.txt", "from S");
    repo.commit("S commit");

    repo.run_stax(&["checkout", &b]);
    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|n| n == &s),
        "sibling S should survive, branches: {:?}",
        branches
    );

    repo.run_stax(&["checkout", &s]);
    assert!(
        repo.path().join("s.txt").exists(),
        "S's own file should remain"
    );
    assert!(
        repo.path().join("b.txt").exists(),
        "S should now have B's commits underneath (rebased onto survivor)"
    );

    let json = repo.get_status_json();
    let s_parent = json["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|br| br["name"].as_str() == Some(&s))
        .expect("S should be tracked")["parent"]
        .as_str()
        .unwrap_or("")
        .to_string();
    assert_eq!(s_parent, a, "S's parent should be the survivor (A)");
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

    repo.run_stax(&["fold", "--yes"]).assert_success();
    assert!(
        !repo.list_branches().iter().any(|n| n == &b),
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

    let b_sha = repo.get_commit_sha(&b);
    let a_sha = repo.get_commit_sha(&a);

    repo.run_stax(&["branch", "fold", "--yes"]).assert_success();
    repo.run_stax(&["undo", "--yes"]).assert_success();

    assert!(
        repo.list_branches().iter().any(|n| n == &b),
        "undo should restore B"
    );
    assert_eq!(repo.get_commit_sha(&b), b_sha, "B back to original SHA");
    assert_eq!(repo.get_commit_sha(&a), a_sha, "A back to original SHA");
}

#[test]
fn test_fold_orphaned_pr_hint_when_pr_info_present() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "A"]);
    let a = repo.current_branch();
    repo.create_file("a.txt", "from A");
    repo.commit("A commit");

    repo.run_stax(&["bc", "B"]);
    let b = repo.current_branch();
    repo.create_file("b.txt", "from B");
    repo.commit("B commit");

    // Inject pr_info onto B by writing a metadata blob directly. Avoids
    // needing a real GitHub remote in the test.
    let pr_json = format!(
        r#"{{"parentBranchName":"{}","parentBranchRevision":"{}","prInfo":{{"number":4242,"state":"OPEN","isDraft":false}}}}"#,
        a,
        repo.get_commit_sha(&a)
    );
    let mut child = std::process::Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(pr_json.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let blob_hash = String::from_utf8(out.stdout).unwrap().trim().to_string();
    repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{}", b),
        &blob_hash,
    ]);

    let output = repo.run_stax(&["branch", "fold", "--yes"]);
    output.assert_success();
    let out = combined(&output);
    assert!(
        out.contains("PR #4242"),
        "expected orphaned-PR hint to mention PR #4242, got: {}",
        out
    );
    assert!(
        out.contains("gh pr close 4242"),
        "expected gh pr close hint, got: {}",
        out
    );
}
