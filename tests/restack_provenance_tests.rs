//! Tests for restack provenance: stax should always use `git rebase --onto <onto> <stored_upstream>`
//! rather than falling back to plain `git rebase <onto>`.
//!
//! Regression tests for the scenario where a user's branch had its stored
//! `parentBranchRevision` pointing to a commit that is not in the branch's ancestry
//! (e.g. because `stax branch track` stored the parent's current tip instead of the
//! merge-base). Previously, stax fell back to plain `git rebase <parent>` which
//! could replay unrelated trunk commits and cause spurious conflicts.
//!
//! freephite reference: it always runs
//!   `git rebase --onto <parentBranchName> <parentBranchRevision> <branch>`
//! without any ancestor check.

mod common;

use common::{OutputAssertions, TestRepo};
use std::io::Write;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Helper: write stax metadata directly into git refs.
// This lets tests set up "bad" or "drifted" parentBranchRevision values without
// going through stax commands.
// ---------------------------------------------------------------------------

fn write_branch_metadata_raw(repo: &TestRepo, branch: &str, parent_name: &str, parent_revision: &str) {
    let json = format!(
        r#"{{"parentBranchName":"{}","parentBranchRevision":"{}"}}"#,
        parent_name, parent_revision
    );

    // Write the JSON as a git blob object
    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .spawn()
        .expect("Failed to spawn git hash-object");

    child
        .stdin
        .as_mut()
        .expect("stdin missing")
        .write_all(json.as_bytes())
        .expect("Failed to write metadata JSON to stdin");

    let out = child.wait_with_output().expect("git hash-object failed");
    assert!(out.status.success(), "git hash-object exited non-zero");

    let hash = String::from_utf8(out.stdout)
        .expect("non-utf8 hash output")
        .trim()
        .to_string();

    let ref_name = format!("refs/branch-metadata/{}", branch);
    let status = Command::new("git")
        .args(["update-ref", &ref_name, &hash])
        .current_dir(repo.path())
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("Failed to spawn git update-ref");
    assert!(status.success(), "git update-ref exited non-zero");
}

// =============================================================================
// Happy path: stored revision is the correct merge-base
// =============================================================================

/// Standard case: branch created via stax, parent advances, restack cleans it up.
#[test]
fn test_restack_with_correct_stored_revision_succeeds() {
    let repo = TestRepo::new();

    let branches = repo.create_stack(&["feature"]);
    let feature = &branches[0];

    // Advance main with a non-conflicting change
    repo.git(&["checkout", "main"]);
    repo.create_file("main-extra.txt", "extra main content");
    repo.commit("Extra main commit");

    repo.git(&["checkout", feature]);
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());
}

// =============================================================================
// Key regression: stored revision is NOT in the branch's ancestry
// (simulates the bug where `stax branch track` stored parent's current tip)
// =============================================================================

/// When parentBranchRevision is set to main's current HEAD (which is NOT in the
/// feature branch's commit history), restack should still succeed.
///
/// With the fix: `git rebase --onto main <main_head> feature`
///   → git log <main_head>..feature = only the feature's own commits
///   → replays feature commits cleanly onto new main.
#[test]
fn test_restack_with_non_ancestor_stored_revision_succeeds() {
    let repo = TestRepo::new();

    // Create a feature branch manually (bypassing stax bc) so we control metadata
    repo.git(&["checkout", "-b", "my-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Advance main with non-conflicting changes
    repo.git(&["checkout", "main"]);
    repo.create_file("main-a.txt", "main-a content");
    repo.commit("Main commit A");
    repo.create_file("main-b.txt", "main-b content");
    repo.commit("Main commit B");

    let current_main_sha = repo.get_commit_sha("HEAD");

    // Write metadata with parentBranchRevision = current main HEAD.
    // This is NOT in feature's history (feature branched before these commits).
    write_branch_metadata_raw(&repo, "my-feature", "main", &current_main_sha);

    // Initialize stax trunk
    repo.run_stax(&["set-trunk", "main"]);

    repo.git(&["checkout", "my-feature"]);

    // Restack must succeed — the stored revision, though not a direct ancestor of
    // my-feature, still scopes the replay correctly because git computes:
    //   git log <current_main_sha>..my-feature = "Feature commit" only
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "Rebase should not be in progress after successful restack"
    );
}

// =============================================================================
// Stack of two branches with drifted revisions
// =============================================================================

/// Both branches in a stack have their stored revisions overwritten to a
/// non-ancestor SHA. Restack should still complete cleanly.
#[test]
fn test_stack_restack_with_drifted_revisions_succeeds() {
    let repo = TestRepo::new();

    let branches = repo.create_stack(&["branch-a", "branch-b"]);
    let branch_a = &branches[0];
    let branch_b = &branches[1];

    // Advance main
    repo.git(&["checkout", "main"]);
    repo.create_file("main-extra.txt", "main-extra content");
    repo.commit("Main extra commit");
    let new_main_sha = repo.get_commit_sha("HEAD");

    // Simulate metadata drift by overwriting stored revisions
    write_branch_metadata_raw(&repo, branch_a, "main", &new_main_sha);
    write_branch_metadata_raw(&repo, branch_b, branch_a, &new_main_sha);

    repo.git(&["checkout", branch_b]);
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());
}

// =============================================================================
// sync --restack path (the actual command the user hit: `st rs --restack`)
// =============================================================================

/// `stax sync --restack` goes through a different code path in sync.rs than
/// plain `stax restack`.  Verify it succeeds even when parentBranchRevision
/// is drifted to a non-ancestor SHA.
#[test]
fn test_sync_restack_with_drifted_revision_succeeds() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch via stax and push it
    repo.git(&["checkout", "-b", "sync-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", "sync-feature"]);

    // Advance remote main (simulating another developer's push)
    repo.simulate_remote_commit("main-remote.txt", "remote content", "Remote main commit");

    // Fetch so local main can advance
    repo.git(&["fetch", "origin"]);
    repo.git(&["checkout", "main"]);
    repo.git(&["merge", "--ff-only", "origin/main"]);

    let new_main_sha = repo.get_commit_sha("HEAD");

    // Corrupt metadata: point stored revision to new main HEAD (not in feature ancestry)
    write_branch_metadata_raw(&repo, "sync-feature", "main", &new_main_sha);

    repo.git(&["checkout", "sync-feature"]);

    // Run sync --restack — this is the exact command the user runs as `st rs --restack`
    let output = repo.run_stax(&["sync", "--restack", "--quiet"]);
    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "Rebase should not be in progress after successful sync --restack"
    );
}

// =============================================================================
// Behavioral proof: restack with drifted revision preserves only feature commits
// =============================================================================

/// When stored revision is not in feature's ancestry (triggering the old is_ancestor=FALSE
/// path), restack must still apply ONLY the feature's own commits on top of the new
/// parent — not replay any main history.
///
/// Setup: feature branches at M1. Main adds M2, M3.
/// Stored parentBranchRevision = M2 (a main-only commit NOT in feature's history).
///   → needs_restack: M2 ≠ M3 (current main) → TRUE → restack fires
///   → old code: is_ancestor(M2, feature) = FALSE → falls back to plain `git rebase main`
///   → new code: always `git rebase --onto main M2 feature`
/// Both replay only {F1, F2}. We verify the outcome: exactly 2 commits atop main.
#[test]
fn test_restack_with_non_ancestor_revision_preserves_only_feature_commits() {
    let repo = TestRepo::new();

    // Create feature with exactly 2 commits, branched at initial commit
    repo.git(&["checkout", "-b", "count-feature"]);
    repo.create_file("f1.txt", "file one");
    repo.commit("Feature commit 1");
    repo.create_file("f2.txt", "file two");
    repo.commit("Feature commit 2");

    // Advance main: add M2 first, capture its SHA, then add M3
    repo.git(&["checkout", "main"]);
    repo.create_file("m1.txt", "main one");
    repo.commit("Main commit 1");
    let mid_main_sha = repo.get_commit_sha("HEAD"); // M2 — NOT in feature's history

    repo.create_file("m2.txt", "main two");
    repo.commit("Main commit 2"); // M3 — current main HEAD

    // Store parentBranchRevision = M2 (not in feature history, not equal to current main)
    // This triggers needs_restack (M2 ≠ M3) and hits the non-ancestor path.
    write_branch_metadata_raw(&repo, "count-feature", "main", &mid_main_sha);
    repo.run_stax(&["set-trunk", "main"]);

    repo.git(&["checkout", "count-feature"]);
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());

    // Verify: feature must be exactly 2 commits ahead of main (its own commits only)
    let log_out = repo.git(&["log", "--oneline", "main..count-feature"]);
    let log_str = String::from_utf8_lossy(&log_out.stdout).to_string();
    let feature_only: Vec<&str> = log_str
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    assert_eq!(
        feature_only.len(),
        2,
        "After restack, feature should have exactly 2 commits on top of main \
         (no extra main commits replayed).\nGot {} commit(s):\n{}",
        feature_only.len(),
        feature_only.join("\n")
    );

    // Verify feature is 0 commits behind main (fully rebased)
    let behind_out = repo.git(&["rev-list", "--count", "count-feature..main"]);
    let behind = String::from_utf8_lossy(&behind_out.stdout)
        .trim()
        .parse::<usize>()
        .unwrap_or(99);
    assert_eq!(
        behind, 0,
        "Feature should be 0 commits behind main after restack, got {} behind",
        behind
    );
}

// =============================================================================
// Genuine conflict is still reported correctly (no regression)
// =============================================================================

/// Verify that an actual content conflict still causes restack to stop and
/// report a failure — the provenance fix must not silently swallow real conflicts.
#[test]
fn test_genuine_conflict_still_fails_after_fix() {
    let repo = TestRepo::new();

    // Record main SHA before the conflict commit
    let pre_conflict_sha = repo.get_commit_sha("HEAD");

    // Create feature with a change to shared.txt
    repo.git(&["checkout", "-b", "conflict-feature"]);
    repo.create_file("shared.txt", "feature version\n");
    repo.commit("Feature changes shared.txt");

    // Advance main with a conflicting change to the same file
    repo.git(&["checkout", "main"]);
    repo.create_file("shared.txt", "main version\n");
    repo.commit("Main changes shared.txt");

    // Write metadata with the pre-conflict main SHA as parentBranchRevision
    // (this is the correct merge-base — we want a real conflict, not metadata drift)
    write_branch_metadata_raw(&repo, "conflict-feature", "main", &pre_conflict_sha);
    repo.run_stax(&["set-trunk", "main"]);

    repo.git(&["checkout", "conflict-feature"]);

    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_failure();
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should be in progress after a genuine conflict"
    );

    repo.abort_rebase();
}
