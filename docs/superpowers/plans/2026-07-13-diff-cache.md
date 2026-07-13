# Responsive Diff Cache Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make repeated GUI branch selection instant and replace the monolithic diff cache with bounded revision-keyed JSON files.

**Architecture:** `TuiDiffCache` keeps its caller-facing revision-key API but persists each `DiskCachedDiff` independently. `WorkspaceState` adds a small snapshot-scoped LRU that restores a previously displayed branch patch synchronously before background hydration.

**Tech Stack:** Rust, serde_json, fs4 file locks, GPUI state tests, cargo-nextest, Docker-backed `make test`.

## Global Constraints

- Add no runtime dependency.
- Persist entries under `.git/stax/diff-cache/v1` using parent, branch, and merge-base object IDs.
- Cap persistent storage at 128 entries and 100 MiB; cap GUI memory at 32 entries.
- Cache failures degrade to misses and must not replace a successfully calculated diff.
- Full-suite validation must use `make test`.

---

### Task 1: Per-revision persistent files

**Files:**
- Modify: `src/cache.rs`
- Test: `src/cache.rs`
- Test: `tests/application_session_tests.rs`

**Interfaces:**
- Consumes: `TuiDiffCache::key`, `DiskCachedDiff`, `persist_json_atomic`, and `acquire_cache_lock`.
- Produces: unchanged `read_persisted(git_dir, key)` and `insert_persisted(git_dir, key, diff)` APIs backed by independent files.

- [ ] **Step 1: Write failing persistence tests**

Add tests that insert two keys and assert independent JSON paths, place malformed JSON beside a valid requested entry and assert the valid entry still loads, overwrite a malformed requested entry, and invoke a parameterized cleanup helper to verify count and byte limits while ignoring unrelated files.

```rust
#[test]
fn requested_diff_does_not_parse_unrelated_entries() {
    let dir = tempfile::tempdir().unwrap();
    TuiDiffCache::insert_persisted(dir.path(), "v1:a:b:c".into(), disk_diff("kept"))
        .unwrap();
    fs::write(TuiDiffCache::entries_dir(dir.path()).join("broken.json"), b"{").unwrap();
    assert_eq!(
        TuiDiffCache::read_persisted(dir.path(), "v1:a:b:c").unwrap(),
        Some(disk_diff("kept"))
    );
}
```

- [ ] **Step 2: Verify the new tests fail for the aggregate cache**

Run: `cargo nextest run --lib cache::tests::requested_diff_does_not_parse_unrelated_entries`

Expected: FAIL because `entries_dir` and independent entry files do not exist.

- [ ] **Step 3: Implement independent entry paths and bounded cleanup**

Keep revision keys stable for callers and map their colon-separated components to a portable hyphen-separated filename. Read and lock one entry; write one entry atomically; best-effort touch hits, delete malformed requested entries, remove the legacy aggregate file after a successful write, and clean oldest entry files after writes.

```rust
fn entry_path(git_dir: &Path, key: &str) -> PathBuf {
    Self::entries_dir(git_dir).join(format!("{}.json", key.replace(':', "-")))
}

pub(crate) fn read_persisted(git_dir: &Path, key: &str) -> Result<Option<DiskCachedDiff>> {
    let path = Self::entry_path(git_dir, key);
    let _lock = acquire_cache_lock(&path, LockMode::Shared)?;
    match load_json_unlocked(&path) {
        Ok(diff) => Ok(diff),
        Err(error) => {
            let _ = fs::remove_file(&path);
            Err(error)
        }
    }
}
```

- [ ] **Step 4: Run focused cache and application-session tests**

Run: `cargo nextest run --lib cache::tests::`

Run: `cargo nextest run application_session_tests::cached_diff`

Expected: PASS.

---

### Task 2: Snapshot-scoped GUI memory cache

**Files:**
- Modify: `crates/stax-gui/src/state.rs`
- Test: `crates/stax-gui/src/state.rs`
- Test: `crates/stax-gui/src/views/hydration_tests.rs`

**Interfaces:**
- Consumes: `WorkspaceState::select_branch`, `apply_cached_diff`, `apply_diff`, and snapshot refresh transitions.
- Produces: synchronous A → B → A patch restoration with existing generation rejection unchanged.

- [ ] **Step 1: Write failing state tests**

Replace the prior assertion that a different branch always discards the patch with explicit A → B → A reuse, invalidation after snapshot replacement, and 32-entry LRU eviction tests.

```rust
#[test]
fn returning_to_a_visited_branch_restores_its_ready_patch() {
    let mut state = WorkspaceState::new(snapshot(
        "/repo",
        "feature-a",
        &[("feature-a", true), ("feature-b", false)],
    ));
    let (a, _) = state.begin_hydration().unwrap();
    assert!(state.apply_diff(a, Ok(diff("a"))));
    state.select_branch("feature-b").unwrap();
    let (b, _) = state.begin_hydration().unwrap();
    assert!(state.apply_diff(b, Ok(diff("b"))));
    state.select_branch("feature-a").unwrap();
    assert_eq!(state.diff().ready(), Some(&diff("a")));
}
```

- [ ] **Step 2: Verify the reuse test fails**

Run: `cargo nextest run -p stax-gui state::tests::returning_to_a_visited_branch_restores_its_ready_patch`

Expected: FAIL because selecting B currently discards A permanently.

- [ ] **Step 3: Implement the bounded memory LRU**

Add a private cache keyed by branch and parent names. Record accepted cached and fresh results, restore an entry during valid selection, update recency on access, evict past 32, and clear entries when a refreshed snapshot is applied.

```rust
#[derive(Debug, Clone, Default)]
struct SessionDiffCache {
    entries: HashMap<(String, Option<String>), BranchDiff>,
    recency: VecDeque<(String, Option<String>)>,
}
```

- [ ] **Step 4: Run focused GUI tests**

Run: `cargo nextest run -p stax-gui state::tests::`

Run: `cargo nextest run -p stax-gui hydration_tests::`

Expected: PASS with stale-generation behavior unchanged.

---

### Task 3: Verification and publication

**Files:**
- Modify if required: `docs/superpowers/specs/2026-07-13-diff-cache-design.md`
- No user-facing command documentation changes are expected.

**Interfaces:**
- Consumes: completed persistent and memory cache behavior.
- Produces: formatted, linted, fully tested stacked PR.

- [ ] **Step 1: Format and run targeted verification**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets --all-features -- -D warnings`

Expected: PASS.

- [ ] **Step 2: Run the repository-required full suite**

Run: `make test`

Expected: PASS through the Docker test path.

- [ ] **Step 3: Commit and submit**

```bash
git add src/cache.rs crates/stax-gui/src/state.rs tests/application_session_tests.rs \
  docs/superpowers/plans/2026-07-13-diff-cache.md
git commit -m "perf(gui): reuse revision-keyed branch diffs"
stax submit
```

- [ ] **Step 4: Set a complete PR body**

Include the measured 20.3 MB aggregate-cache cause, per-entry persistence, in-memory A → B → A behavior, corruption/cleanup handling, exact tests run, and a one-line documentation-impact note.
