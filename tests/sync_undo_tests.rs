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

// ─── Test 4 ──────────────────────────────────────────────────────────────────

#[test]
fn sync_undo_restores_trunk_after_fast_forward() {
    let repo = TestRepo::new_with_remote();

    // Capture trunk SHA before any remote changes.
    let trunk_sha_before = repo.get_commit_sha("main");

    // Create feature and push it (not on trunk).
    repo.run_stax(&["bc", "feature-ff"]);
    let feature = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature]);

    // Simulate a remote commit on trunk (so sync has to fast-forward trunk).
    repo.simulate_remote_commit("remote-main.txt", "from remote", "Remote main commit");

    // Checkout feature so we're NOT on trunk — this exercises the non-checkout
    // path (bare git update-ref fast-forward), which is where plan_trunk_move is
    // called in that code branch.
    let sync_out = repo.run_stax(&["sync", "--force"]);
    assert!(
        sync_out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&sync_out)
    );

    let trunk_sha_after_sync = repo.get_commit_sha("main");
    assert_ne!(
        trunk_sha_before, trunk_sha_after_sync,
        "trunk should have moved during sync"
    );

    // Undo — should restore trunk to its pre-sync SHA.
    let undo_out = repo.run_stax(&["undo", "--yes"]);
    assert!(
        undo_out.status.success(),
        "undo failed: {}",
        TestRepo::stderr(&undo_out)
    );

    let trunk_sha_after_undo = repo.get_commit_sha("main");
    assert_eq!(
        trunk_sha_before, trunk_sha_after_undo,
        "undo should restore trunk to its pre-sync SHA"
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

    // Return to main so sync can fast-forward it in the non-checkout path.
    repo.run_stax(&["t"]);

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
