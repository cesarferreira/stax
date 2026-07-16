# Preserve Sync Worktrees Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `stax sync` preserve branch-owning worktrees by default while retaining explicit interactive removal and safe skip behavior.

**Architecture:** Add a sync-local deletion action model so confirmation is separate from execution. Reuse `BranchDeleteResolution` to switch an affected worktree to trunk when possible and fall back to detached `HEAD`; execute this preservation before branch/metadata/remote deletion, and fail closed if the worktree cannot be freed.

**Tech Stack:** Rust, `dialoguer::Select`, Git worktrees, consolidated nextest integration suite.

## Global Constraints

- `stax sync --force` always preserves linked worktrees; force controls prompting and never authorizes removal.
- Preservation may switch to trunk or detach `HEAD`, but must not reset, clean, stash, or delete files.
- A failed preservation leaves the local branch, remote branch, Stax metadata, and worktree intact.
- Explicit removal continues through existing removal hooks, blocker checks, and warm-slot behavior.
- User-visible behavior must be documented in `docs/workflows/multi-worktree.md` and `skills.md`.
- Full-suite verification uses `make test`; lint verification uses `make lint`.

---

### Task 1: Preserve linked worktrees during forced sync cleanup

**Files:**
- Modify: `tests/integration_tests.rs`
- Modify: `src/git/repo.rs`
- Modify: `src/commands/sync.rs`

**Interfaces:**
- Produces: `GitRepo::switch_worktree_for_branch_delete(&BranchDeleteResolution) -> Result<BranchDeleteSwitchTarget>`.
- Produces: `SyncBranchDeleteAction::{DeleteOnly, PreserveWorktree, RemoveWorktree { force: bool }, Skip}` for cleanup planning and execution.
- Consumes: existing `BranchDeleteResolution`, `BranchDeleteSwitchTarget`, and `attempt_local_branch_delete`.

- [x] **Step 1: Write the failing forced-sync integration test**

Add a test next to `test_sync_deletes_merged_branches` that creates and pushes a tracked feature branch, adds a linked worktree for it, writes `.gitignore` plus a worktree-local `.env`, merges the branch remotely, and runs sync from main:

```rust
#[test]
fn test_sync_force_preserves_worktree_for_merged_branch() {
    let repo = TestRepo::new_with_remote();
    repo.run_stax(&["bc", "preserved-worktree"]);
    let branch = repo.current_branch();
    repo.create_file("feature.txt", "feature\n");
    repo.create_file(".gitignore", ".env\n");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch]);

    repo.run_stax(&["t"]);
    let worktree_root = test_tempdir();
    let worktree = worktree_root.path().join("preserved-worktree");
    assert!(repo.git(&["worktree", "add", worktree.to_str().unwrap(), &branch]).status.success());
    fs::write(worktree.join(".env"), "TOKEN=local\n").unwrap();
    repo.merge_branch_on_remote(&branch);

    let output = repo.run_stax(&["sync", "--force"]);
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));
    assert!(worktree.exists());
    assert_eq!(fs::read_to_string(worktree.join(".env")).unwrap(), "TOKEN=local\n");
    assert!(!repo.list_branches().contains(&branch));
    assert_eq!(TestRepo::stdout(&repo.git_in(&worktree, &["rev-parse", "--abbrev-ref", "HEAD"])).trim(), "HEAD");
}
```

- [x] **Step 2: Run the test and verify the destructive behavior fails it**

Run: `cargo nextest run integration_tests::test_sync_force_preserves_worktree_for_merged_branch`

Expected: FAIL because the linked worktree is removed or the merged branch remains checked out and cannot be deleted.

- [x] **Step 3: Add the switch-or-detach repository helper**

Import `BranchDeleteSwitchTarget` in sync and add this method beside `branch_delete_resolution` in `src/git/repo.rs`:

```rust
pub fn switch_worktree_for_branch_delete(
    &self,
    resolution: &BranchDeleteResolution,
) -> Result<BranchDeleteSwitchTarget> {
    if let BranchDeleteSwitchTarget::Branch(target) = &resolution.switch_target
        && self.switch_branch_in(&resolution.worktree.path, target).is_ok()
    {
        return Ok(BranchDeleteSwitchTarget::Branch(target.clone()));
    }

    let output = self.run_git(&resolution.worktree.path, &["switch", "--detach"])?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        anyhow::bail!(
            "git switch --detach failed in '{}': {}",
            resolution.worktree.path.display(),
            stderr
        );
    }
    Ok(BranchDeleteSwitchTarget::Detach)
}
```

- [x] **Step 4: Add the cleanup action model and forced preservation path**

In `src/commands/sync.rs`, add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncBranchDeleteAction {
    DeleteOnly,
    PreserveWorktree,
    RemoveWorktree { force: bool },
    Skip,
}
```

Store this action in both merged and upstream-gone deletion decisions. For a non-current branch with a worktree, resolve `--force` to `PreserveWorktree` and quiet non-forced execution to `Skip`. Before child rebasing/reparenting, execute preservation:

```rust
if action == SyncBranchDeleteAction::PreserveWorktree {
    let Some(cleanup) = blocking_worktree_cleanup.as_ref() else {
        stats.record_cleanup_skip(branch, "worktree resolution missing");
        continue;
    };
    match repo.switch_worktree_for_branch_delete(&cleanup.resolution) {
        Ok(target) => {
            if !quiet {
                let destination = match target {
                    BranchDeleteSwitchTarget::Branch(target) => format!("switched to {target}"),
                    BranchDeleteSwitchTarget::Detach => "detached HEAD".to_string(),
                };
                println!("    {} kept linked worktree {} ({})", "→".cyan(), cleanup.resolution.worktree.name.cyan(), destination);
            }
        }
        Err(error) => {
            stats.record_cleanup_skip(branch, format!("couldn't preserve linked worktree: {error}"));
            if !quiet {
                println!("    {} couldn't preserve linked worktree '{}': {}", "↷".yellow(), cleanup.resolution.worktree.name, error);
            }
            continue;
        }
    }
}
```

Pass `RemoveWorktree { force }` to the existing removal helper only for an explicit remove decision. After `attempt_local_branch_delete`, stop processing that candidate when local deletion fails so remote deletion and metadata cleanup do not run.

- [x] **Step 5: Run the focused test and sync unit tests**

Run: `cargo nextest run integration_tests::test_sync_force_preserves_worktree_for_merged_branch commands::sync::tests`

Expected: PASS with the linked worktree present, detached, and `.env` preserved.

- [x] **Step 6: Amend the stax branch commit**

Run: `stax modify -a -m "feat: preserve worktrees during sync cleanup"`

Expected: the current `cesar/preserve-sync-worktrees` commit is amended with Task 1.

---

### Task 2: Add the interactive preserve/remove/skip menu and fail-closed coverage

**Files:**
- Modify: `src/commands/sync.rs`
- Modify: `tests/integration_tests.rs`

**Interfaces:**
- Consumes: `SyncBranchDeleteAction` and `BlockingWorktreeCleanup` from Task 1.
- Produces: `linked_worktree_delete_options(&BlockingWorktreeCleanup) -> Vec<(String, SyncBranchDeleteAction)>`.
- Produces: `choose_linked_worktree_delete_action(...) -> Result<SyncBranchDeleteAction>`.

- [x] **Step 1: Replace obsolete prompt unit tests with failing menu-option tests**

Replace the linked-worktree `sync_delete_prompt` tests in `src/commands/sync.rs` with assertions for ordered default-safe choices:

```rust
#[test]
fn linked_worktree_delete_options_default_to_preserve() {
    let options = linked_worktree_delete_options(&linked_worktree_cleanup(&[]));
    assert_eq!(options[0].1, SyncBranchDeleteAction::PreserveWorktree);
    assert!(options[0].0.contains("Keep worktree"));
    assert_eq!(options[1].1, SyncBranchDeleteAction::RemoveWorktree { force: false });
    assert_eq!(options.last().unwrap().1, SyncBranchDeleteAction::Skip);
}

#[test]
fn linked_worktree_delete_options_label_dirty_removal_as_destructive() {
    let options = linked_worktree_delete_options(&linked_worktree_cleanup(&["dirty"]));
    assert_eq!(options[1].1, SyncBranchDeleteAction::RemoveWorktree { force: true });
    assert!(options[1].0.contains("Force-remove dirty worktree"));
}

#[test]
fn linked_worktree_delete_options_omit_remove_for_locked_worktree() {
    let options = linked_worktree_delete_options(&linked_worktree_cleanup(&["locked"]));
    assert_eq!(options.len(), 2);
    assert!(options.iter().all(|(_, action)| !matches!(action, SyncBranchDeleteAction::RemoveWorktree { .. })));
}

#[test]
fn linked_worktree_delete_options_omit_remove_for_main_worktree() {
    let mut cleanup = linked_worktree_cleanup(&[]);
    cleanup.resolution.worktree.is_main = true;
    let options = linked_worktree_delete_options(&cleanup);
    assert_eq!(options.len(), 2);
    assert!(options.iter().all(|(_, action)| !matches!(action, SyncBranchDeleteAction::RemoveWorktree { .. })));
}
```

- [x] **Step 2: Run unit tests and verify the option builder is missing**

Run: `cargo nextest run commands::sync::tests::linked_worktree_delete_options`

Expected: FAIL because `linked_worktree_delete_options` does not exist.

- [x] **Step 3: Implement the action menu**

Import `dialoguer::Select`, implement the option builder, and use it only for interactive cleanup candidates owned by another worktree:

```rust
fn linked_worktree_delete_options(
    cleanup: &BlockingWorktreeCleanup,
) -> Vec<(String, SyncBranchDeleteAction)> {
    let mut options = vec![(
        match &cleanup.resolution.switch_target {
            BranchDeleteSwitchTarget::Branch(target) => format!("Keep worktree, switch it to '{target}', and delete branch"),
            BranchDeleteSwitchTarget::Detach => "Keep worktree, detach HEAD, and delete branch".to_string(),
        },
        SyncBranchDeleteAction::PreserveWorktree,
    )];
    if cleanup.can_remove_during_sync() {
        options.push(("Remove worktree and delete branch".to_string(), SyncBranchDeleteAction::RemoveWorktree { force: false }));
    } else if cleanup.can_force_remove_dirty_worktree_during_sync() {
        options.push(("Force-remove dirty worktree and delete branch".to_string(), SyncBranchDeleteAction::RemoveWorktree { force: true }));
    }
    options.push(("Skip".to_string(), SyncBranchDeleteAction::Skip));
    options
}

fn choose_linked_worktree_delete_action(
    branch: &str,
    cleanup: &BlockingWorktreeCleanup,
) -> Result<SyncBranchDeleteAction> {
    let options = linked_worktree_delete_options(cleanup);
    let labels = options.iter().map(|(label, _)| label.as_str()).collect::<Vec<_>>();
    let selected = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Branch '{branch}' is checked out in worktree '{}'. What should stax do?", cleanup.resolution.worktree.name))
        .items(&labels)
        .default(0)
        .interact()?;
    Ok(options[selected].1)
}
```

Use `Skip` to record `not confirmed` without adding the candidate to the confirmed deletion set. Preserve ordinary yes/no confirmation for cleanup candidates without another worktree and for the current-branch checkout flow.

- [x] **Step 4: Add end-to-end explicit removal and dirty-preservation tests**

Use `crate::common::run_stax_in_script` to run interactive sync in a pseudo-terminal. Select the second option with one down-arrow plus Enter and assert the worktree is removed and branch deleted. Add a second `--force` fixture with tracked, untracked, and ignored changes in the linked worktree; make trunk switching conflict so the fallback detaches, then assert every file retains its exact contents. Add an upstream-gone fixture with PR metadata and a linked worktree to prove that path chooses the same preserve action. For the failure path, put the affected worktree into an unresolved merge before forced sync and assert a failed switch/detach retains the worktree, local/remote branch, and `refs/branch-metadata/<branch>`.

```rust
let output = crate::common::run_stax_in_script(
    &repo.path(),
    &["sync"],
    "wait_for_tui_text \"What should stax do?\"; printf '\\033[B\\n'",
);
assert!(output.status.success(), "{}", TestRepo::stderr(&output));
assert!(!worktree.exists());
assert!(!repo.list_branches().contains(&branch));
```

- [x] **Step 5: Run the sync preservation and menu test set**

Run: `cargo nextest run integration_tests::test_sync_force_preserves_worktree_for_merged_branch integration_tests::test_sync_interactive_removes_linked_worktree integration_tests::test_sync_force_preserves_dirty_linked_worktree integration_tests::test_sync_force_preserves_upstream_gone_linked_worktree integration_tests::test_sync_preservation_failure_keeps_all_refs commands::sync::tests::linked_worktree_delete_options`

Expected: all new tests PASS, including explicit remove and dirty fallback-to-detach behavior.

- [x] **Step 6: Amend the stax branch commit**

Run: `stax modify -a -m "feat: preserve worktrees during sync cleanup"`

Expected: the current branch commit contains both forced and interactive behavior.

---

### Task 3: Document behavior and complete repository verification

**Files:**
- Modify: `docs/workflows/multi-worktree.md`
- Modify: `skills.md`
- Modify if formatting requires it: files changed in Tasks 1-2

**Interfaces:**
- Consumes: final command semantics from Tasks 1-2.
- Produces: user and agent documentation matching the shipped behavior.

- [x] **Step 1: Update multi-worktree documentation**

Replace the cleanup bullet in `docs/workflows/multi-worktree.md` and add a short cleanup section:

```markdown
- `st sync` cleanup preserves a linked worktree that owns a merged or upstream-gone branch by switching it to trunk or detaching `HEAD` before deleting the branch.

## Sync cleanup

When a cleanup candidate is checked out in another worktree, interactive sync offers to keep the worktree (the default), remove it, or skip the branch. Keeping it preserves ignored, untracked, and modified files. `st sync --force` also keeps the worktree; `--force` skips prompts but never authorizes worktree removal.
```

- [x] **Step 2: Update agent-facing command guidance**

Change the `skills.md` force description and cleanup note to state:

```text
stax sync --force                  # Force sync without prompts; preserve linked worktrees during cleanup
# sync cleanup switches/detaches linked worktrees before deleting merged/gone branches; interactive removal remains explicit.
```

- [x] **Step 3: Run fast formatting and targeted verification**

Run: `make lint-fast`

Expected: formatting and library/binary Clippy checks exit 0.

Run: `cargo nextest run integration_tests::test_sync_ commands::sync::tests`

Expected: sync integration and unit tests exit 0.

- [x] **Step 4: Run required final lint**

Run: `make lint`

Expected: all-target/all-feature lint exits 0.

- [x] **Step 5: Start Docker and run the required full suite**

Run: `docker info >/dev/null && make test`

Expected: Docker is available and the complete test suite exits 0. If Docker is unavailable, launch Docker Desktop with `open -a Docker`, wait for `docker info` to succeed, and rerun `make test`; do not fall back to the native full suite.

- [x] **Step 6: Amend the final stax branch commit and inspect the stack**

Run: `stax modify -a -m "feat: preserve worktrees during sync cleanup"`

Run: `stax ls`

Expected: `cesar/preserve-sync-worktrees` is a clean branch stacked directly on `main`.
