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

use crate::common;

use common::{OutputAssertions, TestRepo};
use stax::application::{
    NoopOperationReporter, OperationErrorDetails, OperationErrorKind, OperationOutcome,
    OperationSideEffects, RepositorySession, RestackScope, TransactionStatus,
};
use std::io::Write;
use std::path::PathBuf;
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

fn write_branch_metadata_with_pr(
    repo: &TestRepo,
    branch: &str,
    parent_name: &str,
    parent_revision: &str,
    pr_state: &str,
) {
    let json = format!(
        r#"{{"parentBranchName":"{}","parentBranchRevision":"{}","prInfo":{{"number":42,"state":"{}","isDraft":false}}}}"#,
        parent_name, parent_revision, pr_state
    );

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
    assert_git_success(
        repo,
        &[
            "update-ref",
            &format!("refs/branch-metadata/{branch}"),
            &hash,
        ],
        "write metadata with PR",
    );
}

fn application_restack(
    repo: &TestRepo,
    scope: RestackScope,
) -> stax::application::OperationReceipt {
    RepositorySession::open(repo.path())
        .unwrap()
        .restack(scope, false, &mut NoopOperationReporter)
        .unwrap()
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

fn assert_git_success(repo: &TestRepo, args: &[&str], context: &str) {
    let out = repo.git(args);
    assert!(
        out.status.success(),
        "{} failed: git {}\nstdout:\n{}\nstderr:\n{}",
        context,
        args.join(" "),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn output_text(output: std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn repository_git_dir(repo: &TestRepo) -> PathBuf {
    let path = PathBuf::from(output_text(repo.git(&["rev-parse", "--git-common-dir"])));
    if path.is_absolute() {
        path
    } else {
        repo.path().join(path)
    }
}

#[cfg(unix)]
fn install_post_rewrite_hook(repo: &TestRepo, script: &str) {
    use std::os::unix::fs::PermissionsExt;

    let hook = repository_git_dir(repo).join("hooks").join("post-rewrite");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(&hook, script).unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&hook, permissions).unwrap();
}

fn local_ref_lock_path(repo: &TestRepo, branch: &str) -> PathBuf {
    repository_git_dir(repo)
        .join("refs")
        .join("heads")
        .join(format!("{branch}.lock"))
}

fn create_empty_commit(repo: &TestRepo, message: &str) {
    assert_git_success(repo, &["commit", "--allow-empty", "-m", message], message);
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
    repo.set_trunk("main");

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
    repo.set_trunk("main");

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
// Regression (#679): external rebase leaves stored boundary stale
// =============================================================================

/// Reproduces issue #679: the branch was rebased externally onto the advanced
/// parent, so the parent tip already equals the branch merge-base, but the
/// stored `parentBranchRevision` still points at the old parent tip. Restack
/// must treat the stored boundary as stale, rebase from the merge-base (a
/// no-op), leave the branch SHA untouched, and repair the metadata so no
/// further restack is required.
#[test]
fn test_restack_after_external_rebase_is_noop_and_repairs_metadata() {
    let repo = TestRepo::new();

    // Feature with 2 commits.
    repo.git(&["checkout", "-b", "count-feature"]);
    repo.create_file("f1.txt", "file one");
    repo.commit("Feature commit 1");
    repo.create_file("f2.txt", "file two");
    repo.commit("Feature commit 2");

    // Advance main by one commit after capturing the old tip.
    repo.git(&["checkout", "main"]);
    let old_main = repo.get_commit_sha("HEAD");
    repo.create_file("s.txt", "main advance");
    repo.commit("Main advance commit");

    // Externally rebase the feature onto the advanced main.
    repo.git(&["checkout", "count-feature"]);
    repo.git(&["rebase", "main"]);
    let feature_before = repo.get_commit_sha("count-feature");

    // Stored boundary is stale: still the old main tip.
    write_branch_metadata_raw(&repo, "count-feature", "main", &old_main);
    repo.set_trunk("main");

    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());

    // No replay: the branch SHA is unchanged.
    assert_eq!(
        repo.get_commit_sha("count-feature"),
        feature_before,
        "restack after an external rebase must be a no-op (no commit replay)"
    );

    // Exactly the two feature commits above main, and none behind.
    assert_eq!(rev_list_count(&repo, "main..count-feature"), 2);
    assert_eq!(rev_list_count(&repo, "count-feature..main"), 0);

    // Metadata repaired: stored parentBranchRevision now equals current main tip.
    let main_tip = repo.get_commit_sha("main");
    let meta_out = repo.git(&["cat-file", "-p", "refs/branch-metadata/count-feature"]);
    assert!(meta_out.status.success(), "failed to read branch metadata");
    let meta = String::from_utf8_lossy(&meta_out.stdout).to_string();
    assert!(
        meta.contains(&format!("\"parentBranchRevision\":\"{main_tip}\"")),
        "metadata should be repaired to the current main tip so no further restack is needed;\n\
         expected parentBranchRevision={main_tip}\nmetadata=\n{meta}"
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

    assert_git_success(
        &repo,
        &["checkout", "-b", "busy-feature"],
        "create busy-feature",
    );
    repo.create_file("feat1.txt", "feature one");
    repo.commit("Feature work 1");
    repo.create_file("feat2.txt", "feature two");
    repo.commit("Feature work 2");

    assert_git_success(&repo, &["checkout", "main"], "checkout main");
    for i in 0..TRUNK_COMMITS {
        create_empty_commit(&repo, &format!("Trunk churn commit {i}"));
    }

    let trunk_delta = rev_list_count(&repo, &format!("{old_main_tip}..main"));
    assert_eq!(
        trunk_delta, TRUNK_COMMITS,
        "sanity: main should have diverged from the stored fork SHA by TRUNK_COMMITS"
    );

    write_branch_metadata_raw(&repo, "busy-feature", "main", &old_main_tip);
    repo.set_trunk("main");

    assert_git_success(
        &repo,
        &["checkout", "busy-feature"],
        "checkout busy-feature",
    );
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

    assert_git_success(&repo, &["checkout", "-b", "sync-busy"], "create sync-busy");
    repo.create_file("sf1.txt", "x");
    repo.commit("sync feature 1");
    repo.create_file("sf2.txt", "y");
    repo.commit("sync feature 2");
    assert_git_success(
        &repo,
        &["push", "-u", "origin", "sync-busy"],
        "push sync-busy",
    );

    assert_git_success(&repo, &["checkout", "main"], "checkout main");
    for i in 0..TRUNK_COMMITS {
        create_empty_commit(&repo, &format!("main advance {i}"));
    }
    assert_git_success(&repo, &["push", "origin", "main"], "push main");

    write_branch_metadata_raw(&repo, "sync-busy", "main", &old_main_tip);
    repo.set_trunk("main");

    assert_git_success(&repo, &["checkout", "sync-busy"], "checkout sync-busy");
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
    repo.set_trunk("main");

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
// Preflight advisory: warn before rebase when stored boundary inflates the range
// =============================================================================

/// Helper: build the merge-from-main fixture used by the preflight tests.
/// Returns the configured branch name.
fn build_merge_from_main_fixture(repo: &TestRepo) -> String {
    const PRE_MERGE_TRUNK: usize = 30;
    const POST_MERGE_TRUNK: usize = 5;

    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "preflight-feature"]);
    repo.create_file("p1.txt", "p1");
    repo.commit("preflight commit 1");

    repo.git(&["checkout", "main"]);
    for i in 0..PRE_MERGE_TRUNK {
        repo.create_file(&format!("pre_pf_{i}.txt"), "x");
        repo.commit(&format!("trunk pre {i}"));
    }

    repo.git(&["checkout", "preflight-feature"]);
    repo.git(&[
        "merge",
        "main",
        "--no-edit",
        "-m",
        "Merge branch 'main' into preflight-feature",
    ]);
    repo.create_file("p2.txt", "p2");
    repo.commit("preflight commit 2");

    repo.git(&["checkout", "main"]);
    for i in 0..POST_MERGE_TRUNK {
        repo.create_file(&format!("post_pf_{i}.txt"), "y");
        repo.commit(&format!("trunk post {i}"));
    }

    write_branch_metadata_raw(repo, "preflight-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "preflight-feature"]);

    "preflight-feature".to_string()
}

/// When stored boundary drift inflates the replay range, restack should print
/// a `preflight:` notice and automatically rebase from the merge-base instead.
#[test]
fn test_restack_preflight_repairs_when_stored_range_dominates_merge_base() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let branch = build_merge_from_main_fixture(&repo);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        stdout.contains("preflight:") || stderr.contains("preflight:"),
        "expected a preflight correction notice; stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert!(
        stdout.contains("using merge-base boundary")
            || stderr.contains("using merge-base boundary"),
        "expected the notice to say stax used the merge-base boundary"
    );
    assert_eq!(
        rev_list_count(&repo, &format!("main..{branch}")),
        2,
        "automatic preflight repair should leave only the feature commits above main"
    );
}

/// `restack.preflight_warn = false` in the config must silence the advisory.
#[test]
fn test_restack_preflight_silenced_by_config() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");
    std::fs::write(
        config_dir.path().join("config.toml"),
        "[restack]\npreflight_warn = false\n",
    )
    .expect("write config");

    let branch = build_merge_from_main_fixture(&repo);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stdout.contains("preflight:") && !stderr.contains("preflight:"),
        "preflight advisory should be silenced when restack.preflight_warn=false; \
         stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert_eq!(
        rev_list_count(&repo, &format!("main..{branch}")),
        2,
        "preflight_warn=false should silence output, not disable automatic repair"
    );
}

/// `--quiet` must also silence the advisory regardless of config.
#[test]
fn test_restack_preflight_silenced_by_quiet_flag() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let branch = build_merge_from_main_fixture(&repo);

    let output = repo.run_stax_with_env(
        &["restack", "--yes", "--quiet"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stdout.contains("preflight:") && !stderr.contains("preflight:"),
        "preflight advisory should respect --quiet; stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert_eq!(
        rev_list_count(&repo, &format!("main..{branch}")),
        2,
        "--quiet should silence output, not disable automatic repair"
    );
}

/// Linear branch with stored boundary far behind but small actual divergence
/// (no merges from main) should NOT trigger the advisory because merge-base
/// matches the stored boundary's effective replay set.
#[test]
fn test_restack_preflight_silent_on_clean_linear_branch() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "linear-quiet"]);
    repo.create_file("lq1.txt", "x");
    repo.commit("linear 1");
    repo.create_file("lq2.txt", "y");
    repo.commit("linear 2");

    repo.git(&["checkout", "main"]);
    for i in 0..40 {
        repo.create_file(&format!("lq_trunk_{i}.txt"), "t");
        repo.commit(&format!("linear trunk {i}"));
    }

    write_branch_metadata_raw(&repo, "linear-quiet", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "linear-quiet"]);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stdout.contains("preflight:") && !stderr.contains("preflight:"),
        "linear branch should not trigger preflight advisory; \
         stdout=\n{stdout}\nstderr=\n{stderr}"
    );
}

// =============================================================================
// Preflight: leading commits already squash-merged into the parent
// =============================================================================

/// Build a branch whose first two commits both edit `shared.txt`, squash those
/// two commits into `main`, then advance `main` again. The branch keeps a third,
/// still-unmerged commit. Returns the branch name.
fn build_squash_merged_prefix_fixture(repo: &TestRepo) -> String {
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "squashed-feature"]);
    repo.create_file("shared.txt", "v1\n");
    repo.commit("feature commit 1");
    repo.create_file("shared.txt", "v2\n");
    repo.commit("feature commit 2");
    repo.create_file("extra.txt", "still unmerged\n");
    repo.commit("feature commit 3");

    // GitHub-style squash merge of the first two commits.
    repo.git(&["checkout", "main"]);
    assert_git_success(
        repo,
        &["merge", "--squash", "squashed-feature~1"],
        "squash merge prefix",
    );
    repo.commit("Squash merge of feature commits 1 and 2 (#1)");
    repo.create_file("unrelated.txt", "trunk moved on\n");
    repo.commit("unrelated trunk commit");

    write_branch_metadata_raw(repo, "squashed-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "squashed-feature"]);

    "squashed-feature".to_string()
}

/// The first N commits of a branch were squash-merged: restack must rebase only
/// the commits after them instead of replaying the merged ones and conflicting.
#[test]
fn test_restack_preflight_skips_squash_merged_prefix() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let branch = build_squash_merged_prefix_fixture(&repo);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        stdout.contains("already merged into 'main'")
            || stderr.contains("already merged into 'main'"),
        "expected a preflight notice about the squash-merged commits; \
         stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert_eq!(
        rev_list_count(&repo, &format!("main..{branch}")),
        1,
        "only the unmerged third commit should remain above main"
    );
    // Nothing may be lost or invented: relative to main, the branch differs only by
    // its own unmerged commit.
    let changed = output_text(repo.git(&["diff", "--name-only", "main", &branch]));
    assert_eq!(changed, "extra.txt");
    let shared = output_text(repo.git(&["show", &format!("{branch}:shared.txt")]));
    assert_eq!(shared, "v2", "squashed content from main must be kept");
    let extra = output_text(repo.git(&["show", &format!("{branch}:extra.txt")]));
    assert_eq!(extra, "still unmerged", "unmerged commit must be preserved");
}

/// With `preflight_auto_repair = false` the same fixture conflicts, which proves
/// the squash detection (not the fixture) is what makes the restack succeed.
#[test]
fn test_restack_squash_merged_prefix_conflicts_when_auto_repair_disabled() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");
    std::fs::write(
        config_dir.path().join("config.toml"),
        "[restack]\npreflight_auto_repair = false\n",
    )
    .expect("write config");

    build_squash_merged_prefix_fixture(&repo);

    let output = repo.run_stax_with_env(
        &["restack", "--yes", "--quiet"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_failure();
    assert!(
        repo.has_rebase_in_progress(),
        "replaying squash-merged commits should conflict without the repair"
    );

    repo.abort_rebase();
}

/// Trunk commits that are not a squash of the branch's commits must not make
/// stax skip anything: every unmerged commit is still replayed.
#[test]
fn test_restack_preflight_does_not_skip_unmatched_commits() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let fork_point = repo.get_commit_sha("HEAD");
    repo.git(&["checkout", "-b", "unmatched-feature"]);
    repo.create_file("feat1.txt", "one\n");
    repo.commit("unmatched 1");
    repo.create_file("feat2.txt", "two\n");
    repo.commit("unmatched 2");

    repo.git(&["checkout", "main"]);
    repo.create_file("other.txt", "trunk\n");
    repo.commit("unrelated trunk commit");

    write_branch_metadata_raw(&repo, "unmatched-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "unmatched-feature"]);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stdout.contains("already merged") && !stderr.contains("already merged"),
        "no commits are merged, so no squash notice is expected; \
         stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    assert_eq!(
        rev_list_count(&repo, "main..unmatched-feature"),
        2,
        "both unmerged commits must be replayed"
    );
}

/// Same text added at a *different place* in the same file has the same -U0
/// patch-id as the branch commit, but the change is not actually on trunk. The
/// branch commit must be kept (and here it must conflict or replay, never vanish).
#[test]
fn test_restack_preflight_does_not_skip_same_text_at_different_place() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    repo.create_file("list.txt", "a\nb\nc\nd\ne\nf\n");
    repo.commit("add list");
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "lookalike-feature"]);
    repo.create_file("list.txt", "a\nb\nc\nNEW\nd\ne\nf\n");
    repo.commit("add NEW after c");
    repo.create_file("tail.txt", "tail\n");
    repo.commit("add tail");

    // Trunk adds the identical line, but after e instead of after c.
    repo.git(&["checkout", "main"]);
    repo.create_file("list.txt", "a\nb\nc\nd\ne\nNEW\nf\n");
    repo.commit("add NEW after e");

    write_branch_metadata_raw(&repo, "lookalike-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "lookalike-feature"]);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stdout.contains("already merged") && !stderr.contains("already merged"),
        "a look-alike change must not be treated as merged; stdout=\n{stdout}\nstderr=\n{stderr}"
    );
    if repo.has_rebase_in_progress() {
        repo.abort_rebase();
    } else {
        assert_eq!(
            rev_list_count(&repo, "main..lookalike-feature"),
            2,
            "both branch commits must still be replayed"
        );
    }
}

/// Later trunk edits to the squashed file (on other lines) must not defeat the
/// detection: the 3-way merge is still a no-op for the squashed changes.
#[test]
fn test_restack_preflight_skips_squash_when_trunk_edited_same_file_elsewhere() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    repo.create_file("doc.txt", "1\n2\n3\n4\n5\n6\n7\n8\n9\n");
    repo.commit("add doc");
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "doc-feature"]);
    repo.create_file("doc.txt", "1\nTWO\n3\n4\n5\n6\n7\n8\n9\n");
    repo.commit("edit line 2");
    repo.create_file("doc.txt", "1\nTWO\n3\n4\n5\n6\n7\n8\nNINE\n");
    repo.commit("edit line 9");
    repo.create_file("more.txt", "kept\n");
    repo.commit("unmerged follow-up");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "doc-feature~1"], "squash");
    repo.commit("Squash merge (#2)");
    repo.create_file("doc.txt", "1\nTWO\n3\n4\nFIVE\n6\n7\n8\nNINE\n");
    repo.commit("trunk edits line 5");

    write_branch_metadata_raw(&repo, "doc-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "doc-feature"]);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();
    assert_eq!(rev_list_count(&repo, "main..doc-feature"), 1);
    let doc = output_text(repo.git(&["show", "doc-feature:doc.txt"]));
    assert_eq!(doc, "1\nTWO\n3\n4\nFIVE\n6\n7\n8\nNINE");
}

/// A branch whose every commit was squash-merged restacks onto main cleanly
/// instead of conflicting.
#[test]
fn test_restack_preflight_handles_fully_squash_merged_branch() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let fork_point = repo.get_commit_sha("HEAD");
    repo.git(&["checkout", "-b", "all-merged"]);
    repo.create_file("m.txt", "v1\n");
    repo.commit("m 1");
    repo.create_file("m.txt", "v2\n");
    repo.commit("m 2");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "all-merged"], "squash");
    repo.commit("Squash merge all (#3)");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "all-merged", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "all-merged"]);

    let output = repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());
    assert_eq!(rev_list_count(&repo, "main..all-merged"), 0);
}

/// A child stacked on a branch with a squash-merged prefix must keep only its own
/// commits: the child's boundary is the parent's old tip, not the squashed prefix.
#[test]
fn test_restack_stack_child_of_squash_merged_branch_keeps_only_own_commits() {
    let repo = TestRepo::new();
    let config_dir = tempfile::TempDir::new().expect("create config dir");

    let fork_point = repo.get_commit_sha("HEAD");
    repo.git(&["checkout", "-b", "stack-parent"]);
    repo.create_file("s.txt", "v1\n");
    repo.commit("parent 1");
    repo.create_file("s.txt", "v2\n");
    repo.commit("parent 2");
    repo.create_file("p3.txt", "p3\n");
    repo.commit("parent 3");
    let parent_tip = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "stack-child"]);
    repo.create_file("child.txt", "child\n");
    repo.commit("child 1");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "stack-parent~1"], "squash");
    repo.commit("Squash merge parent 1+2 (#4)");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "stack-parent", "main", &fork_point);
    write_branch_metadata_raw(&repo, "stack-child", "stack-parent", &parent_tip);
    repo.set_trunk("main");
    repo.git(&["checkout", "stack-child"]);

    let output = repo.run_stax_with_env(
        &["restack", "--all", "--yes", "--quiet"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());
    assert_eq!(rev_list_count(&repo, "main..stack-parent"), 1);
    assert_eq!(
        rev_list_count(&repo, "stack-parent..stack-child"),
        1,
        "child must replay only its own commit"
    );
}

/// Run `stax restack --yes` with an isolated config dir.
fn restack_isolated(repo: &TestRepo) -> std::process::Output {
    let config_dir = tempfile::TempDir::new().expect("create config dir");
    repo.run_stax_with_env(
        &["restack", "--yes"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    )
}

fn assert_no_merged_notice(output: &std::process::Output) {
    let stdout = TestRepo::stdout(output);
    let stderr = TestRepo::stderr(output);
    assert!(
        !stdout.contains("already merged") && !stderr.contains("already merged"),
        "must not treat commits as merged; stdout=\n{stdout}\nstderr=\n{stderr}"
    );
}

/// The squash was reverted on trunk, so the squashed changes are NOT on main any
/// more. Skipping the branch's commits would silently drop the user's work.
#[test]
fn test_restack_preflight_does_not_skip_commits_whose_squash_was_reverted() {
    let repo = TestRepo::new();
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "reverted-feature"]);
    repo.create_file("r.txt", "v1\n");
    repo.commit("r 1");
    repo.create_file("r.txt", "v2\n");
    repo.commit("r 2");
    repo.create_file("keep.txt", "keep\n");
    repo.commit("r 3");

    repo.git(&["checkout", "main"]);
    assert_git_success(
        &repo,
        &["merge", "--squash", "reverted-feature~1"],
        "squash",
    );
    repo.commit("Squash merge (#5)");
    assert_git_success(&repo, &["revert", "--no-edit", "HEAD"], "revert squash");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "reverted-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "reverted-feature"]);

    let output = restack_isolated(&repo);
    assert_no_merged_notice(&output);
    output.assert_success();
    assert_eq!(
        rev_list_count(&repo, "main..reverted-feature"),
        3,
        "all three commits must survive because the squash is no longer on main"
    );
    assert_eq!(
        output_text(repo.git(&["show", "reverted-feature:r.txt"])),
        "v2"
    );
}

/// Binary changes to the same path by different bytes are not the same change.
#[test]
fn test_restack_preflight_does_not_skip_different_binary_change_to_same_path() {
    let repo = TestRepo::new();
    std::fs::write(repo.path().join("b.bin"), [0u8, 1, 2, 3, 0, 5]).unwrap();
    repo.commit("add binary");
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "binary-feature"]);
    std::fs::write(repo.path().join("b.bin"), [0u8, 9, 9, 9, 0, 5]).unwrap();
    repo.commit("binary change A");
    repo.create_file("after.txt", "after\n");
    repo.commit("after binary");

    repo.git(&["checkout", "main"]);
    std::fs::write(repo.path().join("b.bin"), [0u8, 7, 7, 7, 0, 5]).unwrap();
    repo.commit("binary change B on trunk");

    write_branch_metadata_raw(&repo, "binary-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "binary-feature"]);
    let before = repo.get_commit_sha("binary-feature");

    let output = restack_isolated(&repo);
    assert_no_merged_notice(&output);
    if repo.has_rebase_in_progress() {
        repo.abort_rebase();
        assert_eq!(repo.get_commit_sha("binary-feature"), before);
    } else {
        assert_eq!(rev_list_count(&repo, "main..binary-feature"), 2);
    }
}

/// The squash commit contains more than the branch's commits (edited at merge
/// time), so it is not a match: behave exactly as before, losing nothing.
#[test]
fn test_restack_preflight_does_not_skip_when_squash_contains_extra_changes() {
    let repo = TestRepo::new();
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "extra-feature"]);
    repo.create_file("e.txt", "v1\n");
    repo.commit("e 1");
    repo.create_file("e.txt", "v2\n");
    repo.commit("e 2");
    repo.create_file("tail.txt", "tail\n");
    repo.commit("e 3");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "extra-feature~1"], "squash");
    repo.create_file("sneaky.txt", "added while merging\n");
    repo.commit("Squash merge plus extra (#6)");

    write_branch_metadata_raw(&repo, "extra-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "extra-feature"]);
    let before = repo.get_commit_sha("extra-feature");

    let output = restack_isolated(&repo);
    assert_no_merged_notice(&output);
    if repo.has_rebase_in_progress() {
        repo.abort_rebase();
        assert_eq!(repo.get_commit_sha("extra-feature"), before);
    }
}

/// A single commit cherry-picked onto trunk is dropped; the rest is kept.
#[test]
fn test_restack_preflight_handles_cherry_picked_first_commit() {
    let repo = TestRepo::new();
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "picked-feature"]);
    repo.create_file("c.txt", "c1\n");
    repo.commit("pick me");
    let picked = repo.get_commit_sha("HEAD");
    repo.create_file("c2.txt", "c2\n");
    repo.commit("keep me");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["cherry-pick", &picked], "cherry-pick");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "picked-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "picked-feature"]);

    restack_isolated(&repo).assert_success();
    assert!(!repo.has_rebase_in_progress());
    assert_eq!(rev_list_count(&repo, "main..picked-feature"), 1);
    assert_eq!(
        output_text(repo.git(&["diff", "--name-only", "main", "picked-feature"])),
        "c2.txt"
    );
}

/// A stored boundary that is not in the branch's ancestry (metadata drift) must
/// not stop the squash detection from working, and must not lose commits.
#[test]
fn test_restack_preflight_squash_with_non_ancestor_stored_revision() {
    let repo = TestRepo::new();

    repo.git(&["checkout", "-b", "side"]);
    repo.create_file("side.txt", "side\n");
    repo.commit("side commit");
    let unrelated_sha = repo.get_commit_sha("HEAD");
    repo.git(&["checkout", "main"]);

    repo.git(&["checkout", "-b", "drift-feature"]);
    repo.create_file("d.txt", "v1\n");
    repo.commit("d 1");
    repo.create_file("d.txt", "v2\n");
    repo.commit("d 2");
    repo.create_file("keep.txt", "keep\n");
    repo.commit("d 3");

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "drift-feature~1"], "squash");
    repo.commit("Squash merge (#7)");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "drift-feature", "main", &unrelated_sha);
    repo.set_trunk("main");
    repo.git(&["checkout", "drift-feature"]);

    restack_isolated(&repo).assert_success();
    assert!(!repo.has_rebase_in_progress());
    assert_eq!(
        output_text(repo.git(&["diff", "--name-only", "main", "drift-feature"])),
        "keep.txt"
    );
}

/// Commits that cancel each other out (empty cumulative diff) must not confuse
/// the search or lose anything.
#[test]
fn test_restack_preflight_handles_empty_cumulative_diff() {
    let repo = TestRepo::new();
    let fork_point = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "cancel-feature"]);
    repo.create_file("x.txt", "x\n");
    repo.commit("add x");
    assert_git_success(&repo, &["rm", "-q", "x.txt"], "remove x");
    repo.commit("remove x");
    repo.create_file("real.txt", "real\n");
    repo.commit("real change");

    repo.git(&["checkout", "main"]);
    repo.create_file("real.txt", "different\n");
    repo.commit("trunk touches the same path");
    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["reset", "--hard", &fork_point], "reset trunk");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "cancel-feature", "main", &fork_point);
    repo.set_trunk("main");
    repo.git(&["checkout", "cancel-feature"]);

    let output = restack_isolated(&repo);
    assert_no_merged_notice(&output);
    output.assert_success();
    assert_eq!(rev_list_count(&repo, "main..cancel-feature"), 3);
}

/// The parent was amended after the child was created, so the child still holds the
/// parent's OLD last commit and git cannot recognise it as already applied. The child
/// must be rebased from its stored boundary (the old parent tip), not from the
/// squash-merged prefix, or the stale commit would be replayed and conflict.
#[test]
fn test_restack_stack_child_of_amended_squash_merged_parent() {
    let repo = TestRepo::new();

    let fork_point = repo.get_commit_sha("HEAD");
    repo.git(&["checkout", "-b", "amended-parent"]);
    repo.create_file("s.txt", "v1\n");
    repo.commit("parent 1");
    repo.create_file("s.txt", "v2\n");
    repo.commit("parent 2");
    repo.create_file("p3.txt", "p3\n");
    repo.commit("parent 3");
    let old_parent_tip = repo.get_commit_sha("HEAD");

    repo.git(&["checkout", "-b", "amended-child"]);
    repo.create_file("child.txt", "child\n");
    repo.commit("child 1");

    repo.git(&["checkout", "amended-parent"]);
    repo.create_file("p3.txt", "p3 amended\n");
    assert_git_success(&repo, &["add", "-A"], "stage amend");
    assert_git_success(
        &repo,
        &["commit", "--amend", "--no-edit", "-q"],
        "amend parent",
    );

    repo.git(&["checkout", "main"]);
    assert_git_success(&repo, &["merge", "--squash", "amended-parent~1"], "squash");
    repo.commit("Squash merge parent 1+2 (#8)");
    repo.create_file("u.txt", "u\n");
    repo.commit("unrelated");

    write_branch_metadata_raw(&repo, "amended-parent", "main", &fork_point);
    write_branch_metadata_raw(&repo, "amended-child", "amended-parent", &old_parent_tip);
    repo.set_trunk("main");
    repo.git(&["checkout", "amended-child"]);

    let config_dir = tempfile::TempDir::new().expect("create config dir");
    let output = repo.run_stax_with_env(
        &["restack", "--all", "--yes", "--quiet"],
        &[("STAX_CONFIG_DIR", config_dir.path().to_str().unwrap())],
    );
    output.assert_success();
    assert!(!repo.has_rebase_in_progress());
    assert_eq!(rev_list_count(&repo, "main..amended-parent"), 1);
    assert_eq!(rev_list_count(&repo, "amended-parent..amended-child"), 1);
    assert_eq!(
        output_text(repo.git(&["show", "amended-child:p3.txt"])),
        "p3 amended",
        "the child must keep the amended parent content"
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
    repo.set_trunk("main");

    repo.git(&["checkout", "conflict-feature"]);

    let output = repo.run_stax(&["restack", "--yes", "--quiet"]);
    output.assert_failure();
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should be in progress after a genuine conflict"
    );

    repo.abort_rebase();
}

#[test]
fn application_restack_recomputes_child_after_parent_rebase() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["app-parent", "app-child"]);
    let parent_before = repo.get_commit_sha(&branches[0]);
    let child_before = repo.get_commit_sha(&branches[1]);

    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-main-advance.txt", "main moved\n");
    repo.commit("Advance main for application restack");
    repo.git(&["checkout", &branches[1]]).assert_success();

    let receipt = application_restack(
        &repo,
        RestackScope::StackContaining(branches[1].to_string()),
    );

    assert_ne!(repo.get_commit_sha(&branches[0]), parent_before);
    assert_ne!(repo.get_commit_sha(&branches[1]), child_before);
    assert_eq!(
        receipt.side_effects,
        OperationSideEffects::RepositoryChanged
    );
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. }
            if branches == &vec![branches[0].clone(), branches[1].clone()]
    ));
}

#[test]
fn application_restack_preserves_open_pr_fast_path() {
    let repo = TestRepo::new();
    let branch = repo.create_stack(&["app-open-pr"]).remove(0);
    let original_parent = repo.get_commit_sha("main");
    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-open-pr-main.txt", "main moved\n");
    repo.commit("Advance main below open PR");
    write_branch_metadata_with_pr(&repo, &branch, "main", &original_parent, "OPEN");
    repo.git(&["checkout", &branch]).assert_success();

    let receipt = application_restack(&repo, RestackScope::Branch(branch.clone()));

    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. } if branches == &vec![branch]
    ));
}

#[test]
fn application_restack_skips_frozen_and_reports_it() {
    let repo = TestRepo::new();
    let branch = repo.create_stack(&["app-frozen"]).remove(0);
    let branch_before = repo.get_commit_sha(&branch);
    repo.run_stax(&["freeze", &branch]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-frozen-main.txt", "main moved\n");
    repo.commit("Advance main below frozen branch");
    repo.git(&["checkout", &branch]).assert_success();

    let receipt = application_restack(&repo, RestackScope::Branch(branch.clone()));

    assert_eq!(repo.get_commit_sha(&branch), branch_before);
    assert_eq!(receipt.side_effects, OperationSideEffects::None);
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked {
            ref branches,
            ref skipped_frozen,
        } if branches.is_empty() && skipped_frozen == &vec![branch]
    ));
}

#[test]
fn application_stack_containing_excludes_unrelated_sibling_subtree() {
    let repo = TestRepo::new();
    let base = repo.create_stack(&["app-fork-base"]).remove(0);
    let selected = repo.create_stack(&["app-selected"]).remove(0);
    repo.run_stax(&["checkout", &base]).assert_success();
    let sibling = repo.create_stack(&["app-sibling"]).remove(0);
    let selected_before = repo.get_commit_sha(&selected);
    let sibling_before = repo.get_commit_sha(&sibling);
    repo.run_stax(&["checkout", &base]).assert_success();
    repo.create_file("app-fork-base-v2.txt", "base moved\n");
    repo.commit("Advance fork base");
    repo.run_stax(&["checkout", &selected]).assert_success();

    application_restack(&repo, RestackScope::StackContaining(selected.clone()));

    assert_ne!(repo.get_commit_sha(&selected), selected_before);
    assert_eq!(repo.get_commit_sha(&sibling), sibling_before);
}

#[test]
fn application_restack_noop_has_no_transaction() {
    let repo = TestRepo::new();
    let branch = repo.create_stack(&["app-noop"]).remove(0);

    let receipt = application_restack(&repo, RestackScope::Branch(branch.clone()));

    assert_eq!(receipt.transaction, None);
    assert_eq!(receipt.side_effects, OperationSideEffects::None);
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. } if branches.is_empty()
    ));
}

#[cfg(unix)]
#[test]
fn restack_ref_lock_failure_after_first_branch_uses_failed_finalizer() {
    let repo = TestRepo::new();
    let branches =
        repo.create_stack(&["app-failed-finalizer-first", "app-failed-finalizer-second"]);
    let first = branches[0].clone();
    let second = branches[1].clone();
    let first_before = repo.get_commit_sha(&first);
    let second_before = repo.get_commit_sha(&second);
    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-failed-finalizer-main.txt", "main moved\n");
    repo.commit("Advance main before ref lock failure");
    let lock_path = local_ref_lock_path(&repo, &second);
    install_post_rewrite_hook(
        &repo,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"rebase\" ]; then : > '{}'; fi\n",
            lock_path.display()
        ),
    );
    repo.git(&["checkout", &second]).assert_success();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::StackContaining(second.clone()),
            false,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::LocalGit);
    assert_eq!(error.side_effects, OperationSideEffects::RepositoryChanged);
    assert_ne!(repo.get_commit_sha(&first), first_before);
    assert_eq!(repo.get_commit_sha(&second), second_before);
    let receipt = error.receipt.expect("failed in-memory receipt");
    let transaction = receipt.transaction.expect("transaction summary");
    assert_eq!(transaction.status, TransactionStatus::Failed);
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. } if branches == &vec![first]
    ));
}

#[cfg(unix)]
#[test]
fn restack_receipt_persistence_failure_retains_in_memory_failed_receipt() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&[
        "app-failed-receipt-persist-first",
        "app-failed-receipt-persist-second",
    ]);
    let first = branches[0].clone();
    let second = branches[1].clone();
    repo.git(&["checkout", "main"]).assert_success();
    repo.create_file("app-failed-receipt-persist-main.txt", "main moved\n");
    repo.commit("Advance main before failed receipt persistence");
    let ops_dir = repository_git_dir(&repo).join("stax").join("ops");
    std::fs::create_dir_all(&ops_dir).unwrap();
    let lock_path = local_ref_lock_path(&repo, &second);
    install_post_rewrite_hook(
        &repo,
        &format!(
            "#!/bin/sh\nif [ \"$1\" = \"rebase\" ]; then for receipt in '{}'/*.json; do rm -f \"$receipt\"; mkdir \"$receipt\"; done; : > '{}'; fi\n",
            ops_dir.display(),
            lock_path.display()
        ),
    );
    repo.git(&["checkout", &second]).assert_success();

    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(
            RestackScope::StackContaining(second.clone()),
            false,
            &mut NoopOperationReporter,
        )
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::LocalGit);
    assert!(error.primary.contains(&second));
    assert!(
        error.diagnostic_chain.contains("Failed to write receipt"),
        "diagnostics should retain receipt persistence failure: {}",
        error.diagnostic_chain
    );
    let receipt = error.receipt.expect("failed in-memory receipt");
    let transaction = receipt.transaction.expect("transaction summary");
    assert_eq!(transaction.status, TransactionStatus::Failed);
    assert!(matches!(
        receipt.outcome,
        OperationOutcome::Restacked { ref branches, .. } if branches == &vec![first]
    ));
}

#[test]
fn application_restack_preflights_linked_target_before_stash() {
    let repo = TestRepo::new();
    let branch = repo.create_stack(&["app-linked-preflight"]).remove(0);
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked");
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &branch])
        .assert_success();
    std::fs::write(linked.join("dirty.txt"), "dirty\n").unwrap();
    let linked_git_dir = PathBuf::from(
        String::from_utf8_lossy(&repo.git_in(&linked, &["rev-parse", "--git-dir"]).stdout)
            .trim()
            .to_string(),
    );
    let linked_git_dir = if linked_git_dir.is_absolute() {
        linked_git_dir
    } else {
        linked.join(linked_git_dir)
    };
    std::fs::create_dir_all(linked_git_dir.join("rebase-merge")).unwrap();

    let mut reporter = NoopOperationReporter;
    let error = RepositorySession::open(repo.path())
        .unwrap()
        .restack(RestackScope::Branch(branch.clone()), true, &mut reporter)
        .unwrap_err();

    assert_eq!(error.kind, OperationErrorKind::RebaseInProgress);
    assert_eq!(
        error.details,
        OperationErrorDetails::Rebase {
            branch: Some(branch),
            worktree: linked.canonicalize().unwrap(),
        }
    );
    let stash_list = repo.git_in(&linked, &["stash", "list"]);
    assert!(String::from_utf8_lossy(&stash_list.stdout).is_empty());
    assert!(linked.join("dirty.txt").exists());
}
