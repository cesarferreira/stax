//! Tests for `st upstack onto` -- mass reparent current + descendants onto new parent.

mod common;

use common::{OutputAssertions, TestRepo};

/// Helper: find a branch entry in status JSON by exact suffix match.
fn find_branch<'a>(
    branches: &'a [serde_json::Value],
    suffix: &str,
) -> Option<&'a serde_json::Value> {
    branches.iter().find(|b| {
        b["name"]
            .as_str()
            .map(|n| n.ends_with(suffix))
            .unwrap_or(false)
    })
}

#[test]
fn upstack_onto_moves_branch_and_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Build: main -> a -> b -> c
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    repo.run_stax(&["create", "c"]).assert_success();
    repo.create_file("c.txt", "c");
    repo.commit("commit c");

    // Go to b, run upstack onto main
    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Reparented"),
        "Should mention reparenting: {}",
        stdout
    );
    assert!(
        stdout.contains("descendant"),
        "Should mention descendants: {}",
        stdout
    );

    // Verify b's parent is now main
    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");

    let b_entry = find_branch(branches, "b").expect("should find branch b");
    assert_eq!(b_entry["parent"].as_str().unwrap(), "main");

    // c should still be a child of b (subtree preserved)
    let c_entry = find_branch(branches, "c").expect("should find branch c");
    let c_parent = c_entry["parent"].as_str().unwrap();
    assert!(
        c_parent.ends_with("b"),
        "c's parent should still be b, got: {}",
        c_parent
    );
}

#[test]
fn upstack_onto_from_trunk_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_failure();
    output.assert_stderr_contains("trunk");
}

#[test]
fn upstack_onto_circular_dependency_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Go to a, try to reparent onto b (descendant)
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_failure();
    output.assert_stderr_contains("circular");
}

#[test]
fn upstack_onto_single_branch_no_descendants() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Build: main -> a, main -> b (siblings)
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["trunk"]).assert_success();
    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    // Move a onto b
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["upstack", "onto", "b"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Reparented"), "Should reparent: {}", stdout);
    assert!(
        !stdout.contains("descendant"),
        "Leaf branch should have no descendants: {}",
        stdout
    );
}

#[test]
fn upstack_onto_same_parent_is_noop() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    // Try to reparent onto the current parent (main)
    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("already parented") || stdout.contains("Nothing to do"),
        "Should detect no-op: {}",
        stdout
    );
}

#[test]
fn upstack_onto_nonexistent_target_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    let output = repo.run_stax(&["upstack", "onto", "nonexistent"]);
    output.assert_failure();
    output.assert_stderr_contains("does not exist");
}

#[test]
fn upstack_onto_self_fails() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    let output = repo.run_stax(&["upstack", "onto", "a"]);
    output.assert_failure();
    output.assert_stderr_contains("itself");
}

/// `st move <target>` is a graphite-parity alias that dispatches to the same
/// `commands::upstack::onto::run` as `st upstack onto <target>`. Behavioural
/// parity is verified end-to-end: a stack of a → b gets reparented b onto
/// main via the alias, and `status --json` shows the same resulting parent
/// pointer that `upstack onto` produces.
#[test]
fn move_alias_reparents_like_upstack_onto() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["move", "main"]);
    output.assert_success();
    assert!(
        TestRepo::stdout(&output).contains("Reparented"),
        "`st move` should print the same reparent summary as `st upstack onto`",
    );

    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");
    let b_entry = find_branch(branches, "b").expect("should find branch b");
    assert_eq!(b_entry["parent"].as_str().unwrap(), "main");
}

/// `st mv` is the short form. Same dispatch, same outcome — just typing.
#[test]
fn mv_short_alias_works() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");

    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["mv", "main"]);
    output.assert_success();

    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");
    let b_entry = find_branch(branches, "b").expect("should find branch b");
    assert_eq!(b_entry["parent"].as_str().unwrap(), "main");
}

/// The alias must reject the same error cases `upstack onto` does, so the
/// guards in `commands::upstack::onto::run` aren't silently bypassed.
#[test]
fn move_alias_rejects_trunk_and_circular() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // On trunk: "Cannot reparent trunk" — same as upstack onto.
    let output = repo.run_stax(&["move", "main"]);
    output.assert_failure();
    output.assert_stderr_contains("trunk");

    // Circular: reparent a onto its descendant b should fail.
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");
    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");
    repo.run_stax(&["checkout", "a"]);
    let output = repo.run_stax(&["move", "b"]);
    output.assert_failure();
    output.assert_stderr_contains("circular");
}

// =============================================================================
// Git history correctness after reparent (the core bug: #433)
//
// Before the fix, `stax mv` only updated metadata — the git ref was never
// moved. After `stax create b` while on `a`, `b` points to the same commit
// as `a`. Running `stax mv b main` would update b's parent pointer to `main`
// in the metadata, but `b` in git still sits on top of `a`'s history.
// =============================================================================

/// Core regression test for issue #433.
///
/// Scenario that reproduces the exact bug:
///   main → a (commit A) → b (no unique commits, same SHA as a)
///   `stax mv b main`
///
/// After the fix: `b` must point to main's tip, NOT to commit A.
/// Before the fix: `b` still points to A1 (a's commit), so `git log main..b`
/// would show a's commit — phantom commit from the wrong parent.
#[test]
fn mv_with_no_unique_commits_moves_to_new_parent_tip() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // main → a (has commit A)
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    // a → b (no unique commits — b is created at same SHA as a)
    repo.run_stax(&["create", "b"]).assert_success();

    // Verify precondition: b and a point to the same commit
    let a_sha = repo.get_commit_sha("a");
    let b_sha_before = repo.get_commit_sha("b");
    assert_eq!(
        a_sha, b_sha_before,
        "Precondition: b and a should share the same commit before mv"
    );

    let output = repo.run_stax(&["mv", "main"]);
    output.assert_success();

    // After mv: b should be at main's tip (fast-forward, no commits ahead)
    let main_sha = repo.get_commit_sha("main");
    let b_sha_after = repo.get_commit_sha("b");
    assert_eq!(
        main_sha, b_sha_after,
        "After mv to main with no unique commits, b should point to main's tip"
    );

    // git log main..b should be empty (no commits unique to b)
    let log = repo.git(&["rev-list", "--count", "main..b"]);
    let count = String::from_utf8_lossy(&log.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(99);
    assert_eq!(
        count, 0,
        "b should have 0 commits ahead of main after mv (no unique commits to replay)"
    );
}

/// When the branch has unique commits, they must be rebased onto the new parent.
///
/// Scenario: main → a (commit A) → b (commit B, unique to b)
/// `stax mv b main`
/// After: b should have exactly 1 commit ahead of main (commit B only, not A).
#[test]
fn mv_with_unique_commits_rebases_onto_new_parent() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // main → a
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    // a → b with its own commit
    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    let output = repo.run_stax(&["mv", "main"]);
    output.assert_success();

    // b should have exactly 1 commit ahead of main (commit B, not commit A)
    let log = repo.git(&["log", "--oneline", "main..b"]);
    let log_str = String::from_utf8_lossy(&log.stdout).to_string();
    let unique: Vec<&str> = log_str.lines().filter(|l| !l.trim().is_empty()).collect();

    assert_eq!(
        unique.len(),
        1,
        "b should have exactly 1 unique commit (B) above main, got {}:\n{}",
        unique.len(),
        unique.join("\n")
    );
    assert!(
        unique[0].contains("commit B"),
        "The unique commit should be 'commit B', got: {}",
        unique[0]
    );

    // Confirm a's commit is NOT in b's unique history
    let a_in_b = repo.git(&["log", "--oneline", "main..b"]);
    let a_in_b_str = String::from_utf8_lossy(&a_in_b.stdout).to_string();
    assert!(
        !a_in_b_str.contains("commit A"),
        "commit A (from branch a) must NOT appear in b's unique history after mv to main"
    );
}

/// `stax upstack onto` must also always restack (same behaviour as `mv`).
///
/// Reproduces the same scenario via the long form command.
#[test]
fn upstack_onto_always_restacks_git_history() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // main → a (commit A) → b (commit B)
    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["upstack", "onto", "main"]);
    output.assert_success();

    // b should have exactly 1 commit above main (commit B only)
    let log = repo.git(&["log", "--oneline", "main..b"]);
    let log_str = String::from_utf8_lossy(&log.stdout).to_string();
    let unique: Vec<&str> = log_str.lines().filter(|l| !l.trim().is_empty()).collect();

    assert_eq!(
        unique.len(),
        1,
        "upstack onto: b should have exactly 1 unique commit above main, got {}:\n{}",
        unique.len(),
        unique.join("\n")
    );
    assert!(
        !log_str.contains("commit A"),
        "upstack onto: commit A must NOT appear in b's unique history after reparent to main"
    );
}

/// After moving a branch with descendants, the entire subtree must be rebased.
///
/// Scenario: main → a (commit A) → b (commit B) → c (commit C)
/// `stax mv b main`
/// After: b has 1 commit above main (B), c has 1 commit above b (C).
/// Neither b nor c should contain commit A.
#[test]
fn mv_subtree_git_history_correct_after_reparent() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    repo.run_stax(&["create", "c"]).assert_success();
    repo.create_file("c.txt", "content c");
    repo.commit("commit C");

    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["mv", "main"]);
    output.assert_success();

    // b: exactly 1 commit above main (B, not A)
    let b_log = repo.git(&["log", "--oneline", "main..b"]);
    let b_log_str = String::from_utf8_lossy(&b_log.stdout).to_string();
    let b_unique: Vec<&str> = b_log_str.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        b_unique.len(),
        1,
        "b should have exactly 1 unique commit above main after mv, got {}:\n{}",
        b_unique.len(),
        b_unique.join("\n")
    );
    assert!(
        !b_log_str.contains("commit A"),
        "commit A must NOT appear in b's history after mv to main"
    );

    // c: exactly 1 commit above b (C only)
    let c_log = repo.git(&["log", "--oneline", "b..c"]);
    let c_log_str = String::from_utf8_lossy(&c_log.stdout).to_string();
    let c_unique: Vec<&str> = c_log_str.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        c_unique.len(),
        1,
        "c should have exactly 1 unique commit above b after subtree mv, got {}:\n{}",
        c_unique.len(),
        c_unique.join("\n")
    );
    assert!(
        c_log_str.contains("commit C"),
        "c's unique commit should be 'commit C'"
    );
}

/// Metadata parent pointer must also be updated to the new parent after mv.
/// (Regression guard: git history correct but metadata still wrong = still broken.)
#[test]
fn mv_metadata_parent_updated_after_restack() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    repo.run_stax(&["mv", "main"]);

    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid json");
    let branches = json["branches"].as_array().expect("branches array");
    let b_entry = find_branch(branches, "b").expect("should find branch b");

    assert_eq!(
        b_entry["parent"].as_str().unwrap(),
        "main",
        "metadata parent must be 'main' after mv"
    );

    // b should not need a restack (git and metadata are in sync)
    assert_ne!(
        b_entry["needs_restack"].as_bool().unwrap_or(false),
        true,
        "b should not need restack after mv — git and metadata must already agree"
    );
}

/// `--restack` flag is accepted for backward compatibility but now a no-op
/// (restack always happens). Must not error out.
#[test]
fn mv_restack_flag_accepted_for_backward_compat() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    // --restack flag should still be accepted (backward compat) and produce correct output
    let output = repo.run_stax(&["mv", "--restack", "main"]);
    output.assert_success();

    // Git history must still be correct
    let log = repo.git(&["log", "--oneline", "main..b"]);
    let log_str = String::from_utf8_lossy(&log.stdout).to_string();
    let unique: Vec<&str> = log_str.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        unique.len(),
        1,
        "--restack flag: b should still have exactly 1 commit above main"
    );
}

// =============================================================================
// Dirty working tree behaviour
//
// Now that mv always rebases, a dirty working tree blocks the operation.
// --auto-stash-pop stashes before rebasing and pops afterward, matching
// the behaviour of `stax restack --auto-stash-pop`.
// =============================================================================

/// Without --auto-stash-pop, mv with uncommitted changes must fail with a
/// clear message pointing to --auto-stash-pop.
#[test]
fn mv_fails_with_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    // Leave uncommitted changes in the working tree
    repo.create_file("dirty.txt", "uncommitted");

    let output = repo.run_stax(&["mv", "main"]);
    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("uncommitted") || stderr.contains("auto-stash-pop"),
        "Should mention uncommitted changes or --auto-stash-pop, got: {}",
        stderr
    );
}

/// With --auto-stash-pop, mv stashes dirty changes, rebases, then restores them.
#[test]
fn mv_auto_stash_pop_succeeds_with_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    // Leave uncommitted changes
    repo.create_file("dirty.txt", "uncommitted content");

    let output = repo.run_stax(&["mv", "--auto-stash-pop", "main"]);
    output.assert_success();

    // Git history must be correct: b has 1 commit above main (B only)
    let log = repo.git(&["log", "--oneline", "main..b"]);
    let log_str = String::from_utf8_lossy(&log.stdout).to_string();
    let unique: Vec<&str> = log_str.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(
        unique.len(),
        1,
        "--auto-stash-pop: b should have exactly 1 commit above main, got {}:\n{}",
        unique.len(),
        log_str
    );

    // Dirty changes must be restored after the rebase
    let status = repo.git(&["status", "--porcelain"]);
    let status_str = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        status_str.contains("dirty.txt"),
        "--auto-stash-pop: dirty changes should be restored after mv, got:\n{}",
        status_str
    );
}

/// upstack onto also accepts --auto-stash-pop.
#[test]
fn upstack_onto_auto_stash_pop_succeeds_with_dirty_working_tree() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["create", "a"]).assert_success();
    repo.create_file("a.txt", "content a");
    repo.commit("commit A");

    repo.run_stax(&["create", "b"]).assert_success();
    repo.create_file("b.txt", "content b");
    repo.commit("commit B");

    repo.create_file("dirty.txt", "uncommitted content");

    repo.run_stax(&["checkout", "b"]);
    let output = repo.run_stax(&["upstack", "onto", "--auto-stash-pop", "main"]);
    output.assert_success();

    let status = repo.git(&["status", "--porcelain"]);
    let status_str = String::from_utf8_lossy(&status.stdout).to_string();
    assert!(
        status_str.contains("dirty.txt"),
        "upstack onto --auto-stash-pop: dirty changes should be restored, got:\n{}",
        status_str
    );
}
