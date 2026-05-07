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

fn write_branch_metadata_raw(
    repo: &TestRepo,
    branch: &str,
    parent_name: &str,
    parent_revision: &str,
) {
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

fn rev_list_count(repo: &TestRepo, range: &str) -> usize {
    let out = repo.git(&["rev-list", "--count", range]);
    assert!(
        out.status.success(),
        "git rev-list --count {} failed: {}",
        range,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .unwrap_or(0)
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
    let feature_only: Vec<&str> = log_str.lines().filter(|l| !l.trim().is_empty()).collect();

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
// Monorepo-style trunk churn: stale stored parent tip + linear feature branch
// =============================================================================

/// Regression guard: after many commits land on `main`, stored `parentBranchRevision`
/// may still point at the **old** trunk tip. Restack must replay only the feature
/// commits (`stored_tip..feature`), not hundreds of trunk commits.
///
/// If this fails, investigate metadata/restack provenance or accidental plain
/// `git rebase` fallback — not "main moved fast" by itself.
#[test]
fn test_many_trunk_commits_linear_restack_only_replays_feature_commits() {
    // Enough commits to mimic a busy trunk; raise locally if stress-testing.
    const TRUNK_COMMITS: usize = 60;

    let repo = TestRepo::new();

    // Snapshot trunk tip at fork — this simulates `parentBranchRevision` left at the
    // last-known parent tip while engineers landed TRUNK_COMMITS on main.
    let old_main_tip = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "busy-feature"]);
    repo.create_file("feat1.txt", "feature one");
    repo.commit("Feature work 1");
    repo.create_file("feat2.txt", "feature two");
    repo.commit("Feature work 2");

    repo.git(&["checkout", "main"]);
    for i in 0..TRUNK_COMMITS {
        repo.create_file(&format!("trunk_{i}.txt"), &i.to_string());
        repo.commit(&format!("Trunk churn commit {i}"));
    }

    let trunk_delta = rev_list_count(&repo, &format!("{old_main_tip}..main"));
    assert_eq!(
        trunk_delta, TRUNK_COMMITS,
        "sanity: main should have diverged from the stored fork SHA by TRUNK_COMMITS"
    );

    write_branch_metadata_raw(&repo, "busy-feature", "main", &old_main_tip);
    repo.run_stax(&["set-trunk", "main"]);

    repo.git(&["checkout", "busy-feature"]);
    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "rebase should finish cleanly after provenance restack"
    );

    let ahead = rev_list_count(&repo, "main..busy-feature");
    assert_eq!(
        ahead, 2,
        "linear branch must stay exactly 2 commits ahead of main after restack; \
         trunk churn must not appear as extra commits on the feature branch"
    );
}

/// Same scenario as `test_many_trunk_commits_linear_restack_only_replays_feature_commits`,
/// but through `stax sync --restack` (`st rs --restack`) after pushing trunk and feature.
#[test]
fn test_sync_restack_many_trunk_commits_preserves_linear_feature_depth() {
    const TRUNK_COMMITS: usize = 32;

    let repo = TestRepo::new_with_remote();

    let old_main_tip = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "sync-busy"]);
    repo.create_file("sf1.txt", "x");
    repo.commit("sync feature 1");
    repo.create_file("sf2.txt", "y");
    repo.commit("sync feature 2");
    repo.git(&["push", "-u", "origin", "sync-busy"]);

    repo.git(&["checkout", "main"]);
    for i in 0..TRUNK_COMMITS {
        repo.create_file(&format!("remote_trunk_{i}.txt"), "bulk");
        repo.commit(&format!("main advance {i}"));
    }
    repo.git(&["push", "origin", "main"]);

    write_branch_metadata_raw(&repo, "sync-busy", "main", &old_main_tip);
    repo.run_stax(&["set-trunk", "main"]);

    repo.git(&["checkout", "sync-busy"]);
    let output = repo.run_stax(&["sync", "--restack", "--force", "--quiet", "--no-delete"]);
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());

    let ahead = rev_list_count(&repo, "main..sync-busy");
    assert_eq!(
        ahead, 2,
        "sync --restack must leave exactly two feature commits above updated main"
    );
}

// =============================================================================
// Documentation: merging `main` into a feature branch poisons the replay range
// =============================================================================

/// Documents the failure mode where `git merge main` was performed on a feature
/// branch (instead of restack) and `parentBranchRevision` still points at the
/// pre-merge fork tip. Restack will replay every trunk commit pulled in via the
/// merge, even on files the developer never touched on this branch — exactly
/// the “conflicts on files I didn’t touch” experience.
///
/// This test does not assert STAX correctness; it pins the **shape** of the
/// problem so a pre-flight sanity check has a fixture to compare against.
#[test]
fn test_merging_main_into_feature_inflates_stored_replay_range() {
    const PRE_MERGE_TRUNK: usize = 25;
    const POST_MERGE_TRUNK: usize = 10;

    let repo = TestRepo::new();
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "merged-feature"]);
    repo.create_file("ff1.txt", "feat 1");
    repo.commit("feature commit 1");

    repo.git(&["checkout", "main"]);
    for i in 0..PRE_MERGE_TRUNK {
        repo.create_file(&format!("pre_{i}.txt"), "pre");
        repo.commit(&format!("trunk pre-merge {i}"));
    }

    // The antipattern: merge `main` into the feature branch instead of rebasing.
    repo.git(&["checkout", "merged-feature"]);
    repo.git(&[
        "merge",
        "main",
        "--no-edit",
        "-m",
        "Merge branch 'main' into merged-feature",
    ]);

    repo.create_file("ff2.txt", "feat 2");
    repo.commit("feature commit 2");

    repo.git(&["checkout", "main"]);
    for i in 0..POST_MERGE_TRUNK {
        repo.create_file(&format!("post_{i}.txt"), "post");
        repo.commit(&format!("trunk post-merge {i}"));
    }

    // Stored boundary stayed at the original fork tip — the canonical mistake.
    write_branch_metadata_raw(&repo, "merged-feature", "main", &fork_point);
    repo.run_stax(&["set-trunk", "main"]);

    let stored_to_feature = rev_list_count(&repo, &format!("{fork_point}..merged-feature"));
    let merge_base_out = repo.git(&["merge-base", "main", "merged-feature"]);
    let merge_base = String::from_utf8_lossy(&merge_base_out.stdout)
        .trim()
        .to_string();
    let merge_base_to_feature = rev_list_count(&repo, &format!("{merge_base}..merged-feature"));

    // The stored range balloons because the merge dragged trunk commits into the
    // branch's reachable history; the merge-base range is what the user expects.
    assert!(
        stored_to_feature >= PRE_MERGE_TRUNK + 2,
        "stored..feature should include the trunk commits brought in via merge: \
         got {stored_to_feature}, expected at least {}",
        PRE_MERGE_TRUNK + 2
    );
    assert!(
        merge_base_to_feature < stored_to_feature,
        "merge-base range ({merge_base_to_feature}) should be strictly smaller than \
         stored-boundary range ({stored_to_feature}); without that delta there is \
         nothing for a pre-flight sanity check to detect"
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
