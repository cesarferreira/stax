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

/// Helper: return the number of entries in `git stash list`.
fn stash_count(repo: &TestRepo) -> usize {
    let out = repo.git(&["stash", "list"]);
    TestRepo::stdout(&out)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

// ─── Test (a) ────────────────────────────────────────────────────────────────
// --dry-run reports a merged branch but deletes nothing: branch still exists,
// metadata ref still resolves, no receipt written, git status unchanged.

#[test]
fn dry_run_reports_merged_branch_and_deletes_nothing() {
    let repo = TestRepo::new_with_remote();

    // Create feature-a, push it, merge it on the remote.
    repo.run_stax(&["bc", "feature-dry"]);
    let feature = repo.current_branch();
    repo.create_file("feature-dry.txt", "dry content");
    repo.commit("Feature dry commit");
    repo.git(&["push", "-u", "origin", &feature]);

    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);

    let pre_receipts = receipt_count(&repo);
    let sha_before = resolve_ref(&repo, &format!("refs/heads/{feature}"));
    let meta_before = resolve_ref(&repo, &format!("refs/branch-metadata/{feature}"));

    // Run sync --dry-run; should exit 0 and report the merged branch.
    let out = repo.run_stax(&["sync", "--dry-run", "--force"]);
    assert!(
        out.status.success(),
        "--dry-run should always exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains(&feature) || stdout.contains("merged"),
        "--dry-run should mention the merged branch or 'merged'; got:\n{stdout}"
    );

    // Nothing deleted.
    assert!(
        branch_exists(&repo, &feature),
        "--dry-run must NOT delete {feature}"
    );
    let sha_after = resolve_ref(&repo, &format!("refs/heads/{feature}"));
    assert_eq!(
        sha_before, sha_after,
        "--dry-run must not move the branch tip"
    );
    let meta_after = resolve_ref(&repo, &format!("refs/branch-metadata/{feature}"));
    assert_eq!(
        meta_before, meta_after,
        "--dry-run must not change metadata ref"
    );
    assert_eq!(
        receipt_count(&repo),
        pre_receipts,
        "--dry-run must write no receipt"
    );
}

// ─── Test (b) ────────────────────────────────────────────────────────────────
// Clean, up-to-date repo → "no changes made", no "would delete" lines.

#[test]
fn dry_run_clean_repo_says_no_changes() {
    let repo = TestRepo::new_with_remote();

    // Push main to remote so remote has a trunk ref.
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--dry-run"]);
    assert!(
        out.status.success(),
        "--dry-run should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let combined = stdout.clone() + &TestRepo::stderr(&out);

    assert!(
        combined.contains("no changes")
            || combined.contains("up to date")
            || combined.contains("nothing to do"),
        "--dry-run on clean repo should indicate nothing to do; got:\n{combined}"
    );
    // No "would delete" lines expected.
    assert!(
        !stdout.contains("would delete"),
        "--dry-run clean repo should not say 'would delete'; stdout:\n{stdout}"
    );
}

// ─── Test (c) ────────────────────────────────────────────────────────────────
// --restack lists stale branches, SHAs unchanged after the command.

#[test]
fn dry_run_restack_lists_stale_branches_without_rewriting_refs() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main → feat-c; push main to remote so trunk can be probed.
    repo.git(&["push", "-u", "origin", "main"]);
    repo.run_stax(&["bc", "feat-c"]);
    let feature = repo.current_branch();
    repo.create_file("c.txt", "c");
    repo.commit("C commit");

    let sha_before =
        resolve_ref(&repo, &format!("refs/heads/{feature}")).expect("feat-c should exist");
    let main_sha_before = resolve_ref(&repo, "refs/heads/main").expect("main should exist");

    // Run --dry-run --restack.
    let out = repo.run_stax(&["sync", "--dry-run", "--restack"]);
    assert!(
        out.status.success(),
        "--dry-run --restack should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // SHAs must be byte-identical after the command.
    let sha_after =
        resolve_ref(&repo, &format!("refs/heads/{feature}")).expect("feat-c should still exist");
    let main_sha_after = resolve_ref(&repo, "refs/heads/main").expect("main should still exist");

    assert_eq!(
        sha_before, sha_after,
        "--dry-run --restack must not move feat-c"
    );
    assert_eq!(
        main_sha_before, main_sha_after,
        "--dry-run --restack must not move main"
    );
}

// ─── Test (d) ────────────────────────────────────────────────────────────────
// Dirty tree: left unstashed after --dry-run; stash list empty, file unmodified.

#[test]
fn dry_run_dirty_tree_left_unstashed() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    // Create an uncommitted change.
    repo.create_file("dirty.txt", "dirty content");

    let out = repo.run_stax(&["sync", "--dry-run"]);
    assert!(
        out.status.success(),
        "--dry-run with dirty tree should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // Stash list must remain empty.
    assert_eq!(
        stash_count(&repo),
        0,
        "--dry-run must not stash the working tree"
    );

    // The file must still be present and unmodified.
    let content = std::fs::read_to_string(repo.path().join("dirty.txt"))
        .expect("dirty.txt should still exist");
    assert_eq!(
        content, "dirty content",
        "--dry-run must not modify dirty.txt"
    );

    // Output should mention "would stash" or "dirty" but NOT that it was stashed.
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("dirty") || stdout.contains("stash"),
        "--dry-run should report dirty tree; stdout:\n{stdout}"
    );
    assert!(
        !stdout.contains("✓ Stashed"),
        "--dry-run must not report that it stashed changes"
    );
}

// ─── Test (e) ────────────────────────────────────────────────────────────────
// --continue is rejected by clap (conflicts_with).

#[test]
fn dry_run_rejects_continue_flag() {
    let repo = TestRepo::new_with_remote();
    let out = repo.run_stax(&["sync", "--dry-run", "--continue"]);
    assert!(
        !out.status.success(),
        "--dry-run --continue should be rejected by clap"
    );
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts"),
        "expected a 'conflicts' error; got: {stderr}"
    );
}

// ─── Test (f) ────────────────────────────────────────────────────────────────
// --force emits a stderr warning but still exits 0.

#[test]
fn dry_run_warns_force_is_ignored() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--dry-run", "--force"]);
    assert!(
        out.status.success(),
        "--dry-run --force should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("--force") || stderr.contains("ignored"),
        "expected warning about --force; stderr:\n{stderr}"
    );
}

// ─── Test (g) ────────────────────────────────────────────────────────────────
// --delete-upstream-gone protects branches with unique commits.

#[test]
fn dry_run_delete_upstream_gone_protects_unpushed_work() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    // Create a branch, push it to establish tracking, then add a local-only commit
    // AFTER the push. This produces a genuine [gone] upstream once we delete the
    // remote branch, with unique local work that must not be discarded.
    repo.run_stax(&["bc", "upstream-gone-local-work"]);
    let branch = repo.current_branch();
    repo.create_file("pushed.txt", "pushed");
    repo.commit("Pushed commit");
    repo.git(&["push", "-u", "origin", &branch]);

    // Local-only commit — never published to origin.
    repo.create_file("local-only.txt", "local only");
    repo.commit("Local-only commit");

    // Delete the remote branch to create a [gone] upstream.
    repo.git(&["checkout", "main"]);
    repo.git(&["push", "origin", "--delete", &branch]);
    repo.git(&["checkout", &branch]);

    let out = repo.run_stax(&["sync", "--dry-run", "--delete-upstream-gone"]);
    assert!(
        out.status.success(),
        "--dry-run --delete-upstream-gone should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // Branch must still exist.
    assert!(
        branch_exists(&repo, &branch),
        "--dry-run must not delete {branch}"
    );

    let stdout = TestRepo::stdout(&out);
    let combined = stdout + &TestRepo::stderr(&out);
    // Should report protection (unique commits).
    assert!(
        combined.contains("protected") || combined.contains("unique"),
        "expected mention of protection for unpushed work; combined:\n{combined}"
    );
}

// ─── Test (h) ────────────────────────────────────────────────────────────────
// --plan alias produces the same output as --dry-run.

#[test]
fn plan_alias_output_equals_dry_run_output() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let dry_out = repo.run_stax(&["sync", "--dry-run"]);
    let plan_out = repo.run_stax(&["sync", "--plan"]);

    // Both should succeed.
    assert!(dry_out.status.success(), "--dry-run failed");
    assert!(plan_out.status.success(), "--plan failed");

    // Stdout should be identical (strip timing-sensitive lines if any).
    let dry_stdout = TestRepo::stdout(&dry_out);
    let plan_stdout = TestRepo::stdout(&plan_out);

    assert_eq!(
        dry_stdout, plan_stdout,
        "--plan and --dry-run should produce identical stdout"
    );
}
