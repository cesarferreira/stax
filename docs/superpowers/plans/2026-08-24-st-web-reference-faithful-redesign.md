# `st web` reference-faithful redesign implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use the repository's `stax-dev`
> planner → implementer → verifier pipeline. Steps use checkbox syntax for
> tracking.

**Goal:** Make `st web` closely match the approved three-column reference while
preserving all routes, operations, keyboard pane toggles, and theme modes.

**Architecture:** Restructure the existing Maud markup and embedded CSS/JS
without adding a frontend framework. Keep the server API unchanged and use HTMX
out-of-band swaps for shell data that refreshes with branch or mutation state.

**Tech stack:** Rust, Maud, Axum, HTMX, embedded CSS and JavaScript.

---

## Phase 1: Lock the semantic layout with tests

**Files:**
- Modify: `tests/web_tests.rs`
- Modify: `src/web/templates.rs`
- Modify: `src/web/static_assets.rs`

- [ ] Update the initialized-workspace integration test to assert the new
  semantic shell: `stage`, `topbar-actions`, `branch-cards`, `quick-actions`,
  `review-workspace`, and `statusline`.
- [ ] Add a test proving the workspace renders only the Changes tab and omits
  `Commits` and `Stack preview`.
- [ ] Add a test proving System, Light, and Dark theme options remain present.
- [ ] Add a real-change diff test asserting `file-nav`, `diff-gutter`,
  `data-file-name`, and the diff-stat out-of-band fragment.
- [ ] Add focused unit tests for path shortening, hunk-header parsing, diff
  gutter numbering, and dark/system-dark theme-token parity.
- [ ] Run the focused tests and confirm they fail against the old layout:

```bash
cargo nextest run web_tests:: web::static_assets:: web::templates::
```

Expected result: the new semantic-layout assertions fail before implementation.

## Phase 2: Establish the visual token system

**Files:**
- Modify: `src/web/static_assets.rs`

- [ ] Replace duplicated palettes with raw light/dark scales plus semantic
  aliases for canvas, panels, text, borders, accent, success, warning, danger,
  and diff backgrounds.
- [ ] Match the reference in dark mode with off-black/cool-charcoal surfaces and
  one restrained violet accent.
- [ ] Map light mode to equivalent hierarchy and keep System mode following
  `prefers-color-scheme`.
- [ ] Define `--sans` and `--mono`; use monospace only for filenames, counts,
  SHAs when available, and diff content.
- [ ] Add panel/card/control radii, restrained shadows, visible focus rings,
  hover/pressed states, skeletons, and disabled states.
- [ ] Scope the existing global `form { display: contents; }` behavior so forms
  used by the new grids retain explicit layout.

## Phase 3: Rebuild the shell and top bar

**Files:**
- Modify: `src/web/templates.rs`
- Modify: `src/web/static_assets.rs`

- [ ] Replace the current toolbar/pane chrome with a branded top bar and a
  `stage` grid containing stack, review, and inspector panels.
- [ ] Keep all load-bearing ids and HTMX attributes: `banner`, `pane-stack`,
  `pane-changes`, `pane-inspector`, `stack-pane`, `changes-pane`,
  `inspector-pane`, `status-bar`, `search-input`, `theme-select`, and
  `help-template`.
- [ ] Keep project switching, repository-path entry, branch search, refresh,
  undo/redo, theme selection, help, Restack, Open PR, and Submit behavior.
- [ ] Render `Submit stack` as the dominant top-bar control; Restack and Open PR
  remain secondary.
- [ ] Send mutation banners to `#banner` with an HTMX out-of-band swap while the
  returned stack fragment replaces `#stack-pane`.
- [ ] Refresh the top-bar affordance state and status line via top-level
  out-of-band fragments after stack mutations.

## Phase 4: Replace the branch table with connected cards

**Files:**
- Modify: `src/web/templates.rs`
- Modify: `src/web/static_assets.rs`

- [ ] Render stack summary text with branch and PR counts.
- [ ] Replace table-like rows with connected branch cards while preserving each
  row's branch-selection request and checkout button behavior.
- [ ] Keep topology connectors, but stretch their rail over the full card
  height.
- [ ] Render selected/current, trunk, restack, PR, CI, and divergence metadata
  from existing model fields only.
- [ ] Add the approved quick actions: New branch, Restack stack, Submit stack,
  and Undo, each gated by its existing interaction affordance.
- [ ] Keep empty-search and uninitialized-stack messages unchanged.

## Phase 5: Build the review workspace

**Files:**
- Modify: `src/web/templates.rs`
- Modify: `src/web/static_assets.rs`

- [ ] Add a selected-branch header with commit count, changed-file count, and
  total additions/deletions.
- [ ] Render only one active tab, Changes.
- [ ] Change the Changes panel from vertically stacked file list and patch to a
  narrow file navigator beside a large diff pane.
- [ ] Preserve `.changes-panel`, `.file-row`, `data-diff-file`, and
  `diff-file-<id>` because the JavaScript navigation depends on them.
- [ ] Add `data-file-name` and update the diff header when a file is selected.
- [ ] Render old/new gutter numbers by parsing unified-diff hunk headers; keep
  headers and hunk markers unnumbered.
- [ ] Preserve the exact `No changes vs parent` empty-state text.
- [ ] Deliver diff-derived file and line totals to the review header through an
  out-of-band fragment without changing the `/diff` route signature.

## Phase 6: Turn branch details into an inspector

**Files:**
- Modify: `src/web/templates.rs`
- Modify: `src/web/static_assets.rs`

- [ ] Group branch identity, parent, divergence, remote state, pull-request
  state, CI state, and commit messages into readable inspector sections.
- [ ] Keep all current create, rename, delete, move, reorder, restack, submit,
  and Open PR operations and their confirmations.
- [ ] Pin `Submit stack` as the inspector's dominant bottom action, with Restack
  and Open PR secondary.
- [ ] Do not invent unavailable commit SHAs, draft state, or working-tree state.
- [ ] Restyle overlays, help, loading placeholders, empty states, and error
  banners with the same token system.

## Phase 7: Preserve and polish interactions

**Files:**
- Modify: `src/web/static_assets.rs`
- Modify: `src/web/templates.rs`

- [ ] Preserve `/`, Escape, and `1`/`2`/`3` pane shortcuts and pane-preference
  persistence.
- [ ] Add reference quick-action shortcuts for New, Restack, Submit, and Undo,
  guarded so they do not fire inside inputs or overlays.
- [ ] Preserve mutation-button disabling while requests are active.
- [ ] Add visible loading treatment to the affected panel during HTMX requests.
- [ ] Keep file selection, active state, diff-header sync, and smooth scrolling
  after each Changes-panel refresh.
- [ ] Update the shortcut help overlay to match implemented shortcuts only.

## Phase 8: Add responsive behavior

**Files:**
- Modify: `src/web/static_assets.rs`

- [ ] Keep the reference three-column grid on wide desktop screens.
- [ ] At medium widths, keep stack and review side by side and move the
  inspector below the review panel.
- [ ] At narrow widths, order review first, then stack, then inspector.
- [ ] Stack the file navigator above the diff at narrow widths.
- [ ] Prevent horizontal page scrolling and keep primary actions reachable.

## Phase 9: Documentation and verification

**Files:**
- Modify: `docs/interface/web.md`
- Modify only if stale: `README.md`
- Modify only if stale: `skills.md`

- [ ] Replace the GitKraken/table framing with the stack rail, review workspace,
  and branch inspector.
- [ ] Document the implemented shortcut set and state that Commits and Stack
  preview are intentionally omitted.
- [ ] Leave security and route documentation unchanged.
- [ ] Run focused checks:

```bash
make lint-fast
cargo nextest run web_tests:: web::
```

- [ ] Run completion gates:

```bash
make lint
make test
```

- [ ] Inspect the rendered workspace in Dark and Light at desktop width.
- [ ] Inspect at approximately 1100px and 850px to confirm inspector reflow,
  action reachability, and no horizontal page scrollbar.

## Invariants

- Do not change route paths, form fields, operation request types, or Git/GitHub
  semantics.
- Keep public template function signatures used by `src/web/routes.rs`
  unchanged unless compilation proves a minimal presentation input is required.
- Preserve CSRF inclusion and local-host/session guards.
- Keep HTMX out-of-band fragments as top-level siblings.
- Use only existing `BranchSummary`, `BranchDetails`, `BranchDiff`, and
  `InteractionState` data.
