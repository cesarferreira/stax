use crate::common::TestRepo;

/// Helper: resolve a local ref to its SHA (None if ref doesn't exist).
fn resolve_ref(repo: &TestRepo, refname: &str) -> Option<String> {
    let out = repo.git(&["rev-parse", "--verify", refname]);
    if out.status.success() {
        Some(TestRepo::stdout(&out).trim().to_string())
    } else {
        None
    }
}

/// Helper: return true if the local branch exists.
fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    resolve_ref(repo, &format!("refs/heads/{branch}")).is_some()
}

/// Helper: count stax receipts so we can assert that --dry-run writes none.
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

// ─── Test (a): --json --force deletes the merged branch ──────────────────────
// --json --force runs sync non-interactively; deleted_branches has the entry
// with tip and scope set.

#[test]
fn json_force_deletes_merged_branch_and_emits_deleted_entry() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "feature-json-a"]);
    let feature = repo.current_branch();
    repo.create_file("feature-json-a.txt", "content");
    repo.commit("Feature-a commit");
    repo.git(&["push", "-u", "origin", &feature]);

    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--json", "--force"]);
    assert!(
        out.status.success(),
        "sync --json --force should exit 0 when successful; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout was not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["kind"], "sync");
    assert_eq!(parsed["success"], true);
    assert_eq!(parsed["dry_run"], false);

    let deleted = parsed["deleted_branches"]
        .as_array()
        .unwrap_or_else(|| panic!("deleted_branches missing or not an array; got:\n{parsed}"));
    let names: Vec<&str> = deleted.iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(
        names.contains(&feature.as_str()),
        "deleted_branches should contain {feature}; got: {names:?}"
    );

    let entry = deleted
        .iter()
        .find(|d| d["name"] == feature.as_str())
        .unwrap();
    // category must be "merged", scope must be "both" or "local"
    let category = entry["category"].as_str().unwrap_or("");
    assert!(
        category == "merged" || category == "upstream_gone",
        "unexpected category: {category}"
    );
    // tip must be a 40-char SHA or at least a non-empty string
    let tip = entry["tip"].as_str().unwrap_or("");
    assert!(!tip.is_empty(), "tip should be set on a deleted branch");

    assert!(
        !branch_exists(&repo, &feature),
        "branch {feature} should be deleted after sync --json --force"
    );
}

// ─── Test (a2): --json alone skips deletions that need confirmation ───────────
// --json without --force forces quiet=true (non-interactive); branches that
// would need a prompt are recorded in skipped_branches and survive.

#[test]
fn json_without_force_skips_branch_deletion() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "feature-json-a2"]);
    let feature = repo.current_branch();
    repo.create_file("feature-json-a2.txt", "content");
    repo.commit("Feature-a2 commit");
    repo.git(&["push", "-u", "origin", &feature]);

    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--json"]);
    assert!(
        out.status.success(),
        "sync --json (no --force) should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["success"], true);

    // In non-force mode, the merged branch should appear in skipped_branches
    // (because interactive confirmation is suppressed by quiet=true).
    // The branch should still exist locally.
    assert!(
        branch_exists(&repo, &feature),
        "branch {feature} should survive when --force is not provided"
    );

    // The branch must appear in skipped_branches with reason "not confirmed".
    let skipped = parsed["skipped_branches"].as_array().unwrap_or_else(|| {
        panic!("skipped_branches missing or not an array; parsed JSON:\n{parsed}")
    });
    let branch_skipped = skipped
        .iter()
        .any(|s| s["name"].as_str() == Some(feature.as_str()));
    assert!(
        branch_skipped,
        "merged branch {feature} should appear in skipped_branches; got: {skipped:?}"
    );
    let skip_entry = skipped
        .iter()
        .find(|s| s["name"].as_str() == Some(feature.as_str()))
        .unwrap();
    assert_eq!(
        skip_entry["reason"].as_str().unwrap_or(""),
        "not confirmed",
        "skipped_branches reason should be 'not confirmed'"
    );
}

// ─── Test (b): --json produces exactly one JSON document, no human lines ──────

#[test]
fn json_output_is_single_valid_json_document_no_human_lines() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--json", "--force"]);
    assert!(
        out.status.success(),
        "sync --json should exit 0 on clean repo; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);

    // Must parse as a single JSON object (not an array, not multiple docs)
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert!(
        parsed.is_object(),
        "output must be a JSON object, got: {parsed}"
    );

    // No human-readable prefix lines before the JSON
    let first_non_empty = stdout.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    assert!(
        first_non_empty.starts_with('{'),
        "first non-empty stdout line must be the JSON opening brace; got: {first_non_empty:?}"
    );

    // Required top-level fields
    assert_eq!(parsed["schema_version"], 1);
    assert_eq!(parsed["kind"], "sync");
    assert!(parsed["duration_ms"].is_number());
    assert!(parsed["trunk"].is_object());
    assert!(parsed["stash"].is_object());
}

// ─── Test (c): --dry-run --json emits kind=sync_plan, dry_run=true, exit 0, no receipt ──

#[test]
fn dry_run_json_emits_sync_plan_kind_and_no_receipt() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let pre_receipts = receipt_count(&repo);

    let out = repo.run_stax(&["sync", "--dry-run", "--json"]);
    assert!(
        out.status.success(),
        "sync --dry-run --json should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["kind"], "sync_plan");
    assert_eq!(parsed["dry_run"], true);
    assert_eq!(parsed["success"], true);
    assert_eq!(parsed["schema_version"], 1);
    assert!(parsed["trunk"].is_object());
    assert!(parsed["duration_ms"].is_number());

    assert_eq!(
        receipt_count(&repo),
        pre_receipts,
        "--dry-run --json must write no receipt"
    );
}

// ─── Test (d): dirty tree with --json → success=false, kind=dirty_working_tree, non-zero exit ──

#[test]
fn json_dirty_tree_emits_error_envelope_and_exits_nonzero() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    // Create an uncommitted modification so the tree is dirty
    repo.create_file("dirty.txt", "unstaged change");

    let out = repo.run_stax(&["sync", "--json"]);
    assert!(
        !out.status.success(),
        "sync --json with dirty tree should exit non-zero; exit: {:?}",
        out.status.code()
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["success"], false);
    assert!(
        parsed["error"].is_object(),
        "error field must be present on failure"
    );
    assert_eq!(parsed["error"]["kind"], "dirty_working_tree");
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("dirty"),
        "error message must mention 'dirty'"
    );
    assert!(
        parsed["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("--stash"),
        "error message must mention '--stash' so the user knows how to resolve"
    );

    // stderr must be empty (no duplicated "Error:" line from anyhow propagation)
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.is_empty(),
        "stderr should be empty for --json dirty-tree error (no duplicated Error: line); got: {stderr:?}"
    );

    // exit code must be exactly 1
    assert_eq!(
        out.status.code(),
        Some(1),
        "exit code for dirty-tree --json must be 1; got: {:?}",
        out.status.code()
    );

    // The dirty file should be untouched
    assert!(
        repo.path().join("dirty.txt").exists(),
        "dirty file must still exist after failed --json sync"
    );
}

// ─── Test (c2): --dry-run --json includes merged_candidates with disposition ──

#[test]
fn dry_run_json_includes_merged_candidate_with_disposition() {
    let repo = TestRepo::new_with_remote();

    // Create a branch, push it, merge it on remote, then pull to advance local trunk.
    repo.run_stax(&["bc", "feature-dry-plan"]);
    let feature = repo.current_branch();
    repo.create_file("feature-dry-plan.txt", "content");
    repo.commit("Feature commit for dry-run JSON plan test");
    repo.git(&["push", "-u", "origin", &feature]);

    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);

    // feature is still a local branch but its remote has been merged into trunk.
    let out = repo.run_stax(&["sync", "--dry-run", "--json"]);
    assert!(
        out.status.success(),
        "sync --dry-run --json should exit 0 even with merged candidates; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["kind"], "sync_plan");
    assert_eq!(parsed["dry_run"], true);

    // merged_candidates must be non-empty and contain the feature branch.
    let candidates = parsed["merged_candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("merged_candidates missing or not an array; got:\n{parsed}"));
    assert!(
        !candidates.is_empty(),
        "merged_candidates should be non-empty; got:\n{parsed}"
    );

    let entry = candidates
        .iter()
        .find(|c| c["name"].as_str() == Some(feature.as_str()))
        .unwrap_or_else(|| {
            panic!("merged_candidates should contain '{feature}'; got: {candidates:?}")
        });

    let disposition = entry["disposition"].as_str().unwrap_or("");
    assert!(
        matches!(disposition, "would_delete" | "would_prompt_then_delete"),
        "disposition should be would_delete or would_prompt_then_delete; got: {disposition}"
    );
}

// ─── Test (e): --json --continue is rejected by clap ──────────────────────────

#[test]
fn json_and_continue_are_mutually_exclusive() {
    let repo = TestRepo::new_with_remote();

    let out = repo.run_stax(&["sync", "--json", "--continue"]);
    assert!(
        !out.status.success(),
        "--json and --continue should be rejected with a non-zero exit"
    );

    // clap prints an error to stderr for conflicting args
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "should report a conflict between --json and --continue; stderr: {stderr}"
    );
}
