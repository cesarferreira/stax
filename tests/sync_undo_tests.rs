use crate::common::TestRepo;

/// Helper: read the parentBranchName from stax metadata for `branch`.
fn read_parent_branch(repo: &TestRepo, branch: &str) -> String {
    let metadata_ref = format!("refs/branch-metadata/{}", branch);
    let out = repo.git(&["show", &metadata_ref]);
    assert!(
        out.status.success(),
        "failed to read metadata for {branch}: {}",
        TestRepo::stderr(&out)
    );
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&out)).expect("invalid metadata JSON");
    json["parentBranchName"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Helper: read the parentBranchRevision from stax metadata for `branch`.
fn read_parent_revision(repo: &TestRepo, branch: &str) -> String {
    let metadata_ref = format!("refs/branch-metadata/{}", branch);
    let out = repo.git(&["show", &metadata_ref]);
    assert!(
        out.status.success(),
        "failed to read metadata for {branch}: {}",
        TestRepo::stderr(&out)
    );
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&out)).expect("invalid metadata JSON");
    json["parentBranchRevision"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// Helper: resolve a local ref to its SHA via git rev-parse (returns None if the ref doesn't
/// exist).
fn resolve_ref(repo: &TestRepo, refname: &str) -> Option<String> {
    let out = repo.git(&["rev-parse", "--verify", refname]);
    if out.status.success() {
        Some(TestRepo::stdout(&out).trim().to_string())
    } else {
        None
    }
}

/// Helper: check that a local branch exists.
fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    resolve_ref(repo, &format!("refs/heads/{branch}")).is_some()
}

/// Helper: count the number of stax receipts in `.git/stax/ops/`.
fn receipt_count(repo: &TestRepo) -> usize {
    let ops_dir = repo.path().join(".git/stax/ops");
    if !ops_dir.exists() {
        return 0;
    }
    std::fs::read_dir(&ops_dir)
        .expect("read ops dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .count()
}

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_undo_restores_a_merged_branch_at_its_old_tip() {
    let repo = TestRepo::new_with_remote();

    // Create feature-a on main, push it, and merge it on the remote.
    repo.run_stax(&["bc", "feature-a"]);
    let feature = repo.current_branch();
    repo.create_file("feature-a.txt", "feature content");
    repo.commit("Feature A commit");
    let feature_sha_before = repo.get_commit_sha(&feature);

    repo.git(&["push", "-u", "origin", &feature]);

    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);

    // Precondition: branch is already merged.
    let merged_out = repo.git(&["branch", "--merged", "main"]);
    let merged_str = TestRepo::stdout(&merged_out);
    assert!(
        merged_str.contains(&feature),
        "expected {feature} merged into main before sync"
    );

    // Run sync --force, which should delete the merged branch.
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    assert!(
        !branch_exists(&repo, &feature),
        "branch {feature} should have been deleted by sync"
    );

    // Metadata ref should also be gone.
    assert!(
        resolve_ref(&repo, &format!("refs/branch-metadata/{feature}")).is_none(),
        "metadata ref for {feature} should have been deleted by sync"
    );

    // Now undo.
    let undo_out = repo.run_stax(&["undo", "--yes"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    // Branch head should be restored to the pre-delete SHA.
    assert!(
        branch_exists(&repo, &feature),
        "undo should have restored the branch {feature}"
    );
    let feature_sha_after_undo = repo.get_commit_sha(&feature);
    assert_eq!(
        feature_sha_before, feature_sha_after_undo,
        "undo should restore the branch to its original tip"
    );

    // Metadata ref should also be restored.
    assert!(
        resolve_ref(&repo, &format!("refs/branch-metadata/{feature}")).is_some(),
        "undo should restore the metadata ref for {feature}"
    );
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_undo_restores_reparented_child_metadata() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main → a → b
    repo.run_stax(&["bc", "branch-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a");
    repo.commit("Branch A commit");
    repo.git(&["push", "-u", "origin", &branch_a]);

    repo.run_stax(&["bc", "branch-b"]);
    let branch_b = repo.current_branch();
    repo.create_file("b.txt", "b");
    repo.commit("Branch B commit");

    // Merge A on the remote (so sync sees it as merged).
    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&branch_a);
    repo.git(&["pull", "origin", "main"]);

    let merged_out = repo.git(&["branch", "--merged", "main"]);
    let merged_str = TestRepo::stdout(&merged_out);
    assert!(
        merged_str.contains(&branch_a),
        "expected {branch_a} merged into main before sync"
    );

    // Verify b's parent is a before sync.
    let b_parent_before = read_parent_branch(&repo, &branch_b);
    assert_eq!(b_parent_before, branch_a, "b should point to a before sync");

    // sync --force: deletes a and reparents b → main.
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    assert!(
        !branch_exists(&repo, &branch_a),
        "branch_a should have been deleted"
    );

    // After sync, b's parent should have moved away from a.
    let b_parent_after_sync = read_parent_branch(&repo, &branch_b);
    assert_ne!(
        b_parent_after_sync, branch_a,
        "after sync, b should no longer point to the deleted a"
    );

    // Undo — should restore a AND restore b's parent → a.
    let undo_out = repo.run_stax(&["undo", "--yes"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    assert!(
        branch_exists(&repo, &branch_a),
        "undo should restore branch_a"
    );

    let b_parent_after_undo = read_parent_branch(&repo, &branch_b);
    assert_eq!(
        b_parent_after_undo, branch_a,
        "undo should restore b's parent metadata back to a"
    );
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_that_changes_nothing_leaves_the_previous_receipt_undoable() {
    let repo = TestRepo::new_with_remote();

    // Create a branch, commit, and restack to produce a restack receipt.
    repo.run_stax(&["bc", "feature-noop"]);
    let feature = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");

    repo.run_stax(&["checkout", &feature]);
    let restack_out = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        restack_out.status.success(),
        "restack failed: {}",
        TestRepo::stderr(&restack_out)
    );

    let receipts_after_restack = receipt_count(&repo);
    assert!(
        receipts_after_restack >= 1,
        "expected at least one receipt after restack"
    );

    let feature_sha_after_restack = repo.get_commit_sha(&feature);

    // Now run sync with no remote changes — nothing should be snapshotted.
    // (No remote, so fetch either uses a dummy or is skipped for a no-change run.)
    // We just run sync with --force to skip interactive prompts; since nothing changed
    // on the remote, the sync transaction is a no-op and leaves no new receipt.
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    let receipts_after_noop_sync = receipt_count(&repo);
    assert_eq!(
        receipts_after_restack, receipts_after_noop_sync,
        "a no-op sync must not create a new receipt (lazy snapshot guard)"
    );

    // Undo should still undo the RESTACK (the previous receipt), not the sync.
    let undo_out = repo.run_stax(&["undo", "--yes"]);
    assert!(
        undo_out.status.success(),
        "undo after no-op sync failed: {}",
        TestRepo::stderr(&undo_out)
    );

    // After undoing the restack, feature should be back at its pre-restack SHA.
    // (The feature commit SHA changed during restack so post-undo != post-restack.)
    let feature_sha_after_undo = repo.get_commit_sha(&feature);
    assert_ne!(
        feature_sha_after_restack, feature_sha_after_undo,
        "undo should have reversed the restack, changing the feature branch SHA"
    );
}

// ─── Fast-forward-only sync and retention helpers ────────────────────────────

/// Helper: count refs under `refs/stax/backups/`.
fn backup_ref_count(repo: &TestRepo) -> usize {
    let out = repo.git(&["for-each-ref", "--format=%(refname)", "refs/stax/backups/"]);
    TestRepo::stdout(&out).lines().count()
}

/// Helper: list all refs under `refs/stax/backups/`.
fn backup_refs(repo: &TestRepo) -> Vec<String> {
    let out = repo.git(&["for-each-ref", "--format=%(refname)", "refs/stax/backups/"]);
    TestRepo::stdout(&out)
        .lines()
        .map(|l| l.to_string())
        .collect()
}

/// Helper: id of the lexically newest receipt, if any.
fn latest_receipt_id(repo: &TestRepo) -> Option<String> {
    let ops_dir = repo.path().join(".git/stax/ops");
    let mut ids: Vec<String> = std::fs::read_dir(&ops_dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            name.strip_suffix(".json").map(|s| s.to_string())
        })
        .collect();
    ids.sort();
    ids.pop()
}

fn receipt_path(repo: &TestRepo, id: &str) -> std::path::PathBuf {
    repo.path().join(format!(".git/stax/ops/{id}.json"))
}

fn read_receipt(repo: &TestRepo, id: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(receipt_path(repo, id)).expect("read receipt");
    serde_json::from_str(&text).expect("parse receipt")
}

/// Helper: write the user stax config used by the child processes.
fn write_undo_config(repo: &TestRepo, toml: &str) {
    let home = std::path::PathBuf::from(repo.clean_home());
    std::fs::write(home.join(".config").join("stax").join("config.toml"), toml)
        .expect("write stax config");
}

/// Helper: a feature branch pushed to origin, left checked out (trunk not checked out).
fn feature_pushed(repo: &TestRepo, name: &str) -> String {
    repo.run_stax(&["bc", name]);
    let feature = repo.current_branch();
    repo.create_file(&format!("{name}.txt"), "feature");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature]);
    feature
}

/// Helper: produce one real restack receipt and return the feature branch.
/// Trunk is pushed so a later remote commit is a pure fast-forward.
fn make_real_receipt(repo: &TestRepo) -> String {
    repo.run_stax(&["bc", "feature-real"]);
    let feature = repo.current_branch();
    repo.create_file("feature-real.txt", "feature");
    repo.commit("Feature commit");

    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");
    repo.git(&["push", "origin", "main"]);

    repo.run_stax(&["checkout", &feature]);
    let out = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        out.status.success(),
        "restack failed: {}",
        TestRepo::stderr(&out)
    );
    feature
}

/// Helper: write a fabricated receipt (copied from `template_id`) plus a backup ref.
fn fabricate_receipt(repo: &TestRepo, template_id: &str, id: &str, status: &str, with_ref: bool) {
    let mut json = read_receipt(repo, template_id);
    json["op_id"] = serde_json::Value::String(id.to_string());
    json["status"] = serde_json::Value::String(status.to_string());
    std::fs::write(
        receipt_path(repo, id),
        serde_json::to_string_pretty(&json).expect("serialize receipt"),
    )
    .expect("write fabricated receipt");
    if with_ref {
        let sha = repo.get_commit_sha("main");
        let out = repo.git(&["update-ref", &format!("refs/stax/backups/{id}/main"), &sha]);
        assert!(out.status.success(), "update-ref failed");
    }
}

fn prune_stamp_path(repo: &TestRepo) -> std::path::PathBuf {
    repo.path().join(".git/stax/ops-prune.json")
}

// ─── Test 4 ──────────────────────────────────────────────────────────────────

#[test]
fn fast_forward_only_sync_off_trunk_writes_no_receipt_or_backup_refs() {
    let repo = TestRepo::new_with_remote();
    let trunk_sha_before = repo.get_commit_sha("main");
    feature_pushed(&repo, "feature-ff");
    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");

    let receipts_before = receipt_count(&repo);
    let backups_before = backup_ref_count(&repo);
    let latest_before = latest_receipt_id(&repo);

    // Feature is checked out, so trunk moves through the bare update-ref path.
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    assert_ne!(
        trunk_sha_before,
        repo.get_commit_sha("main"),
        "trunk should have moved during sync"
    );
    assert_eq!(receipt_count(&repo), receipts_before, "no receipt expected");
    assert_eq!(
        backup_ref_count(&repo),
        backups_before,
        "no backup refs expected"
    );
    assert_eq!(latest_receipt_id(&repo), latest_before);
    let stdout = TestRepo::stdout(&sync_out);
    assert!(
        !stdout.contains("Backup refs created"),
        "unexpected backup message: {stdout}"
    );
}

#[test]
fn fast_forward_only_sync_on_trunk_keeps_previous_receipt_latest() {
    let repo = TestRepo::new_with_remote();
    make_real_receipt(&repo);
    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");
    repo.run_stax(&["t"]);

    let receipts_before = receipt_count(&repo);
    let backups_before = backup_ref_count(&repo);
    let latest_before = latest_receipt_id(&repo).expect("restack receipt");
    let trunk_before = repo.get_commit_sha("main");

    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    let trunk_after_sync = repo.get_commit_sha("main");
    assert_ne!(trunk_before, trunk_after_sync, "trunk should have moved");
    assert_eq!(receipt_count(&repo), receipts_before);
    assert_eq!(backup_ref_count(&repo), backups_before);
    assert_eq!(
        latest_receipt_id(&repo).as_deref(),
        Some(latest_before.as_str())
    );
    assert!(!TestRepo::stdout(&sync_out).contains("Backup refs created"));

    // Undo targets the previous operation (the restack), not trunk.
    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );
    assert_eq!(
        repo.get_commit_sha("main"),
        trunk_after_sync,
        "undo must not roll trunk back after a fast-forward-only sync"
    );
}

#[test]
fn fast_forward_only_sync_with_trunk_in_linked_worktree_writes_no_receipt() {
    let repo = TestRepo::new_with_remote();
    feature_pushed(&repo, "feature-wt");
    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");

    let wt_parent = tempfile::TempDir::new().expect("tempdir");
    let wt = wt_parent.path().join("trunk-wt");
    let add = repo.git(&["worktree", "add", wt.to_str().expect("utf8"), "main"]);
    assert!(
        add.status.success(),
        "worktree add failed: {}",
        TestRepo::stderr(&add)
    );

    let trunk_before = repo.get_commit_sha("main");
    let receipts_before = receipt_count(&repo);
    let backups_before = backup_ref_count(&repo);

    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    assert_ne!(
        trunk_before,
        repo.get_commit_sha("main"),
        "trunk should advance"
    );
    assert!(
        wt.join("remote-main.txt").exists(),
        "trunk worktree should have been fast-forwarded"
    );
    assert_eq!(receipt_count(&repo), receipts_before);
    assert_eq!(backup_ref_count(&repo), backups_before);
    assert!(!TestRepo::stdout(&sync_out).contains("Backup refs created"));
}

#[test]
fn sync_fast_forward_with_destructive_work_records_trunk_in_receipt() {
    let repo = TestRepo::new_with_remote();
    let trunk_sha_before = repo.get_commit_sha("main");
    let feature = feature_pushed(&repo, "feature-rec");
    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);
    repo.simulate_remote_commit("remote-trunk.txt", "from remote", "Remote trunk commit");
    let trunk_sha_before_sync = repo.get_commit_sha("main");
    assert_ne!(trunk_sha_before, trunk_sha_before_sync);
    repo.run_stax(&["checkout", &feature]);

    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    assert!(TestRepo::stdout(&sync_out).contains("Backup refs created"));

    let id = latest_receipt_id(&repo).expect("sync receipt");
    let receipt = read_receipt(&repo, &id);
    let main_entry = receipt["local_refs"]
        .as_array()
        .expect("local_refs")
        .iter()
        .find(|e| e["branch"] == "main")
        .expect("trunk entry in receipt")
        .clone();
    assert_eq!(
        main_entry["oid_before"].as_str(),
        Some(trunk_sha_before_sync.as_str()),
        "trunk entry must carry the pre-sync OID"
    );
    assert!(
        resolve_ref(&repo, &format!("refs/stax/backups/{id}/main")).is_some(),
        "trunk backup ref should exist"
    );
}

#[test]
fn sync_with_diverged_trunk_still_snapshots() {
    let repo = TestRepo::new_with_remote();
    feature_pushed(&repo, "feature-div");
    repo.run_stax(&["t"]);
    repo.create_file("local-only.txt", "local");
    repo.commit("Local only trunk commit");
    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");

    let receipts_before = receipt_count(&repo);
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    assert_eq!(
        receipt_count(&repo),
        receipts_before + 1,
        "a diverged trunk keeps the conservative snapshot"
    );
}

#[test]
fn sync_resetting_dirty_linked_worktree_trunk_snapshots_first() {
    let repo = TestRepo::new_with_remote();
    repo.create_file("tracked.txt", "base\n");
    repo.commit("Add tracked file");
    repo.git(&["push", "origin", "main"]);
    feature_pushed(&repo, "feature-reset");

    let wt_parent = tempfile::TempDir::new().expect("tempdir");
    let wt = wt_parent.path().join("trunk-wt");
    let add = repo.git(&["worktree", "add", wt.to_str().expect("utf8"), "main"]);
    assert!(
        add.status.success(),
        "worktree add failed: {}",
        TestRepo::stderr(&add)
    );
    std::fs::write(wt.join("tracked.txt"), "dirty\n").expect("dirty tracked file");

    repo.simulate_remote_commit("tracked.txt", "remote\n", "Remote edits tracked");
    let receipts_before = receipt_count(&repo);

    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );
    assert_eq!(
        receipt_count(&repo),
        receipts_before + 1,
        "reset of the trunk worktree must snapshot first"
    );
    assert!(TestRepo::stdout(&sync_out).contains("Backup refs created"));
    let id = latest_receipt_id(&repo).expect("receipt");
    let receipt = read_receipt(&repo, &id);
    assert!(
        receipt["local_refs"]
            .as_array()
            .expect("local_refs")
            .iter()
            .any(|e| e["branch"] == "main"),
        "receipt should contain the trunk entry"
    );
}

// ─── Retention ───────────────────────────────────────────────────────────────

const OLD_A: &str = "20200101T000000Z-aaaaa1";
const OLD_B: &str = "20200101T000000Z-aaaaa2";
const OLD_FAILED: &str = "20200101T000000Z-aaaaa3";
const OLD_PROGRESS: &str = "20200101T000000Z-aaaaa4";
const ORPHAN: &str = "20200101T000000Z-ffffff";

#[test]
fn sync_prunes_old_successful_receipts_and_their_backup_refs() {
    let repo = TestRepo::new_with_remote();
    write_undo_config(&repo, "[undo]\nkeep_last = 1\nkeep_days = 30\n");
    make_real_receipt(&repo);
    let template = latest_receipt_id(&repo).expect("real receipt");

    fabricate_receipt(&repo, &template, OLD_A, "success", true);
    fabricate_receipt(&repo, &template, OLD_B, "success", true);
    fabricate_receipt(&repo, &template, OLD_FAILED, "failed", true);
    fabricate_receipt(&repo, &template, OLD_PROGRESS, "in_progress", true);
    let sha = repo.get_commit_sha("main");
    repo.git(&[
        "update-ref",
        &format!("refs/stax/backups/{ORPHAN}/main"),
        &sha,
    ]);

    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    assert!(!receipt_path(&repo, OLD_A).exists());
    assert!(!receipt_path(&repo, OLD_B).exists());
    assert!(receipt_path(&repo, OLD_FAILED).exists());
    assert!(receipt_path(&repo, OLD_PROGRESS).exists());
    assert!(
        receipt_path(&repo, &template).exists(),
        "newest receipt kept"
    );

    let refs = backup_refs(&repo);
    assert!(!refs.iter().any(|r| r.contains(OLD_A)), "{refs:?}");
    assert!(!refs.iter().any(|r| r.contains(OLD_B)), "{refs:?}");
    assert!(refs.iter().any(|r| r.contains(OLD_FAILED)), "{refs:?}");
    assert!(refs.iter().any(|r| r.contains(OLD_PROGRESS)), "{refs:?}");
    assert!(
        refs.iter().any(|r| r.contains(ORPHAN)),
        "orphan ref must stay"
    );

    assert!(prune_stamp_path(&repo).exists(), "stamp should be written");

    // The newest receipt is still undoable.
    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );
}

#[test]
fn sync_prune_is_throttled_by_daily_stamp() {
    let repo = TestRepo::new_with_remote();
    write_undo_config(&repo, "[undo]\nkeep_last = 1\nkeep_days = 30\n");
    make_real_receipt(&repo);
    let template = latest_receipt_id(&repo).expect("real receipt");
    fabricate_receipt(&repo, &template, OLD_A, "success", true);

    let first = repo.run_stax(&["sync", "--force"]);
    assert!(first.status.success(), "{}", TestRepo::stderr(&first));
    assert!(!receipt_path(&repo, OLD_A).exists(), "first sync prunes");
    assert!(prune_stamp_path(&repo).exists());

    // Fresh stamp: a new old receipt survives the next sync.
    fabricate_receipt(&repo, &template, OLD_B, "success", true);
    let second = repo.run_stax(&["sync", "--force"]);
    assert!(second.status.success(), "{}", TestRepo::stderr(&second));
    assert!(
        receipt_path(&repo, OLD_B).exists(),
        "throttled sync must not prune"
    );

    // Stale stamp: due again.
    std::fs::write(prune_stamp_path(&repo), r#"{"last_pruned_at": 1}"#).expect("write stamp");
    let third = repo.run_stax(&["sync", "--force"]);
    assert!(third.status.success(), "{}", TestRepo::stderr(&third));
    assert!(!receipt_path(&repo, OLD_B).exists(), "due sync prunes");
}

#[test]
fn sync_does_not_prune_when_auto_prune_disabled() {
    let repo = TestRepo::new_with_remote();
    write_undo_config(
        &repo,
        "[undo]\nauto_prune = false\nkeep_last = 1\nkeep_days = 0\n",
    );
    make_real_receipt(&repo);
    let template = latest_receipt_id(&repo).expect("real receipt");
    fabricate_receipt(&repo, &template, OLD_A, "success", true);

    let out = repo.run_stax(&["sync", "--force"]);
    assert!(out.status.success(), "{}", TestRepo::stderr(&out));
    assert!(receipt_path(&repo, OLD_A).exists());
    assert!(!prune_stamp_path(&repo).exists(), "no stamp when disabled");
}

#[test]
fn sync_prune_keeps_corrupt_old_receipts() {
    let repo = TestRepo::new_with_remote();
    write_undo_config(&repo, "[undo]\nkeep_last = 1\nkeep_days = 0\n");
    make_real_receipt(&repo);
    let template = latest_receipt_id(&repo).expect("real receipt");
    fabricate_receipt(&repo, &template, OLD_A, "success", false);
    std::fs::write(receipt_path(&repo, OLD_B), "not json").expect("write corrupt receipt");

    let out = repo.run_stax(&["sync", "--force"]);
    assert!(out.status.success(), "{}", TestRepo::stderr(&out));
    assert!(!receipt_path(&repo, OLD_A).exists());
    assert!(
        receipt_path(&repo, OLD_B).exists(),
        "unparsable receipt is kept"
    );
}

// ─── Test 5 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_undo_then_redo_replays_deletions_and_trunk() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main → feature-del (to be deleted by sync).
    repo.run_stax(&["bc", "feature-del"]);
    let feature = repo.current_branch();
    repo.create_file("del.txt", "del");
    repo.commit("Feature del commit");
    repo.git(&["push", "-u", "origin", &feature]);

    // Merge feature on remote so sync will delete it.
    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);

    // Pull so local main sees the merged commit first.
    repo.git(&["pull", "origin", "main"]);

    // Simulate a remote commit on trunk AFTER the pull so sync has something to fast-forward.
    repo.simulate_remote_commit("remote-trunk.txt", "from remote", "Remote trunk commit");

    let trunk_sha_before_sync = repo.get_commit_sha("main");
    let feature_sha = resolve_ref(&repo, &format!("refs/heads/{feature}"))
        .expect("feature branch should exist before sync");

    // Checkout feature so trunk is updated via the non-checked-out path.
    repo.run_stax(&["checkout", &feature]);

    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    // Verify sync deleted feature and advanced trunk.
    assert!(
        !branch_exists(&repo, &feature),
        "feature should have been deleted by sync"
    );
    let trunk_sha_after_sync = repo.get_commit_sha("main");
    assert_ne!(
        trunk_sha_before_sync, trunk_sha_after_sync,
        "trunk should have moved during sync"
    );

    // The receipt must still carry trunk with its pre-sync OID and a backup ref.
    let receipt_id = latest_receipt_id(&repo).expect("sync receipt");
    let receipt = read_receipt(&repo, &receipt_id);
    let trunk_entry = receipt["local_refs"]
        .as_array()
        .expect("local_refs")
        .iter()
        .find(|e| e["branch"] == "main")
        .expect("trunk entry in receipt");
    assert_eq!(
        trunk_entry["oid_before"].as_str(),
        Some(trunk_sha_before_sync.as_str())
    );
    assert!(
        resolve_ref(&repo, &format!("refs/stax/backups/{receipt_id}/main")).is_some(),
        "trunk backup ref should exist"
    );

    // Undo — restore feature and roll back trunk.
    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    assert!(
        branch_exists(&repo, &feature),
        "undo should restore the deleted feature branch"
    );
    assert_eq!(
        resolve_ref(&repo, &format!("refs/heads/{feature}")).as_deref(),
        Some(feature_sha.as_str()),
        "undo should restore feature to its original SHA"
    );
    let trunk_sha_after_undo = repo.get_commit_sha("main");
    assert_eq!(
        trunk_sha_before_sync, trunk_sha_after_undo,
        "undo should roll trunk back to its pre-sync SHA"
    );

    // Redo — delete feature again and re-advance trunk.
    let redo_out = repo.run_stax(&["redo", "--yes", "--no-push"]);
    assert!(
        redo_out.status.success(),
        "redo failed: {}",
        TestRepo::stderr(&redo_out)
    );

    assert!(
        !branch_exists(&repo, &feature),
        "redo should delete feature again"
    );
    let trunk_sha_after_redo = repo.get_commit_sha("main");
    assert_eq!(
        trunk_sha_after_sync, trunk_sha_after_redo,
        "redo should re-advance trunk to the post-sync SHA"
    );
}

// ─── Test 6 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_restack_undo_rolls_back_trunk_fast_forward() {
    // Regression test: sync --restack used to leave a duplicate `main` entry in
    // the receipt (plan_trunk_move added it with the pre-ff OID; plan_branches
    // added it again with the post-ff OID).  On undo, apply_transaction iterated
    // both entries, first resetting main to pre-ff, then forward to post-ff again
    // — leaving trunk un-rolled-back.  The dedup fix in add_local_ref keeps only
    // the earliest snapshot, so undo now correctly restores trunk.
    let repo = TestRepo::new_with_remote();

    // Create a feature branch off main.
    repo.run_stax(&["bc", "feature-restack"]);
    let feature = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    // Stay on the feature branch: restack scope is the current stack, and trunk is
    // fast-forwarded through the non-checkout path.
    // Capture trunk SHA before any sync activity.
    let trunk_sha_before_sync = repo.get_commit_sha("main");

    // Advance origin/main so sync has a fast-forward to perform.
    repo.simulate_remote_commit(
        "remote-sync-restack.txt",
        "remote",
        "Remote commit for sync",
    );

    // Run sync with restack so both plan_trunk_move AND plan_branches(restack_scope) fire.
    let sync_out = repo.run_stax(&["sync", "--force", "--restack"]);
    assert!(
        sync_out.status.success(),
        "sync --force --restack failed: {}",
        TestRepo::stderr(&sync_out)
    );

    let trunk_sha_after_sync = repo.get_commit_sha("main");
    assert_ne!(
        trunk_sha_before_sync, trunk_sha_after_sync,
        "trunk should have moved during sync"
    );

    // Undo — trunk must be rolled all the way back to the pre-sync SHA.
    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    let trunk_sha_after_undo = repo.get_commit_sha("main");
    assert_eq!(
        trunk_sha_before_sync, trunk_sha_after_undo,
        "undo should restore trunk to its pre-sync SHA (duplicate receipt entry bug)"
    );

    // Feature branch should still exist (nothing was deleted by this sync).
    assert!(
        branch_exists(&repo, &feature),
        "feature branch should still exist after undo"
    );
}

// ─── Test 7 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_restack_conflict_undo_restores_branch_metadata() {
    // Regression test: sync's restack phase only snapshotted branch heads, not
    // branch-metadata refs. When a later branch in the restack scope hit a
    // conflict and sync stopped, undo restored earlier branches' commits but
    // left their rewritten parentBranchRevision in place.
    let repo = TestRepo::new_with_remote();

    // Build stack: main → branch-a → branch-b.
    repo.run_stax(&["bc", "branch-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a");
    repo.commit("Branch A commit");

    repo.run_stax(&["bc", "branch-b"]);
    repo.create_file("shared.txt", "from branch b");
    repo.commit("Branch B commit");

    let main_sha_before = repo.get_commit_sha("main");
    let a_sha_before = repo.get_commit_sha(&branch_a);
    let a_parent_rev_before = read_parent_revision(&repo, &branch_a);
    assert_eq!(
        a_parent_rev_before, main_sha_before,
        "branch-a's recorded parent revision should be main's sha before sync"
    );

    // Advance remote main with a conflicting change to shared.txt, so branch-b's
    // rebase conflicts while branch-a's does not.
    repo.simulate_remote_commit(
        "shared.txt",
        "from remote main",
        "Remote conflicting commit",
    );

    let sync_out = repo.run_stax(&["sync", "--force", "--restack"]);
    assert!(
        !sync_out.status.success(),
        "sync should stop on the branch-b conflict"
    );

    assert_ne!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_before,
        "sync should have rewritten branch-a's metadata before hitting the conflict"
    );

    repo.abort_rebase();

    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    assert_eq!(
        repo.get_commit_sha(&branch_a),
        a_sha_before,
        "undo should restore branch-a to its pre-sync commit"
    );
    assert_eq!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_before,
        "undo should restore branch-a's metadata to its pre-sync parent revision"
    );
    assert_eq!(
        repo.get_commit_sha("main"),
        main_sha_before,
        "undo should restore main to its pre-sync sha"
    );

    // A *failed* sync's receipt cannot be redone (can_redo() requires
    // OpStatus::Success) — there is no coherent "completed" state to replay
    // back to. Confirm that's still rejected cleanly rather than silently
    // reapplying a half-finished restack.
    let redo_out = repo.run_stax(&["redo", "--yes", "--no-push"]);
    assert!(
        !redo_out.status.success(),
        "redo of a failed sync's receipt should be rejected, not silently reapplied"
    );
    assert_eq!(
        repo.get_commit_sha(&branch_a),
        a_sha_before,
        "rejected redo must leave branch-a exactly where undo left it"
    );
}

// ─── Test 8 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_restack_success_undo_then_redo_round_trips_branch_metadata() {
    // Companion to sync_restack_conflict_undo_restores_branch_metadata: that test
    // covers the undo direction after a *failed* restack. This test covers a
    // *successful* restack, where the receipt's after-OIDs for the metadata ref
    // must also be recorded so redo can replay the branch head and its rewritten
    // parentBranchRevision back together.
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "branch-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a");
    repo.commit("Branch A commit");

    let main_sha_before = repo.get_commit_sha("main");
    let a_sha_before = repo.get_commit_sha(&branch_a);
    let a_parent_rev_before = read_parent_revision(&repo, &branch_a);
    assert_eq!(
        a_parent_rev_before, main_sha_before,
        "branch-a's recorded parent revision should be main's sha before sync"
    );

    // Advance remote main with an unrelated file so branch-a's rebase succeeds
    // cleanly (no conflict).
    repo.simulate_remote_commit("remote.txt", "remote", "Remote commit for restack");

    let sync_out = repo.run_stax(&["sync", "--force", "--restack"]);
    assert!(
        sync_out.status.success(),
        "sync --force --restack failed: {}",
        TestRepo::stderr(&sync_out)
    );

    let main_sha_after_sync = repo.get_commit_sha("main");
    let a_sha_after_sync = repo.get_commit_sha(&branch_a);
    let a_parent_rev_after_sync = read_parent_revision(&repo, &branch_a);
    assert_ne!(
        a_sha_after_sync, a_sha_before,
        "sync should have rebased branch-a onto the new main"
    );
    assert_eq!(
        a_parent_rev_after_sync, main_sha_after_sync,
        "sync should have rewritten branch-a's parent revision to the new main sha"
    );

    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );
    assert_eq!(
        repo.get_commit_sha(&branch_a),
        a_sha_before,
        "undo should restore branch-a to its pre-sync commit"
    );
    assert_eq!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_before,
        "undo should restore branch-a's metadata to its pre-sync parent revision"
    );

    let redo_out = repo.run_stax(&["redo", "--yes", "--no-push"]);
    assert!(
        redo_out.status.success(),
        "redo failed: {}",
        TestRepo::stderr(&redo_out)
    );
    assert_eq!(
        repo.get_commit_sha(&branch_a),
        a_sha_after_sync,
        "redo should re-apply branch-a's restacked commit"
    );
    assert_eq!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_after_sync,
        "redo should re-apply branch-a's rewritten parent revision alongside its commit"
    );
    assert_eq!(
        repo.get_commit_sha("main"),
        main_sha_after_sync,
        "redo should re-advance trunk to the post-sync sha"
    );
}

// ─── Test 9 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_restack_conflict_undo_restores_metadata_for_every_successful_branch_in_a_three_branch_stack()
 {
    // Extends sync_restack_conflict_undo_restores_branch_metadata to a 3-branch
    // stack (main -> branch-a -> branch-b -> branch-c), where the two bottom
    // branches restack successfully before the top one conflicts. Undo must
    // restore BOTH branch-a's and branch-b's commit + metadata, not just the
    // bottommost one.
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "branch-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a");
    repo.commit("Branch A commit");

    repo.run_stax(&["bc", "branch-b"]);
    let branch_b = repo.current_branch();
    repo.create_file("b.txt", "b");
    repo.commit("Branch B commit");

    repo.run_stax(&["bc", "branch-c"]);
    repo.create_file("shared.txt", "from branch c");
    repo.commit("Branch C commit");

    let main_sha_before = repo.get_commit_sha("main");
    let a_sha_before = repo.get_commit_sha(&branch_a);
    let a_parent_rev_before = read_parent_revision(&repo, &branch_a);
    let b_sha_before = repo.get_commit_sha(&branch_b);
    let b_parent_rev_before = read_parent_revision(&repo, &branch_b);
    assert_eq!(
        a_parent_rev_before, main_sha_before,
        "branch-a's recorded parent revision should be main's sha before sync"
    );
    assert_eq!(
        b_parent_rev_before, a_sha_before,
        "branch-b's recorded parent revision should be branch-a's sha before sync"
    );

    // Advance remote main with a conflicting change to shared.txt, so
    // branch-c's rebase conflicts while branch-a's and branch-b's do not.
    repo.simulate_remote_commit(
        "shared.txt",
        "from remote main",
        "Remote conflicting commit",
    );

    let sync_out = repo.run_stax(&["sync", "--force", "--restack"]);
    assert!(
        !sync_out.status.success(),
        "sync should stop on the branch-c conflict"
    );

    assert_ne!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_before,
        "sync should have rewritten branch-a's metadata before hitting the conflict"
    );
    assert_ne!(
        read_parent_revision(&repo, &branch_b),
        b_parent_rev_before,
        "sync should have rewritten branch-b's metadata before hitting the conflict"
    );

    repo.abort_rebase();

    let undo_out = repo.run_stax(&["undo", "--yes", "--no-push"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    assert_eq!(
        repo.get_commit_sha(&branch_a),
        a_sha_before,
        "undo should restore branch-a to its pre-sync commit"
    );
    assert_eq!(
        read_parent_revision(&repo, &branch_a),
        a_parent_rev_before,
        "undo should restore branch-a's metadata to its pre-sync parent revision"
    );
    assert_eq!(
        repo.get_commit_sha(&branch_b),
        b_sha_before,
        "undo should restore branch-b to its pre-sync commit"
    );
    assert_eq!(
        read_parent_revision(&repo, &branch_b),
        b_parent_rev_before,
        "undo should restore branch-b's metadata to its pre-sync parent revision"
    );
    assert_eq!(
        repo.get_commit_sha("main"),
        main_sha_before,
        "undo should restore main to its pre-sync sha"
    );
}
