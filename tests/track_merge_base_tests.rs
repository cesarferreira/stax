//! Tests for `stax branch track` storing the merge-base as `parentBranchRevision`.
//!
//! freephite reference: `engine.trackBranch` stores `git.getMergeBase(branch, parent)`,
//! not the parent's current HEAD.  Storing the tip as the revision causes
//! `git rebase --onto` to scope the replay incorrectly when the branch was created
//! from an older parent commit.

mod common;

use common::{OutputAssertions, TestRepo};

// ---------------------------------------------------------------------------
// Helper: read parentBranchRevision from stax metadata
// ---------------------------------------------------------------------------

fn read_parent_revision(repo: &TestRepo, branch: &str) -> String {
    let ref_name = format!("refs/branch-metadata/{}", branch);
    let out = repo.git(&["cat-file", "blob", &ref_name]);
    assert!(
        out.status.success(),
        "Could not read metadata ref for '{}': {}",
        branch,
        String::from_utf8_lossy(&out.stderr)
    );

    let json = String::from_utf8(out.stdout).expect("non-utf8 metadata");
    // Parse parentBranchRevision from the JSON (simple substring extraction)
    let key = r#""parentBranchRevision":""#;
    let start = json.find(key).expect("parentBranchRevision key missing") + key.len();
    let end = json[start..].find('"').expect("closing quote missing") + start;
    json[start..end].to_string()
}

fn get_merge_base(repo: &TestRepo, a: &str, b: &str) -> String {
    let out = repo.git(&["merge-base", a, b]);
    assert!(out.status.success(), "git merge-base failed");
    String::from_utf8(out.stdout)
        .expect("non-utf8")
        .trim()
        .to_string()
}

// =============================================================================
// Interactive `stax branch track --parent <p>` stores merge-base
// =============================================================================

/// When tracking a branch that diverged from main at an older commit, the stored
/// parentBranchRevision must be the merge-base (divergence point), NOT main's
/// current HEAD.
#[test]
fn test_branch_track_stores_merge_base_not_parent_tip() {
    let repo = TestRepo::new();

    // Record main SHA before adding any feature commits
    let divergence_sha = repo.get_commit_sha("HEAD");

    // Create a feature branch manually and add a commit
    repo.git(&["checkout", "-b", "my-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Advance main PAST the divergence point (so tip ≠ merge-base)
    repo.git(&["checkout", "main"]);
    repo.create_file("main-extra.txt", "extra");
    repo.commit("Main advanced");

    // The current main tip is now different from divergence_sha
    let main_tip = repo.get_commit_sha("HEAD");
    assert_ne!(divergence_sha, main_tip, "main must have advanced");

    // Initialize stax and track the feature branch
    repo.run_stax(&["set-trunk", "main"]);
    repo.git(&["checkout", "my-feature"]);

    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    output.assert_success();

    // The stored revision must equal the merge-base, not main's current tip
    let stored_rev = read_parent_revision(&repo, "my-feature");
    let expected_merge_base = get_merge_base(&repo, "main", "my-feature");

    assert_eq!(
        stored_rev, expected_merge_base,
        "parentBranchRevision should be the merge-base (divergence point), \
         not the parent's current tip.\n  stored:   {}\n  expected: {}",
        stored_rev, expected_merge_base
    );
    assert_ne!(
        stored_rev, main_tip,
        "parentBranchRevision must NOT be the parent's current tip"
    );
}

/// When the branch was created at main's current HEAD (no advance), the stored
/// revision is both the merge-base and the tip — function should still succeed.
#[test]
fn test_branch_track_at_current_tip_stores_tip_as_merge_base() {
    let repo = TestRepo::new();

    let main_tip = repo.get_commit_sha("HEAD");

    // Create a feature branch right at main's current HEAD
    repo.git(&["checkout", "-b", "fresh-feature"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    repo.run_stax(&["set-trunk", "main"]);
    repo.git(&["checkout", "fresh-feature"]);

    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    output.assert_success();

    let stored_rev = read_parent_revision(&repo, "fresh-feature");
    let expected = get_merge_base(&repo, "main", "fresh-feature");

    assert_eq!(stored_rev, expected);
    // In this case merge-base == tip since feature was created right at tip
    assert_eq!(stored_rev, main_tip);
}

// =============================================================================
// End-to-end: track + restack with a diverged main
// =============================================================================

/// Full round-trip: branch off old main → advance main → `stax branch track` →
/// `stax restack`. Must succeed without conflict because the stored revision is
/// the correct merge-base, so `git rebase --onto` replays only the feature commits.
#[test]
fn test_track_then_restack_with_diverged_main_succeeds() {
    let repo = TestRepo::new();

    // Create feature at current main (divergence point)
    repo.git(&["checkout", "-b", "long-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit 1");
    repo.create_file("feature2.txt", "more feature content");
    repo.commit("Feature commit 2");

    // Advance main with unrelated changes (no file overlap → no conflict)
    repo.git(&["checkout", "main"]);
    repo.create_file("unrelated-a.txt", "unrelated");
    repo.commit("Main unrelated A");
    repo.create_file("unrelated-b.txt", "unrelated too");
    repo.commit("Main unrelated B");

    // Initialize stax and track
    repo.run_stax(&["set-trunk", "main"]);
    repo.git(&["checkout", "long-feature"]);

    let track_out = repo.run_stax(&["branch", "track", "--parent", "main"]);
    track_out.assert_success();

    // Verify stored revision is merge-base
    let stored_rev = read_parent_revision(&repo, "long-feature");
    let merge_base = get_merge_base(&repo, "main", "long-feature");
    assert_eq!(
        stored_rev, merge_base,
        "track must store merge-base before restack"
    );

    // Restack — must succeed because feature doesn't conflict with main's advances
    let restack_out = repo.run_stax(&["restack", "--yes", "--quiet"]);
    restack_out.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "No rebase should be in progress after clean restack"
    );

    // The stored revision should now equal main's current HEAD
    let stored_after = read_parent_revision(&repo, "long-feature");
    let main_head = repo.get_commit_sha("main");
    assert_eq!(
        stored_after, main_head,
        "After successful restack, parentBranchRevision should be updated to new parent HEAD"
    );
}
