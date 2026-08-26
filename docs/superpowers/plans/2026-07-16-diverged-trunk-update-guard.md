# Diverged Trunk Update Guard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent `stax update` and `stax sync --restack` from rewriting feature branches when local trunk did not reach the fetched remote trunk.

**Architecture:** Add fail-closed checks inside `commands::sync::run` after fetch, after trunk update but before feature-ref cleanup, and immediately before optional restack. Reuse a small trunk/ref comparison helper for the OID checks and final sync reporting; let the existing `?` in `commands::refresh::run` prevent submission after a guard fails.

**Tech Stack:** Rust, real-Git integration fixtures, Cargo test suite.

## Global Constraints

- Plain `stax sync` without `--restack` retains its existing warning-only behavior.
- Divergent local trunk commits are never reset, rebased, merged, or pushed automatically.
- Any automatic stash is restored before the guard returns an error.
- A failed fetch cannot be treated as fresh merely because cached remote-tracking refs match local trunk.
- The first trunk OID guard runs before imported refresh and merged cleanup can mutate feature refs.
- The fix covers both `stax update` and direct `stax sync --restack` through their shared sync path.
- No unrelated sync, restack, submit, or reporting refactors.

---

### Task 1: Fail Closed Before Restack

**Files:**
- Modify: `tests/integration_tests.rs` in the Refresh Tests section
- Modify: `src/commands/sync.rs` near the optional restack phase and final trunk reporting

**Interfaces:**
- Consumes: `resolve_ref_oid(workdir: &Path, reference: &str) -> Option<String>`, `remote_trunk_after_fetch: Option<String>`, `repo.stash_pop() -> Result<()>`
- Produces: `trunk_reached_remote(workdir: &Path, trunk: &str, remote_oid: Option<&str>) -> bool`

- [ ] **Step 1: Write the failing integration test**

Add this test after `test_refresh_no_submit_keeps_original_branch_and_restacks_stack`:

```rust
#[test]
fn test_update_aborts_before_restack_when_trunk_diverges() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "diverged-update"]);
    let feature = repo.current_branch();
    repo.create_file("feature.txt", "feature\n");
    repo.commit("Feature commit");
    let push = repo.git(&["push", "-u", "origin", &feature]);
    assert!(push.status.success(), "failed to seed feature remote");

    let feature_before = repo.get_commit_sha(&feature);
    let remote_path = repo.remote_path().expect("No remote configured");
    let remote_ref = format!("refs/heads/{feature}");
    let remote_before = TestRepo::stdout(&repo.git_in(
        &remote_path,
        &["rev-parse", &remote_ref],
    ))
    .trim()
    .to_string();

    repo.run_stax(&["checkout", "main"]);
    repo.create_file("local-main.txt", "local main\n");
    repo.commit("Local main commit");
    let local_main_before = repo.get_commit_sha("main");
    repo.simulate_remote_commit("remote-main.txt", "remote main\n", "Remote main commit");
    configure_submit_remote(&repo);
    repo.run_stax(&["checkout", &feature]);

    let output = repo.run_stax(&[
        "update",
        "--no-pr",
        "--force",
        "--yes",
        "--no-prompt",
    ]);

    assert!(
        !output.status.success(),
        "update must fail when trunk diverges\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output),
    );
    let diagnostic = format!(
        "{}{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output),
    );
    assert!(diagnostic.contains("Cannot restack because main did not reach origin/main"));
    assert!(diagnostic.contains("Inspect and reconcile main with origin/main, then retry"));
    assert_eq!(repo.get_commit_sha(&feature), feature_before);
    assert_eq!(repo.get_commit_sha("main"), local_main_before);

    let remote_after = TestRepo::stdout(&repo.git_in(
        &remote_path,
        &["rev-parse", &remote_ref],
    ))
    .trim()
    .to_string();
    assert_eq!(remote_after, remote_before);
    assert!(!repo.path().join(".git/rebase-merge").exists());
    assert!(!repo.path().join(".git/rebase-apply").exists());
}
```

- [ ] **Step 2: Run the regression test and verify RED**

Run the targeted regression with:

```bash
cargo nextest run test_update_aborts_before_restack_when_trunk_diverges
```

Expected: FAIL because current `stax update` succeeds, rebases the feature branch onto divergent local trunk, and submits the rewritten branch.

- [ ] **Step 3: Add the shared trunk comparison helper**

In `src/commands/sync.rs`, add:

```rust
fn trunk_reached_remote(workdir: &Path, trunk: &str, remote_oid: Option<&str>) -> bool {
    remote_oid.is_some_and(|remote| {
        resolve_ref_oid(workdir, trunk).as_deref() == Some(remote)
    })
}
```

- [ ] **Step 4: Guard every pre-restack mutation boundary**

Capture whether fetch succeeded. For restack requests, restore any auto-stash and return an actionable error immediately after a failed fetch. After the trunk-update attempt, compare the local trunk with the fixed fetched remote OID and fail before imported refresh or merged cleanup when they differ. Retain the same OID check immediately before the restack loop as a second boundary. Reuse `trunk_reached_remote` for final reporting and a small stash-restoration helper for every error path.

- [ ] **Step 5: Run the regression test and verify GREEN**

Run:

```bash
cargo nextest run test_update_aborts_before_restack_when_trunk_diverges
```

Expected: PASS. The command exits non-zero before rebase or submit, and all recorded refs remain unchanged.

- [ ] **Step 6: Run focused update and sync coverage**

Run:

```bash
cargo nextest run refresh
cargo nextest run commands::sync::tests
```

Expected: all matching tests pass.

- [ ] **Step 7: Run repository verification**

Run:

```bash
make lint
make test
git diff --check
```

Expected: lint and the full test suite pass.

- [ ] **Step 8: Commit with Stax**

Run:

```bash
stax modify -a -m "fix: abort restack when trunk diverges"
```

Expected: the tracked branch contains one focused commit with the design, plan, regression test, and fix.

- [ ] **Step 9: Submit and create the PR with Stax**

Run:

```bash
stax submit --publish --yes --no-prompt
```

Expected: Stax pushes `cesar/fix-update-diverged-trunk`, creates a PR targeting `main`, and reports its URL.
