use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Merge a branch into the current branch using git (fast-forward).
fn merge_branch(repo: &TestRepo, branch: &str) {
    let out = repo.git(&[
        "merge",
        "--no-ff",
        branch,
        "-m",
        &format!("Merge {}", branch),
    ]);
    assert!(
        out.status.success(),
        "Failed to merge branch {}: {}",
        branch,
        TestRepo::stderr(&out)
    );
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, parent_branch: &str, pr_number: u64) {
    write_branch_pr_metadata_with_state(repo, branch, parent_branch, pr_number, "OPEN");
}

fn write_branch_pr_metadata_with_state(
    repo: &TestRepo,
    branch: &str,
    parent_branch: &str,
    pr_number: u64,
    state: &str,
) {
    let metadata = serde_json::json!({
        "parentBranchName": parent_branch,
        "parentBranchRevision": repo.get_commit_sha(parent_branch),
        "prInfo": {
            "number": pr_number,
            "state": state
        }
    });

    let mut child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to hash metadata blob");
    child
        .stdin
        .as_mut()
        .expect("metadata hash stdin")
        .write_all(metadata.to_string().as_bytes())
        .expect("Failed to write metadata JSON");
    let output = child.wait_with_output().expect("Failed to hash metadata");
    assert!(
        output.status.success(),
        "git hash-object failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let blob_hash = String::from_utf8(output.stdout)
        .expect("metadata hash UTF-8")
        .trim()
        .to_string();

    let update_ref = repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{}", branch),
        &blob_hash,
    ]);
    assert!(
        update_ref.status.success(),
        "git update-ref failed: {}",
        TestRepo::stderr(&update_ref)
    );
}

// ---------------------------------------------------------------------------
// Phase 1 — read-only listing
// ---------------------------------------------------------------------------

#[test]
fn sweep_exits_zero_on_empty_branch_set() {
    // Just trunk + current (same branch) — nothing to classify.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
}

#[test]
fn sweep_classifies_merged_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create a branch, add a commit, merge it into main
    repo.run_stax(&["bc", "feature-a"]).assert_success();
    repo.create_file("a.txt", "hello");
    repo.commit("add a");

    // Go back to main and merge
    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "feature-a");

    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("merged"),
        "Expected 'merged' bucket in output:\n{}",
        stdout
    );
    assert!(
        stdout.contains("feature-a"),
        "Expected 'feature-a' in merged output:\n{}",
        stdout
    );
}

#[test]
fn sweep_does_not_list_trunk_or_current() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create and immediately go back — current = main
    repo.run_stax(&["bc", "side"]).assert_success();
    repo.run_stax(&["t"]).assert_success();

    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    // main (trunk) should never appear as a candidate
    let lines_with_main: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("main") && (l.contains("✓") || l.contains("⚑") || l.contains("○")))
        .collect();
    assert!(
        lines_with_main.is_empty(),
        "trunk 'main' should not appear as a deletable branch:\n{}",
        stdout
    );
}

#[test]
fn sweep_does_not_list_current_merged_branch_as_deletable() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "current-merged"]).assert_success();
    repo.create_file("current.txt", "current");
    repo.commit("current branch commit");

    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "current-merged");
    repo.git(&["checkout", "current-merged"]);

    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    let candidate_lines: Vec<&str> = stdout
        .lines()
        .filter(|l| {
            l.contains("current-merged") && (l.contains("✓") || l.contains("⚑") || l.contains("○"))
        })
        .collect();
    assert!(
        candidate_lines.is_empty(),
        "current branch must not appear as a sweep candidate:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("delete 1 merged/gone branch"),
        "summary tip must not count the current branch as deletable:\n{}",
        stdout
    );
}

#[test]
fn sweep_json_does_not_list_current_merged_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "current-json-merged"])
        .assert_success();
    repo.create_file("current-json.txt", "current");
    repo.commit("current json branch commit");

    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "current-json-merged");
    repo.git(&["checkout", "current-json-merged"]);

    let out = repo.run_stax(&["sweep", "--json"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("sweep --json should output valid JSON: {}\n{}", e, stdout));
    let branches = parsed["branches"]
        .as_array()
        .expect("JSON should have branches array");

    assert!(
        branches
            .iter()
            .all(|b| b["name"].as_str() != Some("current-json-merged")),
        "current branch must not appear in sweep JSON:\n{}",
        stdout
    );
}

#[test]
fn sweep_classifies_squash_merged_branch() {
    // A branch whose commits were applied to trunk via `git merge --squash`
    // (and then committed) is integrated even though its tip is not an ancestor
    // of trunk. sweep must classify it as merged, not active.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "feature-squashed"]).assert_success();
    repo.create_file("squashed.txt", "squashed contents");
    repo.commit("feature work to be squashed");

    // Squash-merge the feature into main, leaving the original branch in place.
    repo.run_stax(&["t"]).assert_success();
    let squash = repo.git(&["merge", "--squash", "feature-squashed"]);
    assert!(
        squash.status.success(),
        "git merge --squash failed: {}",
        TestRepo::stderr(&squash)
    );
    let commit = repo.git(&["commit", "-m", "Squash merge feature-squashed"]);
    assert!(
        commit.status.success(),
        "commit of squashed changes failed: {}",
        TestRepo::stderr(&commit)
    );

    let out = repo.run_stax(&["sweep", "--json"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("sweep --json should output valid JSON: {}\n{}", e, stdout));
    let branch = parsed["branches"]
        .as_array()
        .expect("JSON should have branches array")
        .iter()
        .find(|b| b["name"].as_str() == Some("feature-squashed"))
        .expect("feature-squashed should appear in sweep JSON");
    assert_eq!(
        branch["status"].as_str(),
        Some("merged"),
        "squash-merged branch should be classified as merged, not active:\n{}",
        stdout
    );
}

#[test]
fn sweep_classifies_untracked_merged_branch() {
    // A branch created with plain git (not stax track) and then merged should
    // still be classified as merged by sweep.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create an untracked branch with raw git
    repo.git(&["checkout", "-b", "untracked-merged"]);
    repo.create_file("untracked.txt", "x");
    repo.commit("untracked commit");

    // Merge into main
    repo.git(&["checkout", "main"]);
    merge_branch(&repo, "untracked-merged");

    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("untracked-merged"),
        "Expected untracked merged branch in sweep output:\n{}",
        stdout
    );
    assert!(
        stdout.contains("merged"),
        "Expected 'merged' bucket:\n{}",
        stdout
    );
}

#[test]
fn sweep_classifies_stale_branch_with_custom_threshold() {
    // We can't fake old commit timestamps easily, but we can verify that a
    // branch with a very recent commit does NOT appear as stale with a 1-day
    // threshold (since it was just created).
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "fresh-branch"]).assert_success();
    repo.create_file("fresh.txt", "new");
    repo.commit("fresh commit");
    repo.run_stax(&["t"]).assert_success();

    let out = repo.run_stax(&["sweep", "--stale-days", "1"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    // A branch committed to right now must appear in the "active" bucket,
    // not in any "stale  (" bucket. We check specifically for the stale bucket
    // header rather than the word "stale" which also appears in the header line.
    assert!(
        !stdout.contains("stale  ("),
        "A just-created branch should not be in the stale bucket:\n{}",
        stdout
    );
    assert!(
        stdout.contains("active"),
        "fresh-branch should be in the active bucket:\n{}",
        stdout
    );
}

#[test]
fn sweep_shows_active_unmerged_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "work-in-progress"]).assert_success();
    repo.create_file("wip.txt", "wip");
    repo.commit("wip");
    repo.run_stax(&["t"]).assert_success();

    let out = repo.run_stax(&["sweep"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("active"),
        "Expected 'active' bucket for unmerged branch:\n{}",
        stdout
    );
    assert!(
        stdout.contains("work-in-progress"),
        "Expected work-in-progress in active bucket:\n{}",
        stdout
    );
}

#[test]
fn sweep_does_not_treat_upstream_gone_with_local_work_as_deletable() {
    let repo = TestRepo::new_with_remote();
    repo.run_stax(&["init"]).assert_success();

    assert!(
        repo.git(&["checkout", "-b", "gone-with-local-work"])
            .status
            .success()
    );
    repo.create_file("pushed.txt", "pushed");
    repo.commit("pushed work");
    assert!(
        repo.git(&["push", "-u", "origin", "gone-with-local-work"])
            .status
            .success()
    );

    repo.create_file("local-only.txt", "local only");
    repo.commit("local-only work");

    assert!(repo.git(&["checkout", "main"]).status.success());
    assert!(
        repo.git(&["push", "origin", "--delete", "gone-with-local-work"])
            .status
            .success()
    );
    assert!(repo.git(&["fetch", "--prune", "origin"]).status.success());

    let out = repo.run_stax(&["sweep", "--json"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("sweep --json should output valid JSON: {}\n{}", e, stdout));
    let branch = parsed["branches"]
        .as_array()
        .expect("JSON should have branches array")
        .iter()
        .find(|b| b["name"].as_str() == Some("gone-with-local-work"))
        .expect("gone-with-local-work should appear in sweep JSON");
    assert!(
        branch["status"].as_str() == Some("active"),
        "branch with local-only commits should be classified as active:\n{}",
        stdout
    );

    let delete_out = repo.run_stax(&["sweep", "--delete", "--force"]);
    delete_out.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.contains(&"gone-with-local-work".to_string()),
        "gone-with-local-work should survive sweep --delete --force:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_removes_gone_branch_without_unique_work() {
    let repo = TestRepo::new_with_remote();
    repo.run_stax(&["init"]).assert_success();

    assert!(
        repo.git(&["checkout", "-b", "gone-without-local-work"])
            .status
            .success()
    );
    assert!(
        repo.git(&["push", "-u", "origin", "gone-without-local-work"])
            .status
            .success()
    );

    assert!(repo.git(&["checkout", "main"]).status.success());
    assert!(
        repo.git(&["push", "origin", "--delete", "gone-without-local-work"])
            .status
            .success()
    );
    assert!(repo.git(&["fetch", "--prune", "origin"]).status.success());

    let out = repo.run_stax(&["sweep", "--json"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("sweep --json should output valid JSON: {}\n{}", e, stdout));
    let branch = parsed["branches"]
        .as_array()
        .expect("JSON should have branches array")
        .iter()
        .find(|b| b["name"].as_str() == Some("gone-without-local-work"))
        .expect("gone-without-local-work should appear in sweep JSON");
    assert_eq!(
        branch["status"].as_str(),
        Some("merged"),
        "branch without unique commits should remain a safe deletion candidate"
    );

    let delete_out = repo.run_stax(&["sweep", "--delete", "--force"]);
    delete_out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&"gone-without-local-work".to_string()),
        "gone-without-local-work should be deleted:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_removes_tracked_pr_branch_when_upstream_was_deleted() {
    let repo = TestRepo::new_with_remote();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "squash-merged-pr"]).assert_success();
    repo.create_file("squash-pr.txt", "merged via forge");
    repo.commit("squash merged pr work");
    repo.git(&["push", "-u", "origin", "squash-merged-pr"]);
    write_branch_pr_metadata(&repo, "squash-merged-pr", "main", 42);

    repo.run_stax(&["t"]).assert_success();
    let remote_path = repo.remote_path().expect("remote repo path");
    let delete_remote = Command::new("git")
        .args([
            "--git-dir",
            remote_path.to_str().expect("remote path UTF-8"),
            "update-ref",
            "-d",
            "refs/heads/squash-merged-pr",
        ])
        .output()
        .expect("failed to delete branch directly from bare remote");
    assert!(
        delete_remote.status.success(),
        "failed to delete remote branch: {}",
        String::from_utf8_lossy(&delete_remote.stderr)
    );
    let stale_remote_ref = repo.git(&[
        "show-ref",
        "--verify",
        "refs/remotes/origin/squash-merged-pr",
    ]);
    assert!(
        stale_remote_ref.status.success(),
        "test setup should leave stale local remote-tracking ref before sweep"
    );

    let out = repo.run_stax(&["sweep", "--delete", "--force"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&"squash-merged-pr".to_string()),
        "sweep --delete --force should delete a tracked PR branch whose upstream was deleted:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_removes_branch_with_merged_pr_metadata() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "metadata-merged-pr"])
        .assert_success();
    repo.create_file("metadata-pr.txt", "merged according to forge metadata");
    repo.commit("metadata merged pr work");
    write_branch_pr_metadata_with_state(&repo, "metadata-merged-pr", "main", 43, "MERGED");

    repo.run_stax(&["t"]).assert_success();
    let out = repo.run_stax(&["sweep", "--delete", "--force"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&"metadata-merged-pr".to_string()),
        "sweep --delete --force should delete a branch whose PR metadata is MERGED:\n{:?}",
        branches
    );
}

// ---------------------------------------------------------------------------
// Phase 2 — opt-in deletion
// ---------------------------------------------------------------------------

#[test]
fn sweep_delete_removes_merged_branches() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "done-branch"]).assert_success();
    repo.create_file("done.txt", "done");
    repo.commit("done");
    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "done-branch");

    let out = repo.run_stax(&["sweep", "--delete", "--force"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&"done-branch".to_string()),
        "done-branch should have been deleted:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_requires_force_or_prompt() {
    // Without --force, the command should succeed (prompt) or require user input.
    // In tests we can't interact, but at minimum the dry-run (no --delete) must
    // not delete anything.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "keep-me"]).assert_success();
    repo.create_file("k.txt", "k");
    repo.commit("keep");
    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "keep-me");

    // sweep without --delete must not delete
    let out = repo.run_stax(&["sweep"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.contains(&"keep-me".to_string()),
        "keep-me should still exist after read-only sweep:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_never_deletes_current_or_trunk() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // current = main, also the trunk
    let out = repo.run_stax(&["sweep", "--delete", "--force"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.contains(&"main".to_string()),
        "trunk 'main' must never be deleted:\n{:?}",
        branches
    );
}

#[test]
fn sweep_delete_does_not_delete_stale_without_include_stale() {
    // A branch that is only stale (not merged or gone) must survive --delete
    // unless --include-stale is also given.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create an untracked non-merged branch
    repo.git(&["checkout", "-b", "old-wip"]);
    repo.create_file("old.txt", "old");
    repo.commit("old work");
    repo.git(&["checkout", "main"]);

    // With a 0-day stale threshold everything is "stale", but without
    // --include-stale, --delete should NOT touch it.
    let out = repo.run_stax(&["sweep", "--delete", "--force", "--stale-days", "0"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.contains(&"old-wip".to_string()),
        "old-wip should survive --delete without --include-stale:\n{:?}",
        branches
    );
}

#[test]
fn sweep_include_stale_requires_delete_flag() {
    // clap should reject --include-stale without --delete
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let out = repo.run_stax(&["sweep", "--include-stale"]);
    assert!(
        !out.status.success(),
        "sweep --include-stale without --delete should fail:\n{}",
        TestRepo::stderr(&out)
    );
}

// ---------------------------------------------------------------------------
// Phase 3 — JSON output
// ---------------------------------------------------------------------------

#[test]
fn sweep_json_outputs_valid_json() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.run_stax(&["bc", "branch-a"]).assert_success();
    repo.create_file("a.txt", "a");
    repo.commit("commit a");
    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "branch-a");

    repo.run_stax(&["bc", "branch-b"]).assert_success();
    repo.create_file("b.txt", "b");
    repo.commit("commit b");
    repo.run_stax(&["t"]).assert_success();

    let out = repo.run_stax(&["sweep", "--json"]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);

    let parsed: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("sweep --json should output valid JSON: {}\n{}", e, stdout));

    let branches = parsed["branches"]
        .as_array()
        .expect("JSON should have branches array");

    let branch_a = branches
        .iter()
        .find(|b| b["name"].as_str() == Some("branch-a"))
        .expect("branch-a should appear in JSON");
    assert_eq!(
        branch_a["status"].as_str(),
        Some("merged"),
        "branch-a should be merged"
    );

    let branch_b = branches
        .iter()
        .find(|b| b["name"].as_str() == Some("branch-b"))
        .expect("branch-b should appear in JSON");
    assert_eq!(
        branch_b["status"].as_str(),
        Some("active"),
        "branch-b should be active"
    );
}

#[test]
fn sweep_json_conflicts_with_delete() {
    // --json and --delete are mutually exclusive
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    let out = repo.run_stax(&["sweep", "--json", "--delete"]);
    assert!(
        !out.status.success(),
        "sweep --json --delete should fail (conflicting flags):\n{}",
        TestRepo::stderr(&out)
    );
}

#[test]
fn sweep_reparents_tracked_children_on_delete() {
    // When a tracked branch is deleted via sweep, its stax-tracked child
    // should be reparented to trunk so `stax status` doesn't error out.
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Create parent branch (tracked)
    repo.run_stax(&["bc", "parent-merged"]).assert_success();
    repo.create_file("parent.txt", "p");
    repo.commit("parent commit");

    // Create child branch stacked on top (tracked)
    repo.run_stax(&["bc", "child-active"]).assert_success();
    repo.create_file("child.txt", "c");
    repo.commit("child commit");

    // Go back to main and merge the parent
    repo.run_stax(&["t"]).assert_success();
    merge_branch(&repo, "parent-merged");

    // sweep --delete should delete parent-merged and reparent child-active to main
    let out = repo.run_stax(&["sweep", "--delete", "--force"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&"parent-merged".to_string()),
        "parent-merged should be deleted"
    );
    assert!(
        branches.contains(&"child-active".to_string()),
        "child-active should still exist"
    );

    // stax status should succeed (no orphan metadata errors)
    let status_out = repo.run_stax(&["status"]);
    status_out.assert_success();
}
