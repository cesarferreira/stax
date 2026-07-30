use crate::common::TestRepo;

/// Helper: count entries in `git stash list`.
fn stash_count(repo: &TestRepo) -> usize {
    let out = repo.git(&["stash", "list"]);
    TestRepo::stdout(&out)
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

// ─── Test 1: --stash auto-stashes a dirty working tree ───────────────────────

#[test]
fn stash_flag_auto_stashes_dirty_tree_and_syncs() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    // Dirty the working tree without staging
    repo.create_file("dirty.txt", "unstaged change");

    let before = stash_count(&repo);

    let out = repo.run_stax(&["sync", "--stash", "--force"]);
    assert!(
        out.status.success(),
        "sync --stash should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // --stash is a stash-then-restore round trip: the stash list returns to
    // baseline and stdout shows both halves of the trip.
    assert_eq!(
        stash_count(&repo),
        before,
        "sync --stash must pop its stash on success"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Stashed working tree changes."),
        "expected stash confirmation in stdout: {stdout}"
    );
    assert!(
        stdout.contains("Restored stashed changes."),
        "expected stash restore confirmation in stdout: {stdout}"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("dirty.txt")).unwrap(),
        "unstaged change",
        "dirty file content must survive the stash round trip"
    );
}

// ─── Test 2: --stash works in --quiet mode ────────────────────────────────────

#[test]
fn stash_flag_works_in_quiet_mode() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.create_file("quiet-dirty.txt", "unstaged change");

    let before = stash_count(&repo);

    let out = repo.run_stax(&["sync", "--stash", "--quiet", "--force"]);
    assert!(
        out.status.success(),
        "sync --stash --quiet should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // Quiet mode prints nothing; exit 0 alone proves the Always arm ran
    // (Prompt + --quiet would bail with DirtyWorkingTree and exit 1).
    assert_eq!(
        stash_count(&repo),
        before,
        "sync --stash --quiet must pop its stash on success"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("quiet-dirty.txt")).unwrap(),
        "unstaged change",
        "dirty file content must survive the stash round trip"
    );
}

// ─── Test 3: --stash with --json shows stash outcome in JSON ─────────────────

#[test]
fn stash_flag_with_json_reports_stash_outcome() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.create_file("json-dirty.txt", "unstaged change");

    let out = repo.run_stax(&["sync", "--stash", "--json"]);
    assert!(
        out.status.success(),
        "sync --stash --json should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["success"], true);
    assert_eq!(
        parsed["stash"]["stashed"], true,
        "stash.stashed must be true when --stash auto-stashed"
    );
}

// ─── Test 4: --no-stash bails even with --force ──────────────────────────────

#[test]
fn no_stash_bails_even_with_force_flag() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.create_file("force-dirty.txt", "unstaged change");

    let before = stash_count(&repo);

    let out = repo.run_stax(&["sync", "--no-stash", "--force"]);
    assert!(
        !out.status.success(),
        "sync --no-stash --force should exit non-zero when dirty"
    );

    // Nothing should have been stashed
    assert_eq!(
        stash_count(&repo),
        before,
        "sync --no-stash must not create any stash entry"
    );
}

// ─── Test 5: --no-stash with --json emits dirty_working_tree JSON ────────────

#[test]
fn no_stash_with_json_emits_dirty_working_tree_error() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    repo.create_file("json-no-stash-dirty.txt", "unstaged change");

    let out = repo.run_stax(&["sync", "--no-stash", "--json"]);
    assert!(
        !out.status.success(),
        "sync --no-stash --json should exit non-zero when dirty"
    );

    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));

    assert_eq!(parsed["success"], false);
    assert_eq!(
        parsed["error"]["kind"], "dirty_working_tree",
        "error kind must be dirty_working_tree; got: {}",
        parsed["error"]["kind"]
    );
}

// ─── Test 6: --stash and --no-stash together are rejected by clap ─────────────

#[test]
fn stash_and_no_stash_conflict_exits_nonzero() {
    let repo = TestRepo::new_with_remote();

    let out = repo.run_stax(&["sync", "--stash", "--no-stash"]);
    assert!(
        !out.status.success(),
        "--stash and --no-stash together should be rejected with non-zero exit"
    );

    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "should report a conflict between --stash and --no-stash; stderr: {stderr}"
    );

    // clap typically uses exit code 2 for usage errors
    assert_eq!(
        out.status.code(),
        Some(2),
        "exit code for flag conflict must be 2; got: {:?}",
        out.status.code()
    );
}

// ─── Test 7: --prune emits a deprecation warning on stderr ───────────────────

#[test]
fn prune_flag_emits_deprecation_warning_on_stderr() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--prune", "--force"]);
    assert!(
        out.status.success(),
        "sync --prune should still exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("--prune") && stderr.contains("deprecated"),
        "stderr must contain a deprecation warning mentioning --prune; got: {stderr:?}"
    );
    assert!(
        stderr.contains("--full"),
        "deprecation warning must suggest --full; got: {stderr:?}"
    );
}

// ─── Test 8: --prune deprecation warning appears even with --json ─────────────

#[test]
fn prune_flag_deprecation_warning_appears_with_json() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--prune", "--json", "--force"]);
    assert!(
        out.status.success(),
        "sync --prune --json should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    // Warning must be on stderr (JSON stays on stdout)
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("--prune") && stderr.contains("deprecated"),
        "stderr must contain --prune deprecation warning even with --json; got: {stderr:?}"
    );

    // stdout must still be valid JSON
    let stdout = TestRepo::stdout(&out);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout not valid JSON: {e}\n---\n{stdout}"));
    assert_eq!(parsed["kind"], "sync");
}

// ─── Test 9: --dry-run warns that --stash is ignored ─────────────────────────

#[test]
fn dry_run_warns_stash_flags_are_ignored() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);

    let out = repo.run_stax(&["sync", "--dry-run", "--stash"]);
    assert!(
        out.status.success(),
        "sync --dry-run --stash should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );

    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("--stash") && stderr.contains("ignored"),
        "stderr must warn that --stash is ignored by --dry-run; got: {stderr:?}"
    );
}
