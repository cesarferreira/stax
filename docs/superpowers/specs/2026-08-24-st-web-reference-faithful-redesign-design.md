# `st web` reference-faithful redesign

## 1. Goal

Redesign the existing localhost web workspace to closely match the approved
reference while preserving its current HTMX behavior, repository operations,
and System / Light / Dark theme preference.

The workspace should feel native to the web rather than reproduce the terminal
UI: the stack is visually dominant, files have a dedicated navigator, the diff
uses the available width, and branch details form a real inspector.

## 2. Workspace layout

Use a reference-faithful three-column desktop layout:

1. **Stack rail** — connected branch cards, stack summary, and compact quick
   actions.
2. **Review workspace** — selected branch summary followed by a narrow changed
   file navigator and a large diff pane.
3. **Branch inspector** — branch, divergence, remote, PR, commit, and operation
   details with actions grouped at the bottom.

`Submit stack` is the only visually dominant action. Restack, Open PR, refresh,
undo, redo, and branch-level operations remain reachable but secondary.

The review workspace includes only the existing Changes experience. The
reference's Commits and Stack preview tabs are omitted rather than rendered as
inactive controls.

## 3. Visual system

Dark mode should closely match the reference:

- off-black and cool-charcoal surfaces;
- one restrained violet accent;
- subtle separators and borders;
- normal UI typography for navigation and status;
- monospace typography only for filenames, SHAs, counts, and diff content;
- selected branches rendered as distinct cards rather than highlighted table
  rows;
- clear hover, pressed, focus, loading, disabled, empty, and error states.

Light mode uses the same hierarchy and spacing with light equivalents. System
mode continues to follow the operating-system preference.

## 4. Interaction behavior

Existing operation contracts and routes remain unchanged.

- Selecting a branch refreshes the stack, diff, and inspector.
- Selecting a file gives it a strong active state and scrolls its diff into
  view.
- Existing keyboard shortcuts and persisted pane preferences remain supported.
- Mutation controls continue to disable while an operation is in flight.
- Existing confirmation and error behavior remains available in the redesigned
  presentation.

On narrower viewports, the main review area remains primary and the inspector
moves below it. The interface must remain usable without horizontal page
scrolling.

## 5. Implementation boundaries

Restructure the existing Maud templates and embedded CSS/JavaScript. Do not add
a frontend framework, change route contracts, or rewrite repository operations.

Expected production files:

- `src/web/templates.rs`
- `src/web/static_assets.rs`
- `tests/web_tests.rs`
- `docs/interface/web.md`

`src/web/routes.rs` should change only if template inputs require a minimal
presentation-specific addition; operation behavior stays out of scope.

## 6. Verification

Verification must cover:

1. Rust formatting and repository lint targets.
2. Focused `web_tests::` integration tests.
3. The full repository test suite through `make test`.
4. Rendered desktop inspection in dark and light modes.
5. A narrow viewport inspection confirming the inspector reflows and all
   primary actions remain reachable.

Tests should assert the redesigned semantic layout and retain coverage for
security guards, static assets, initialized repositories, and empty diffs.

## 7. Non-goals

- Implementing Commits or Stack preview tabs.
- Changing Git or GitHub operation semantics.
- Introducing a JavaScript framework or external design dependency.
- Removing System or Light theme support.
