# Stax GUI Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete native GUI parity for structural stack operations, safe local undo/redo, search, pane persistence, native menus, and the main TUI keyboard vocabulary.

**Architecture:** Add one typed `stax::application` operation at a time and migrate the existing CLI/TUI adapter before exposing it through GPUI. Structural mutations remain explicit-path, transaction-backed, side-effect-aware operations; GPUI owns only selection, overlay, search, pane, focus, and menu presentation state.

**Tech Stack:** Rust 1.96, git2, existing stax transactions/receipts, Ratatui/crossterm adapters, GPUI 0.2.2, serde/serde_json, cargo-nextest.

## Global Constraints

- Work on `cesar/gpui-gui-phase-3`, stacked directly on `cesar/gpui-gui-phase-2`.
- Keep `stax` as the default workspace member; GPUI must not enter normal CLI builds.
- Never execute `st`, parse terminal output, or change process-global current directory from application or GUI code.
- Only one mutating operation may run per common repository at a time.
- Every non-trivial code change requires happy-path, bad-path, and edge-case coverage.
- Full-suite verification must use `make test`, never a full native `cargo test` run.
- User-visible changes must update `README.md`, relevant `docs/` pages, and `skills.md`.
- Release packaging, icon assets, signing, notarization, and public distribution remain Phase 4.

---

## File Map

### Shared application layer

- Create `src/application/rename.rs` — explicit-path local branch rename and transaction receipt.
- Create `src/application/delete.rs` — explicit-path local branch deletion and transaction receipt.
- Create `src/application/move_subtree.rs` — subtree reparent validation, rebase, progress, and recovery.
- Create `src/application/reorder.rs` — stale-safe linear reorder preview validation and execution.
- Create `src/application/history.rs` — transaction-backed undo/redo requests without terminal output.
- Modify `src/application/operation.rs` — requests, stages, outcomes, error details, and tests.
- Modify `src/application/repository.rs` — dispatch and common mutation/worktree helpers.
- Modify `src/application/mod.rs` — module declarations and public exports.
- Modify `src/ops/receipt.rs` — structural operation kinds and absent-ref before/after facts.
- Modify `src/ops/tx.rs` — optional after-state recording and post-operation checkout facts.

### Existing adapters

- Modify `src/commands/branch/rename.rs` — retain prompting/remote/edit concerns; delegate local literal rename.
- Modify `src/commands/branch/delete.rs` — retain picker/confirmation; delegate deletion.
- Modify `src/commands/upstack/onto.rs` — retain picker/output; delegate selected subtree move.
- Modify `src/commands/reorder.rs` — retain interactive order picker; delegate apply.
- Modify `src/commands/undo.rs` and `src/commands/redo.rs` — retain CLI output/flags; delegate receipt application.
- Modify `src/tui/app.rs` and `src/tui/mod.rs` — typed pending requests for all migrated dashboard actions.

### Native GUI

- Modify `crates/stax-gui/src/state.rs` — structural action availability, search state, pane state, and receipt-driven undo/redo.
- Modify `crates/stax-gui/src/preferences.rs` — private atomic per-repository workspace preferences.
- Modify `crates/stax-gui/src/views/app.rs` — GPUI actions, dispatch, focus, overlays, and key bindings.
- Modify `crates/stax-gui/src/views/operation_overlay.rs` — rename/delete/move/reorder/history overlays.
- Modify `crates/stax-gui/src/views/workspace.rs` — conditional panes, toolbar/menu-facing controls, and focus traversal.
- Modify `crates/stax-gui/src/views/stack_pane.rs` — filtered rows and search field.
- Modify `crates/stax-gui/src/views/inspector_pane.rs` — contextual structural/history controls.
- Modify `crates/stax-gui/src/views/text_input.rs` — reusable search input editing without workspace-shortcut leakage.
- Modify `crates/stax-gui/src/lib.rs` — native menu registration.

### Tests and documentation

- Modify `tests/application_operation_tests.rs` and `tests/tui_commands_tests.rs`.
- Modify `crates/stax-gui/src/views/operation_tests.rs` and `crates/stax-gui/src/views/tests.rs`.
- Modify `README.md`, `docs/interface/gui.md`, `docs/commands/core.md`, `docs/commands/reference.md`, and `skills.md`.

---

### Task 1: Extend transaction receipts for structural ref changes

**Files:**
- Modify: `src/ops/receipt.rs`
- Modify: `src/ops/tx.rs`
- Modify: `src/application/operation.rs`
- Test: `src/ops/receipt.rs`
- Test: `src/ops/tx.rs`

**Interfaces:**
- Produces: receipts that represent ref creation, deletion, and rename in both before and after directions.
- Consumes: existing `OpReceipt`, `Transaction`, `LocalRefEntry`, and `TransactionSummary` contracts.

- [ ] **Step 1: Write failing receipt transition tests**

Add exact coverage for absent before/after refs and checkout restoration:

```rust
fn successful_receipt(kind: OpKind) -> OpReceipt {
    let mut receipt = OpReceipt::new(
        "op-1".into(),
        kind,
        "/tmp/repo".into(),
        "main".into(),
        "feature".into(),
    );
    receipt.mark_success();
    receipt
}

#[test]
fn deleted_ref_is_redoable_with_an_explicit_absent_after_state() {
    let mut receipt = successful_receipt(OpKind::Delete);
    receipt.add_local_ref("feature", Some("1111111111111111111111111111111111111111"));
    receipt.update_local_ref_after_optional("feature", None);
    assert!(receipt.can_undo());
    assert!(receipt.can_redo());
}

#[test]
fn renamed_head_records_distinct_before_and_after_checkout_names() {
    let mut receipt = successful_receipt(OpKind::Rename);
    receipt.head_branch_before = "old".into();
    receipt.head_branch_after = Some("new".into());
    assert_eq!(receipt.undo_head_branch(), "old");
    assert_eq!(receipt.redo_head_branch(), "new");
}
```

- [ ] **Step 2: Run the focused test and confirm it fails to compile**

Run: `cargo nextest run --lib ops::receipt::tests::deleted_ref_is_redoable ops::receipt::tests::renamed_head_records`

Expected: compilation fails because optional after-state and after-head facts do not exist.

- [ ] **Step 3: Add backward-compatible receipt facts**

Add `Rename`, `Delete`, and `MoveSubtree` to `OpKind`. Add these
serde-defaulted fields and accessors:

```rust
// LocalRefEntry
#[serde(default)]
pub after_recorded: bool,

// OpReceipt
#[serde(default)]
pub head_branch_after: Option<String>,

pub fn update_local_ref_after_optional(&mut self, branch: &str, oid: Option<&str>);
pub fn undo_head_branch(&self) -> &str;
pub fn redo_head_branch(&self) -> &str;
```

`update_local_ref_after_optional` sets `after_recorded = true`; old receipts
with a populated `oid_after` remain compatible. `can_redo` must return true for
a successful entry whose before/after states differ even when the explicitly
recorded after state is absent. Extend `Transaction` with
`record_optional_after(repo, branch)` and `set_head_branch_after(name)`; keep
the existing non-optional helper as a compatibility wrapper. Add structural
`OpKind::display_name` values and prove old JSON without `head_branch_after`
or `after_recorded` still deserializes. Add `can_redo` to
`TransactionSummary` and map it from `OpReceipt::can_redo()`.

- [ ] **Step 4: Run contract tests**

Run: `cargo nextest run --lib ops::receipt::tests:: ops::tx::tests:: application::operation::tests::transaction_summary_`

Expected: PASS.

- [ ] **Step 5: Commit the contract**

```bash
git add src/ops/receipt.rs src/ops/tx.rs src/application/operation.rs
git commit -m "feat(ops): record structural ref transitions"
```

---

### Task 2: Extract local rename and migrate CLI/TUI callers

**Files:**
- Create: `src/application/rename.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/commands/branch/rename.rs`
- Modify: `src/tui/mod.rs`
- Test: `tests/application_operation_tests.rs`
- Test: `tests/tui_commands_tests.rs`

**Interfaces:**
- Produces: `RepositorySession::rename_branch(&self, branch, new_name, reporter) -> OperationResult`.
- Consumes: `format_branch_name(..., BranchNameContext::literal())`, `MutationTargets`, `Transaction`, `OpKind`, and `OperationRequest::RenameBranch`.

- [ ] **Step 1: Write failing rename tests**

Cover success, child metadata updates, trunk rejection, non-current rejection,
invalid/colliding names, and rollback-sensitive receipt facts:

```rust
#[test]
fn rename_updates_ref_metadata_children_and_returns_undoable_receipt() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let branches = repo.create_stack(&["parent", "child"]);
    repo.run_stax(&["checkout", &branches[0]]).assert_success();

    let receipt = RepositorySession::open(repo.path()).unwrap()
        .rename_branch(&branches[0], "renamed", &mut NoopOperationReporter)
        .unwrap();

    assert_eq!(repo.current_branch(), "renamed");
    assert!(repo.get_children("renamed").contains(&branches[1]));
    assert!(matches!(receipt.outcome, OperationOutcome::BranchRenamed { .. }));
    assert!(receipt.transaction.as_ref().is_some_and(|tx| tx.can_undo));
}
```

- [ ] **Step 2: Run rename tests and confirm failure**

Run: `cargo nextest run application_operation_tests::rename_`

Expected: FAIL because `RepositorySession::rename_branch` is missing.

- [ ] **Step 3: Implement explicit-path local rename**

Implement the public framed method and an unframed dispatcher method following
`src/application/checkout.rs`. Add `OperationRequest::RenameBranch`,
`OperationStage::RenamingBranch`, and `OperationOutcome::BranchRenamed` in the
same change so dispatch remains exhaustive and compilable. Validate before
starting a transaction, plan the old/new refs plus old/new/child metadata refs,
snapshot, rename through `GitRepo`/git in
the session workdir, update metadata, record after-state, finish, and return:

```rust
OperationReceipt {
    request: request.clone(),
    summary: format!("Renamed {branch} to {new_name}"),
    affected_branches,
    outcome: OperationOutcome::BranchRenamed {
        old_name: branch.to_string(),
        new_name: new_name.to_string(),
    },
    transaction: Some(TransactionSummary::from(&receipt)),
    warnings: formatted.warnings,
    side_effects: OperationSideEffects::RepositoryChanged,
}
```

Map pre-mutation failures to `None` side effects. If a post-ref metadata write
fails, finish the transaction as failed and return a retained receipt with
`RepositoryChanged`.

- [ ] **Step 4: Migrate CLI and TUI**

Keep CLI prompts, `--push`, and `--edit` in `commands/branch/rename.rs`, but
delegate the local rename before optional remote/edit steps. Replace the TUI
`LegacyCommands(["rename", "--literal", name])` path with:

```rust
queue_operation(app, OperationRequest::RenameBranch {
    branch: app.current_branch.clone(),
    new_name: input.clone(),
});
```

- [ ] **Step 5: Verify rename parity**

Run:

```bash
cargo nextest run application_operation_tests::rename_
cargo nextest run tui_commands_tests::test_tui_rename_branch_with_literal
cargo nextest run tui::tests::migrated_tui_actions_never_use_legacy_commands
```

Expected: PASS.

- [ ] **Step 6: Commit rename extraction**

```bash
git add src/application/rename.rs src/application/mod.rs src/application/repository.rs src/commands/branch/rename.rs src/tui/mod.rs tests/application_operation_tests.rs tests/tui_commands_tests.rs
git commit -m "feat(application): share branch rename operation"
```

---

### Task 3: Extract deletion with descendant-aware confirmation facts

**Files:**
- Create: `src/application/delete.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/commands/branch/delete.rs`
- Modify: `src/tui/mod.rs`
- Test: `tests/application_operation_tests.rs`
- Test: `tests/tui_commands_tests.rs`

**Interfaces:**
- Produces: `RepositorySession::delete_branch(&self, branch, force, reporter) -> OperationResult`.
- Consumes: `OperationRequest::DeleteBranch`, `BranchMetadata`, `Transaction`, and `MutationTargets`.

- [ ] **Step 1: Write failing deletion tests**

Add success/undo, trunk, current branch, missing branch, non-force unmerged
branch, and descendant-preservation tests. The descendant test must assert the
child is not deleted or silently reparented.

- [ ] **Step 2: Run deletion tests and confirm failure**

Run: `cargo nextest run application_operation_tests::delete_`

Expected: FAIL because the session method is missing.

- [ ] **Step 3: Implement transactional deletion**

Add `OperationRequest::DeleteBranch`, `OperationStage::DeletingBranch`, and
`OperationOutcome::BranchDeleted` with its dispatcher arm. Validate the target,
collect descendants for `affected_branches`, begin an
`OpKind::Delete` transaction, plan the target ref and metadata, snapshot,
delete the local ref with the supplied force flag, delete only the target's
metadata, record after-state, and return `BranchDeleted`. Retain descendants in
the receipt so presentation layers can warn before confirmation.

- [ ] **Step 4: Migrate CLI and TUI**

Keep picker and confirmation in the CLI adapter. Replace the TUI legacy command
with:

```rust
queue_operation(app, OperationRequest::DeleteBranch {
    branch: branch.clone(),
    force: true,
});
```

- [ ] **Step 5: Verify deletion parity and edge cases**

Run:

```bash
cargo nextest run application_operation_tests::delete_
cargo nextest run tui_commands_tests::test_tui_delete_branch_force
cargo nextest run tui_commands_tests::test_tui_delete_branch_via_dashboard
```

Expected: PASS.

- [ ] **Step 6: Commit deletion extraction**

```bash
git add src/application/delete.rs src/application/mod.rs src/application/repository.rs src/commands/branch/delete.rs src/tui/mod.rs tests/application_operation_tests.rs tests/tui_commands_tests.rs
git commit -m "feat(application): share branch deletion operation"
```

---

### Task 4: Extract subtree move with checkout preservation

**Files:**
- Create: `src/application/move_subtree.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/commands/upstack/onto.rs`
- Modify: `src/tui/app.rs`
- Modify: `src/tui/mod.rs`
- Test: `tests/application_operation_tests.rs`
- Test: `tests/upstack_onto_tests.rs`

**Interfaces:**
- Produces: `RepositorySession::move_subtree(&self, source, new_parent, auto_stash, reporter) -> OperationResult`.
- Consumes: `RestackExecutionOptions`, existing restack preflight/stash helpers, `Stack::descendants`, and `OperationRequest::MoveSubtree`.

- [ ] **Step 1: Write failing move tests**

Cover successful source+descendant movement from a different checkout, original
checkout restoration, same-parent no-op, trunk/missing metadata/missing parent,
cycle rejection, dirty worktree preflight, auto-stash retry, and conflict
recovery state.

```rust
#[test]
fn move_subtree_preserves_original_checkout() {
    let repo = TestRepo::new();
    repo.set_trunk("main");
    let branches = repo.create_stack(&["a", "b"]);
    repo.run_stax(&["checkout", "main"]).assert_success();
    let receipt = RepositorySession::open(repo.path()).unwrap()
        .move_subtree(&branches[1], "main", false, &mut NoopOperationReporter)
        .unwrap();
    assert_eq!(repo.current_branch(), "main");
    assert!(matches!(receipt.outcome, OperationOutcome::SubtreeMoved { .. }));
}
```

- [ ] **Step 2: Run move tests and confirm failure**

Run: `cargo nextest run application_operation_tests::move_subtree_`

Expected: FAIL because the operation does not exist.

- [ ] **Step 3: Implement shared move execution**

Add `OperationRequest::MoveSubtree`, `OperationStage::MovingSubtree`, and
`OperationOutcome::SubtreeMoved` with its dispatcher arm. Move the
non-interactive validation and rebase logic from
`commands/upstack/onto.rs` into the new module. Acquire one mutation lease for
source, descendants, new parent, and current checkout. Report one
`MovingSubtree` preparation event followed by bottom-to-top `Restacking`
events. Persist new parent metadata only after the source rebase succeeds.
Finish one transaction containing every moved branch.

- [ ] **Step 4: Migrate CLI and TUI adapters**

The CLI retains `FuzzySelect` and colored output, then passes the chosen target
to the application method. The TUI move picker queues `MoveSubtree` directly;
remove the `checkout <source>` plus `upstack onto <target>` legacy sequence.

- [ ] **Step 5: Verify move behavior**

Run:

```bash
cargo nextest run application_operation_tests::move_subtree_
cargo nextest run upstack_onto_tests::
cargo nextest run tui::tests::migrated_tui_actions_never_use_legacy_commands
```

Expected: PASS.

- [ ] **Step 6: Commit move extraction**

```bash
git add src/application/move_subtree.rs src/application/mod.rs src/application/repository.rs src/commands/upstack/onto.rs src/tui/app.rs src/tui/mod.rs tests/application_operation_tests.rs tests/upstack_onto_tests.rs
git commit -m "feat(application): share subtree move operation"
```

---

### Task 5: Extract stale-safe linear reorder

**Files:**
- Create: `src/application/reorder.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/commands/reorder.rs`
- Modify: `src/tui/app.rs`
- Modify: `src/tui/mod.rs`
- Test: `tests/application_operation_tests.rs`
- Test: `tests/reorder_tests.rs`

**Interfaces:**
- Produces: `RepositorySession::reorder_stack(&self, original_order, proposed_order, auto_stash, reporter) -> OperationResult`.
- Consumes: immutable order vectors, `Stack`, rebase preflight, `Transaction`, and `OperationRequest::ReorderStack`.

- [ ] **Step 1: Write failing reorder tests**

Cover two/three-branch success, unchanged order, empty/one-branch order,
duplicate/missing/trunk entries, fork rejection, cycle prevention, stale
original order, dirty worktrees, conflict, checkout restoration, and undoable
receipt facts.

- [ ] **Step 2: Run reorder tests and confirm failure**

Run: `cargo nextest run application_operation_tests::reorder_ reorder_tests::`

Expected: FAIL because the shared method is missing.

- [ ] **Step 3: Implement validation and apply**

Add `OperationRequest::ReorderStack`, `OperationStage::ReorderingStack`, and
`OperationOutcome::StackReordered` with its dispatcher arm. Reload the current
stack, derive its one linear non-trunk chain, and compare it
exactly with `original_order` before mutation. Reject forks and stale previews
with `PreconditionFailed` and `None` side effects. Compute changed parent pairs,
run one preflight, then transactionally update metadata and rebase changed
branches bottom-to-top. Return both original and applied order.

- [ ] **Step 4: Migrate CLI/TUI order pickers**

Keep terminal and TUI preview state, but delete their direct metadata/rebase
application code. Both adapters submit the same request:

```rust
OperationRequest::ReorderStack {
    original_order,
    proposed_order,
    auto_stash: false,
}
```

- [ ] **Step 5: Verify reorder behavior**

Run:

```bash
cargo nextest run application_operation_tests::reorder_
cargo nextest run reorder_tests::
cargo nextest run tui::tests::migrated_tui_actions_never_use_legacy_commands
```

Expected: PASS.

- [ ] **Step 6: Commit reorder extraction**

```bash
git add src/application/reorder.rs src/application/mod.rs src/application/repository.rs src/commands/reorder.rs src/tui/app.rs src/tui/mod.rs tests/application_operation_tests.rs tests/reorder_tests.rs
git commit -m "feat(application): share stack reorder operation"
```

---

### Task 6: Extract safe local undo and redo

**Files:**
- Create: `src/application/history.rs`
- Modify: `src/application/mod.rs`
- Modify: `src/application/repository.rs`
- Modify: `src/commands/undo.rs`
- Modify: `src/commands/redo.rs`
- Test: `tests/application_operation_tests.rs`
- Test: `tests/integration_tests.rs`
- Test: `tests/additional_coverage_tests.rs`
- Test: `tests/edge_cases_tests.rs`

**Interfaces:**
- Produces: `RepositorySession::undo_transaction(operation_id, update_remote, reporter)` and `redo_transaction(...)`.
- Consumes: canonical receipt lookup/application helpers extracted from the current command modules.

- [ ] **Step 1: Write failing history tests**

Create a rename/delete/reorder receipt, undo it locally, redo it locally, and
assert exact refs and transaction outcomes. Add missing receipt, dirty tree,
failed receipt, non-undoable receipt, and `update_remote: false` tests.

- [ ] **Step 2: Run history tests and confirm failure**

Run: `cargo nextest run application_operation_tests::undo_ application_operation_tests::redo_`

Expected: FAIL because the session methods are missing.

- [ ] **Step 3: Move receipt application into the application module**

Add `OperationRequest::UndoTransaction`/`RedoTransaction`, matching stages,
`TransactionUndone`/`TransactionRedone` outcomes, and dispatcher arms. Extract
terminal-free lookup, validation, local ref restoration, metadata restoration,
remote restoration, and status persistence. Undo must delete refs whose
`existed_before` is false; redo must delete refs whose explicit after-state is
absent. Checkout `undo_head_branch()` after undo and `redo_head_branch()` after
redo. Report
`UndoingTransaction`/`RedoingTransaction` progress per ref. Return typed
outcomes and conservative side effects. Never update remotes when
`update_remote` is false.

- [ ] **Step 4: Keep CLI presentation as adapters**

CLI modules parse optional operation ids and flags, ask prompts where required,
call the session method, and render the typed receipt. Remove duplicated ref
application logic.

- [ ] **Step 5: Verify history behavior**

Run:

```bash
cargo nextest run application_operation_tests::undo_ application_operation_tests::redo_
cargo nextest run integration_tests::test_undo integration_tests::test_redo additional_coverage_tests::test_undo_no_operations additional_coverage_tests::test_redo_no_operations edge_cases_tests::test_undo_yes_flag_no_prompt edge_cases_tests::test_redo_yes_flag_no_prompt
```

Expected: PASS.

- [ ] **Step 6: Commit history extraction**

```bash
git add src/application/history.rs src/application/mod.rs src/application/repository.rs src/commands/undo.rs src/commands/redo.rs tests/application_operation_tests.rs tests/integration_tests.rs tests/additional_coverage_tests.rs tests/edge_cases_tests.rs
git commit -m "feat(application): share undo and redo operations"
```

---

### Task 7: Extend GUI state and overlay models

**Files:**
- Modify: `crates/stax-gui/src/state.rs`
- Modify: `crates/stax-gui/src/views/operation_overlay.rs`
- Test: `crates/stax-gui/src/views/operation_tests.rs`

**Interfaces:**
- Produces: structural fields in `InteractionState`; overlay variants for rename, delete, move, reorder, stash retry, undo, and redo.
- Consumes: `WorkspaceState` snapshot/selection/receipt and the typed requests produced by Tasks 2–6.

- [ ] **Step 1: Write failing availability tests**

Assert rename only for current non-trunk; delete only for selected non-current
non-trunk; move for tracked non-trunk with candidates; reorder only for a
linear stack of at least two branches; local undo only when `can_undo` and
`!changed_remote_refs`; and all structural actions disabled during mutation.

- [ ] **Step 2: Run GUI state tests and confirm failure**

Run: `cargo nextest run -p stax-gui state::tests::interaction_ views::operation_tests::structural_`

Expected: FAIL because the fields/overlays are missing.

- [ ] **Step 3: Add state authority and immutable previews**

Extend `InteractionState` with `rename`, `delete`, `move_subtree`, `reorder`,
`undo`, and `redo`. Add state helpers that derive descendants, eligible parent
candidates, and one linear order from the current snapshot. Add overlays:

```rust
RenameBranch { branch: String, validation_error: Option<String> },
ConfirmDelete { branch: String, descendants: Vec<String> },
PickMoveParent { source: String, candidates: Vec<String>, query: String, selected: usize },
ConfirmMove { source: String, new_parent: String, branches: Vec<String>, auto_stash: bool },
ReorderStack { original: Vec<String>, proposed: Vec<String>, moving: usize },
ConfirmReorder { original: Vec<String>, proposed: Vec<String>, auto_stash: bool },
ConfirmUndo { operation_id: String, branches: Vec<String> },
ConfirmRedo { operation_id: String, branches: Vec<String> },
```

- [ ] **Step 4: Verify state transitions and cancellation**

Run: `cargo nextest run -p stax-gui state::tests:: views::operation_tests::structural_`

Expected: PASS.

- [ ] **Step 5: Commit GUI state models**

```bash
git add crates/stax-gui/src/state.rs crates/stax-gui/src/views/operation_overlay.rs crates/stax-gui/src/views/operation_tests.rs
git commit -m "feat(gui): model structural operation flows"
```

---

### Task 8: Wire structural GUI controls, shortcuts, and receipts

**Files:**
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/views/inspector_pane.rs`
- Modify: `crates/stax-gui/src/views/operation_overlay.rs`
- Modify: `crates/stax-gui/src/operation.rs`
- Test: `crates/stax-gui/src/views/operation_tests.rs`

**Interfaces:**
- Produces: GPUI actions `RenameSelected`, `DeleteSelected`, `MoveSelected`, `ReorderSelectedStack`, `UndoLatest`, and `RedoLatest`.
- Consumes: Task 7 action availability/overlays and `OperationService`.

- [ ] **Step 1: Write failing end-to-end GPUI action tests**

For each button and shortcut, open the required overlay, assert cancellation
sends no request, confirm, and assert the fake service receives exactly the
typed request. Cover `e`, `d`, `m`, `o`, `cmd-z`, and `cmd-shift-z`, plus text
input/picker shortcut suppression.

- [ ] **Step 2: Run action tests and confirm failure**

Run: `cargo nextest run -p stax-gui views::operation_tests::rename views::operation_tests::delete views::operation_tests::move views::operation_tests::reorder views::operation_tests::undo views::operation_tests::redo`

Expected: FAIL because actions are not registered.

- [ ] **Step 3: Register actions and dispatch exact requests**

Add action handlers and bindings. Extend `confirm_overlay` so every confirmed
overlay produces one request and clears/restores focus exactly once. Reuse the
existing operation event loop; extend fake receipt helpers for new outcomes.
Extend `finish_operation_from_retained_result` so a `DirtyWorktree` failure for
`MoveSubtree { auto_stash: false }` or `ReorderStack { auto_stash: false }`
opens the equivalent confirmation with `auto_stash: true` and the structured
`OperationErrorDetails::Rebase` worktree path, matching the existing restack
retry flow.

- [ ] **Step 4: Render contextual controls and receipt actions**

Add inspector buttons with disabled reasons. Render affected branches and
warnings in confirmations. Add Undo/Redo controls to the terminal operation
banner only for safe local transaction facts. Remote-changing receipts display
the operation id and CLI guidance instead.

- [ ] **Step 5: Verify GPUI operations**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::
cargo nextest run -p stax-gui views::tests::
```

Expected: PASS.

- [ ] **Step 6: Commit structural GUI actions**

```bash
git add crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/workspace.rs crates/stax-gui/src/views/inspector_pane.rs crates/stax-gui/src/views/operation_overlay.rs crates/stax-gui/src/operation.rs crates/stax-gui/src/views/operation_tests.rs
git commit -m "feat(gui): add structural stack actions"
```

---

### Task 9: Add search and per-repository pane persistence

**Files:**
- Modify: `crates/stax-gui/src/preferences.rs`
- Modify: `crates/stax-gui/src/state.rs`
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/views/stack_pane.rs`
- Modify: `crates/stax-gui/src/views/text_input.rs`
- Test: `crates/stax-gui/src/views/tests.rs`

**Interfaces:**
- Produces: `WorkspacePreferenceStore`, `PaneVisibility`, `PaneWidths`, and stack-search state.
- Consumes: canonical repository root and existing private atomic preference-writing pattern.

- [ ] **Step 1: Write failing preference and search tests**

Cover private atomic round-trip, separate values for two repositories, corrupt
JSON fallback, invalid widths fallback, refusal to hide the last pane, divider
drag clamping/persistence, filtered selection navigation, no-match state,
Escape reset, text-edit priority, and hidden-pane focus skip.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo nextest run -p stax-gui preferences::tests::workspace_ state::tests::search_ views::tests::pane_`

Expected: FAIL because workspace preferences/search are missing.

- [ ] **Step 3: Implement per-repository workspace preferences**

Store one JSON document under `dirs::data_dir()/stax/gui/workspaces.json`, keyed
by canonical repository path. Use the same lock, `0600` file, `0700`
directory, `NamedTempFile`, fsync, and atomic persist pattern as recent
repositories. Clamp widths to `0.15..=0.70`, normalize their sum, and fall back
to `0.29/0.43/0.28`. Add a `WorkspacePreferenceStore` trait to `AppServices`,
inject a real store in `AppServices::native`, and inject an in-memory fake in
GPUI tests so opening one repository loads only that repository's workspace
settings.

- [ ] **Step 4: Implement search and conditional pane rendering**

Generalize the existing text input editing core so a dedicated search entity
can share insertion, deletion, cursor movement, and focus behavior without
branch-name validation. Add `/` focus, case-insensitive substring filtering,
filtered Up/Down/Enter, Escape clear, and no-match presentation. Render only
visible panes using saved widths. Add two pointer-draggable dividers that
update adjacent widths, clamp each visible pane to `0.15..=0.70`, persist on
drag end, and expose keyboard-neutral debug selectors for GPUI tests. Add `1`,
`2`, `3`, and Tab behavior with at-least-one-pane and focus-skip rules.

- [ ] **Step 5: Verify persistence and navigation**

Run:

```bash
cargo nextest run -p stax-gui preferences::tests::
cargo nextest run -p stax-gui state::tests::search_ views::tests::pane_
```

Expected: PASS.

- [ ] **Step 6: Commit search and panes**

```bash
git add crates/stax-gui/src/preferences.rs crates/stax-gui/src/state.rs crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/workspace.rs crates/stax-gui/src/views/stack_pane.rs crates/stax-gui/src/views/text_input.rs crates/stax-gui/src/views/tests.rs
git commit -m "feat(gui): persist panes and add stack search"
```

---

### Task 10: Add native menus and complete keyboard parity

**Files:**
- Modify: `crates/stax-gui/src/lib.rs`
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/tests.rs`
- Modify: `crates/stax-gui/src/views/operation_tests.rs`

**Interfaces:**
- Produces: application, File, Edit, View, Branch, and Stack menu models.
- Consumes: the same GPUI actions and `InteractionState` used by visible controls.

- [ ] **Step 1: Write failing menu-dispatch tests**

Assert menu actions share action types with buttons/key bindings; commands are
disabled during mutation; text input consumes workspace shortcut letters; and
all main TUI shortcuts have one documented binding.

- [ ] **Step 2: Run menu tests and confirm failure**

Run: `cargo nextest run -p stax-gui views::tests::menu_ views::operation_tests::shortcut_`

Expected: FAIL because menus are missing/incomplete.

- [ ] **Step 3: Register native menus**

Build menus in `crates/stax-gui/src/lib.rs` after action registration. Include
Open/Refresh, Undo/Redo, Search, pane toggles, Checkout/Create/Rename/Delete/
Move, Reorder/Restack/Submit/Open PR, and standard Quit/About roles. Dispatch
existing actions only; do not add menu-only business logic.

- [ ] **Step 4: Complete binding and focus tests**

Verify Up/Down, Enter, `n`, `e`, `d`, `m`, `o`, `r`, Shift-R, `s`, `p`, `/`,
`1`/`2`/`3`, Tab, Cmd-O, Cmd-R, Cmd-Z, Cmd-Shift-Z, Enter confirm, and Escape
dismiss in their correct contexts.

- [ ] **Step 5: Commit menus and parity**

```bash
git add crates/stax-gui/src/lib.rs crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/tests.rs crates/stax-gui/src/views/operation_tests.rs
git commit -m "feat(gui): add native menus and keyboard parity"
```

---

### Task 11: Update documentation and run final verification

**Files:**
- Modify: `README.md`
- Modify: `docs/interface/gui.md`
- Modify: `docs/commands/core.md`
- Modify: `docs/commands/reference.md`
- Modify: `skills.md`
- Modify: any source/test file needed for verification-only fixes

**Interfaces:**
- Consumes: all Phase 3 user-visible behavior.
- Produces: accurate contributor/user/agent documentation and verified branch state.

- [ ] **Step 1: Update all user-visible documentation**

Document rename/delete/move/reorder, exact confirmations, local-only safe
undo/redo, search, pane persistence/toggles, menus, shortcuts, conflict
recovery, and the fact that packaged distribution remains Phase 4. Remove
Phase 2 statements that incorrectly list these capabilities as unavailable.

- [ ] **Step 2: Run documentation consistency checks**

Run:

```bash
rg -n "rename|delete|move|reorder|undo|redo|search|pane|Phase 4" README.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md skills.md
git diff --check
```

Expected: every capability appears in the relevant surface and no whitespace
errors are reported.

- [ ] **Step 3: Run focused application and GUI verification**

Run:

```bash
cargo nextest run application_operation_tests::
cargo nextest run -p stax-gui --locked
cargo check -p stax-gui --locked
cargo clippy -p stax-gui --all-targets --locked -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::clone_on_copy \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::let_and_return \
  -A clippy::manual_checked_ops \
  -A clippy::needless_borrow \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::unnecessary_sort_by \
  -A clippy::useless_format \
  -A clippy::useless_vec
make gui-app-test
make gui-app
```

Expected: all commands pass and the unsigned developer app bundle assembles.

- [ ] **Step 4: Run repository lint and full suite**

Start Docker Desktop if necessary, then run:

```bash
make lint
make test
```

Expected: PASS. If `make test` reports that Docker is unavailable, run
`open -a Docker`, wait for Docker Desktop to become ready, and retry `make test`;
do not substitute a full native runner.

- [ ] **Step 5: Inspect the final branch and stack**

Run:

```bash
git status --short
git diff cesar/gpui-gui-phase-2...HEAD --stat
stax status
```

Expected: clean worktree; Phase 3 is one branch above Phase 2; changes contain
no packaging/signing assets.

- [ ] **Step 6: Commit documentation or verification fixes**

```bash
git add README.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md skills.md
git commit -m "docs(gui): document structural parity"
```

Skip this commit only if documentation was already committed with the behavior
it describes and verification produced no changes.

---

## Phase Boundary

Stop after Phase 3 verification. Create `cesar/gpui-gui-phase-4` with
`stax create gpui-gui-phase-4` only after Phase 3 is clean and reviewed. Phase 4 then
owns the Strata “S” app icon, final bundle identity, release-mode packaging,
architecture-specific GitHub artifacts, unsigned Gatekeeper documentation,
optional signing/notarization secrets, accessibility hardening, and packaged
performance/smoke tests.
