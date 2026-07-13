# Stax GUI Phase 3 Design

## Status

Approved in conversation on 2026-07-13. This document is the written design
for review before implementation planning.

## Summary

Phase 3 completes structural and destructive parity between the main TUI and
the native macOS GUI. Rename, delete, move/reparent, reorder, undo, and redo
will execute through the same typed, repository-scoped application layer used
by the CLI, TUI, and GUI. The GUI will also gain stack search, per-repository
pane persistence, native menu commands, and the remaining main-TUI keyboard
vocabulary.

This phase is implemented on `cesar/gpui-gui-phase-3`, stacked directly on
`cesar/gpui-gui-phase-2`. Release packaging, app identity, icon assets,
signing, notarization, and public distribution remain Phase 4.

## Goals

- Replace the TUI's remaining legacy command paths for rename, delete,
  move/reparent, and reorder with typed application operations.
- Expose the same operations in the GUI without invoking `st`, parsing terminal
  output, changing process-global current directory, or introducing separate
  transaction semantics.
- Preserve selection without checkout: selecting a branch does not change
  `HEAD`, and internal temporary checkouts return to the original branch.
- Show exact affected branches before destructive or history-rewriting work.
- Preserve transaction receipts and expose safe local undo and redo.
- Add stack search, pane visibility and size persistence, native menus, and
  complete main-TUI keyboard parity.
- Keep the CLI package free of GPUI and macOS-only dependencies.

## Non-goals

- Remote branch rename or deletion.
- Advanced create or submit flags that are not part of the main TUI.
- The dedicated split, hunk-split, ready, or worktree TUIs.
- Replacing stax transaction receipts or conflict recovery commands.
- Changing the behavior of existing Phase 2 checkout, create, restack, submit,
  or open-PR operations except where shared operation plumbing requires it.
- macOS release packaging or distribution.

## Architecture

### Incremental operation extraction

Each structural operation is extracted independently into `stax::application`
and migrated in this order:

1. Rename.
2. Delete.
3. Move/reparent.
4. Reorder.
5. Undo and redo.

An operation is not available in the GUI until the CLI or TUI adapter that
already owns the behavior calls the same application implementation. This
keeps each extraction reviewable and prevents the application layer from
becoming a second implementation beside the command module.

The application layer continues to accept a canonical repository path and an
`OperationReporter`. It does not depend on GPUI, Ratatui, terminal prompts,
colored output, or process-global output.

### Typed requests

`OperationRequest` gains presentation-neutral requests equivalent to:

```rust
RenameBranch {
    branch: String,
    new_name: String,
}
DeleteBranch {
    branch: String,
    force: bool,
}
MoveSubtree {
    source: String,
    new_parent: String,
    auto_stash: bool,
}
ReorderStack {
    original_order: Vec<String>,
    proposed_order: Vec<String>,
    auto_stash: bool,
}
UndoTransaction {
    operation_id: String,
    update_remote: bool,
}
RedoTransaction {
    operation_id: String,
    update_remote: bool,
}
```

These are the public request variants and fields used by every adapter. In
particular, reorder carries both the previewed and proposed order so execution
can reject a stale preview rather than applying it to a changed stack.

`OperationStage` gains distinct rename, delete, topology update, reorder,
undo, and redo stages. `OperationOutcome` returns operation-specific facts,
including the old and new branch name, deleted branch, moved subtree and new
parent, applied order, and the transaction affected by undo or redo.

## Operation Semantics

### Rename

- Rename is enabled only when the selected branch is the current branch.
- Trunk cannot be renamed.
- Empty, invalid, or colliding names fail before any ref or metadata change.
- Phase 3 renames local state only; it neither pushes the new branch nor
  deletes a remote ref.
- Branch metadata, current selection, cached per-branch GUI state, and the
  receipt all use the final normalized name.
- The ref and metadata changes are captured in one transaction so a successful
  local rename can be undone.

### Delete

- Trunk and the current branch cannot be deleted.
- The GUI always presents a destructive confirmation naming the branch and any
  tracked descendants before sending `force: true`.
- Cancellation produces no request and no transaction.
- Existing descendants are not silently deleted or reparented. Their metadata
  retains its existing parent and the confirmation warns when deletion will
  leave descendants requiring an explicit move/reparent.
- The branch ref and its metadata are captured in one transaction, allowing a
  successful local deletion to be undone when the canonical receipt permits.

### Move/reparent

- Move operates on the selected source branch and its complete descendant
  subtree; it does not require the source to be checked out.
- Trunk cannot move. The source, its descendants, and its existing parent are
  excluded from invalid parent choices as appropriate.
- The new parent must exist and must not be the source or one of its
  descendants.
- Reparent and restack are one operation. Metadata is not left pointing at the
  new parent while Git history still reflects the old parent.
- The original checked-out branch is restored after success and after failures
  where checkout restoration is safe.
- Dirty affected worktrees fail with the typed preflight used by restack. The
  GUI may retry with `auto_stash: true` only after showing the exact worktrees.
- A rebase conflict preserves the repository's in-progress state and directs
  recovery through `st continue`, `st abort`, or `st undo` when valid.

### Reorder

- Reorder applies to one linear stack containing the selected branch and
  excludes trunk from the movable list.
- Forked topology is rejected with an explanation that branches must be moved
  explicitly before a linear reorder can be previewed.
- The GUI keeps the original order immutable and edits a separate proposed
  order. No repository state changes while previewing.
- The preview lists every parent-pointer change, every branch to rebase, and
  any predictable conflict or dirty-worktree precondition.
- Execution reloads stack state and requires it to match `original_order`.
  Mismatch is a no-side-effect stale-preview error requiring refresh.
- Parent metadata updates and bottom-to-top rebases run under one reorder
  transaction. The original checkout is restored when safe.

### Undo and redo

- A receipt is the authority for whether an operation can be undone or redone.
- The GUI exposes one-click Undo or Redo only for the latest displayed
  transaction when canonical receipt facts allow it and
  `changed_remote_refs == false`.
- Transactions with remote changes show the operation identifier and direct
  users to the explicit CLI flow rather than performing an implicit remote
  mutation.
- Undo and redo require a clean worktree and explicit confirmation listing the
  refs that will change.
- Completion refreshes the snapshot and retains the new receipt; failure uses
  the same side-effect-aware refresh policy as other operations.

## GUI Experience

### Contextual actions and overlays

The inspector exposes Rename, Delete, Move, Reorder, Undo, and Redo only when
their preconditions are meaningful. Disabled controls retain a short reason.
Only one repository mutation runs at a time, and repository opening, refresh,
selection changes, and other mutations remain disabled during unsafe stages.

Rename uses the existing branch-name input component. Delete uses a
destructive confirmation. Move uses a searchable parent picker followed by a
history-rewrite confirmation. Reorder uses a dedicated preview overlay with
keyboard and pointer controls, followed by a final apply confirmation. Undo
and redo use receipt-driven confirmations.

Text input and picker contexts consume printable keys before workspace
shortcuts. Escape dismisses a non-running overlay; an active Git mutation
cannot be cancelled from the GUI.

### Search

- `/` focuses stack search.
- Search is a case-insensitive substring filter over branch names.
- Up and Down move within filtered results; Enter checks out the selected
  result; Escape clears search and restores the current-branch selection.
- Filtering never changes checkout or discards the current snapshot.
- Empty and no-match states preserve pane geometry.

### Pane persistence

Pane visibility and widths are stored per canonical repository path in a
private, atomically replaced GUI preferences file. Stack, Changes, and
Inspector can be toggled independently, but the final visible pane cannot be
hidden. Corrupt or out-of-range persisted values fall back to defaults without
preventing repository opening.

The Phase 3 keyboard vocabulary includes `1`, `2`, and `3` for pane toggles,
Tab for focus traversal, and the existing navigation and operation shortcuts.
Focus skips hidden panes.

### Native menus

The application menu exposes Open Repository, Refresh, pane visibility,
search, checkout, create, rename, delete, move, reorder, restack selected/all,
submit, open PR, undo, and redo. Menu commands dispatch the same GPUI actions
as buttons and key bindings. Their enabled state follows the same
`InteractionState` authority used by the rendered controls.

## Data Flow

1. The GUI derives action availability from the current snapshot, selection,
   active operation, and latest receipt.
2. A picker or preview captures immutable source facts.
3. Confirmation converts those facts into an `OperationRequest`.
4. `OperationService` executes against the canonical repository path and
   streams typed events.
5. Progress updates the existing operation presentation without interpreting
   terminal text.
6. Completion or a side-effecting failure invalidates detail generations and
   refreshes the snapshot.
7. Preferred selection comes from the typed outcome: the new name after
   rename, the original checkout after delete/move/reorder, and the affected
   branch after undo/redo.

## Error Handling

Phase 3 uses the existing typed categories and adds precise context for:

- Invalid or colliding branch names.
- Protected trunk or current-branch operations.
- Missing or stale branch metadata.
- Invalid parent and topology cycles.
- Forked stacks unsupported by linear reorder.
- Stale reorder previews.
- Dirty affected worktrees.
- Rebase conflicts.
- Missing, ineligible, or stale transaction receipts.
- Undo or redo blocked by remote effects.

Validation failures report `OperationSideEffects::None`. Metadata/ref changes,
temporary checkouts, rebases, and receipt application report repository side
effects conservatively so clients refresh after partial failure. Diagnostics
retain the underlying error chain and remain copyable.

## Testing Strategy

Every new application operation receives:

- A happy-path unit or temp-repository test.
- Invalid-input and precondition tests that assert no side effects.
- Rename covers trunk/current selection and name collisions; delete covers
  trunk/current selection and descendants; move covers missing metadata and
  cycles; reorder covers empty, one-branch, forked, and stale stack shapes.
- Dirty-worktree and conflict coverage for history-rewriting operations.
- Receipt, `can_undo`, side-effect, and preferred-selection assertions.

Integration tests exercise the existing CLI/TUI adapters after migration and
prove migrated actions no longer construct legacy command sequences. GUI GPUI
tests cover buttons, menus, shortcuts, overlay priority, cancellation,
progress, completion, errors, safe undo/redo visibility, search, and persisted
pane restoration.

Focused verification uses package and module filters. Final Phase 3
verification uses the repository-required `make test`, plus
`cargo nextest run -p stax-gui --locked`, GUI clippy/check gates, and the
developer app bundle smoke test.

## Documentation Impact

Phase 3 updates:

- `README.md` for the expanded GUI capability summary.
- `docs/interface/gui.md` for structural operations, receipts, search, panes,
  menus, shortcuts, and recovery.
- `docs/commands/core.md` and `docs/commands/reference.md` where `st gui`
  capability summaries appear.
- `skills.md` so agents know which operations remain terminal-only.

## Exit Criterion

Phase 3 is complete when rename, delete, move/reparent, reorder, and safe local
undo/redo use one typed implementation across the existing adapters and the
GUI; search, pane persistence, menus, and main-TUI shortcuts are covered by
tests; the full repository suite passes; and no release-packaging work has
entered the branch.
