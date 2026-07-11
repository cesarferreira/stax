# Stax GPUI Desktop App Design

## Status

Approved for implementation planning on 2026-07-11.

## Summary

Stax will gain a native macOS desktop application built with GPUI. The first
public release will manage one repository per window and reach feature parity
with the main `st` TUI while preserving the CLI and TUI as first-class
interfaces.

The app will ship as `Stax.app` and open from either Finder/Spotlight or a new
`st gui` command. Its main window will use a three-pane cockpit with a native
graphite visual language:

- A searchable stack tree on the left.
- A file list, diffstat, and patch viewer in the center.
- Branch details, CI, PR state, commits, and contextual actions on the right.

The GUI will live in a separate workspace crate over a new typed,
presentation-neutral application layer in the root `stax` library. GPUI will
not become part of normal CLI builds.

## Goals

- Provide a polished native macOS workflow for browsing and operating on a
  stax repository.
- Match the main TUI's branch and stack operations: checkout, create, rename,
  delete, move/reparent, reorder, restack selected/all, submit, and open PR.
- Reuse the same business rules, transaction receipts, caches, and undo
  semantics as the CLI and TUI.
- Render local state immediately and hydrate diffs, PRs, and CI without
  blocking the window.
- Preserve keyboard efficiency while adding discoverable mouse controls and
  native macOS interaction patterns.
- Keep the GUI dependency and release lifecycle isolated from the CLI package.

## Non-goals

- Linux or Windows support in the first public release.
- Multiple repositories inside one window.
- Replacing or deprecating the CLI or TUI.
- GUI coverage for the dedicated split, hunk-split, ready, or worktree TUIs in
  the first release.
- An automatic application updater in the first release.
- Parsing human-readable CLI output or driving normal operations through
  subprocesses.

## Product Experience

### Launch and repository selection

`Stax.app` opens a welcome window when no repository was supplied. The welcome
window lists recent repositories and offers an Open Folder action. A repository
opens in its own window.

`st gui` asks macOS to open the installed app by bundle identifier and passes
the current repository path. An optional path argument may open another
repository. If the app is not installed, the command returns an actionable
installation error rather than silently falling back.

Before opening the cockpit, the app verifies that the path is a Git repository
and that stax metadata is initialized. Invalid paths, non-Git directories, and
uninitialized repositories receive dedicated recovery actions.

### Main window

The toolbar shows the repository and current branch, refresh state, and
stack-level actions. The content area has three resizable panes:

1. **Stack pane**
   - Tree topology, current branch, and trunk.
   - PR, CI, ahead/behind, remote, and restack indicators.
   - Search and keyboard navigation.
   - Selection without checkout; checkout remains an explicit action.

2. **Changes pane**
   - File list and aggregate diffstat.
   - Scrollable syntax-colored patch.
   - Lazy loading with stale-request protection.
   - Cached content while a refresh is in progress.

3. **Inspector pane**
   - Parent, remote, PR, CI, commit, and stack-health summaries.
   - Recommended next action.
   - Context-sensitive branch and stack operations.
   - Structured operation progress and the latest receipt.

Pane sizes and visibility are persisted per repository. The existing TUI
keyboard vocabulary remains available where it maps cleanly, while standard
macOS menu commands and shortcuts provide discoverable equivalents.

### Visual direction

The app uses a native graphite style: quiet macOS surfaces, restrained blue
accent color, compact information density, SF system typography for interface
elements, and monospace typography for branch metadata and patches. It follows
the macOS light or dark appearance and uses color primarily for status and
diff semantics.

Destructive actions use native confirmation sheets. Reorder uses a preview
before applying changes. Empty, loading, stale, and failure states retain the
same pane geometry to avoid layout jumps.

## Architecture

### Workspace structure

The root manifest becomes a workspace while retaining the existing root
`stax` package as the default member. A new `crates/stax-gui` package contains:

- GPUI and macOS platform dependencies.
- Application startup and window coordination.
- Views, components, actions, key bindings, and menus.
- GUI-specific preferences and recent-repository state.
- App bundle metadata and packaging configuration.

The existing CLI install path and default build do not compile GPUI. GPUI is
pre-1.0, so the GUI crate pins compatible versions and updates them
deliberately.

### Shared application layer

The root library gains a presentation-neutral application module. Its public
surface consists of typed state and operations rather than terminal output:

- `RepositorySession` owns repository-scoped dependencies and refresh state.
- `RepositorySnapshot` contains stack topology and branch summaries.
- Detail requests return diffs, commits, PR state, and CI state.
- `OperationRequest` describes a validated mutation.
- `OperationEvent` reports progress, completion, cancellation, or failure.
- `OperationReceipt` exposes recovery and undo information.

This layer wraps existing engine, Git, forge, cache, and transaction code. It
must not depend on GPUI, Ratatui, terminal state, prompts, or process-global
output.

CLI and TUI presentation adapters will move onto the same operations as they
are extracted. The migration may be incremental, but an operation is not
considered available in the GUI until both interfaces share the same
underlying implementation.

### GPUI state model

GPUI entities own presentation state only:

- Open windows and recent repositories.
- Current snapshot and selected branch.
- Pane size, visibility, search, and scroll state.
- Active detail-request generation.
- Active operation and progress presentation.
- Modal and confirmation state.

Repository, GitHub, and cache work runs away from the render thread. Every
detail request carries a monotonically increasing generation identifier.
Results update the selected branch only when their repository, branch, and
generation still match.

## Data Flow

### Read path

1. Resolve and validate the repository path.
2. Create a `RepositorySession`.
3. Load local stack topology and cached details.
4. Render the cockpit immediately.
5. Hydrate selected-branch details, PR state, and CI concurrently.
6. Publish typed updates to the GPUI entity.
7. Ignore cancelled or stale generations.

The GUI and TUI continue sharing caches under the repository's common
`.git/stax` directory.

### Write path

1. Convert a UI action into an `OperationRequest`.
2. Validate current repository state and operation preconditions.
3. Present a native confirmation when the operation is destructive or
   topology-changing.
4. Execute through the shared application layer.
5. Stream structured progress to the window.
6. Apply the result and refresh the repository snapshot.
7. Present the receipt, undo availability, and any follow-up action.

Only one mutating operation may run per repository session. Read hydration may
continue when safe, but mutation completion invalidates relevant generations
and triggers a fresh snapshot.

## Error Handling

Errors are typed into user-actionable categories:

- Invalid or unavailable repository.
- Stax initialization required.
- Authentication or authorization failure.
- Dirty worktree or failed precondition.
- Rebase conflict.
- Local Git operation failure.
- GitHub or network failure.
- Partial remote update.
- Unsupported or unavailable platform capability.

The primary message explains what happened and the safest next action.
Expandable diagnostics preserve the underlying error chain and support copying
details. Authentication errors direct users to the existing stax auth flow.
Conflicts preserve the repository's in-progress state and expose the existing
continue, abort, and resolve paths.

The GUI does not create alternative transaction or recovery semantics.
Destructive operations continue using existing snapshots, receipts, undo, and
redo behavior.

## Phased Delivery

### 1. Shared core and read-only shell

- Convert the root package into a workspace without changing default CLI
  builds.
- Add the GPUI app crate and macOS window startup.
- Extract typed repository snapshots and detail requests.
- Add the welcome window, folder picker, recent repositories, and cockpit.
- Render stack, branch details, cached diffs, PR state, and CI.
- Establish stale-request rejection and baseline accessibility.

Exit criterion: the app can open a repository and remain responsive while
showing the same read state as the TUI.

### 2. Everyday operations

- Extract shared checkout, create, restack selected/all, submit, and open-PR
  operations.
- Add contextual actions, confirmations, progress, completion, and error
  presentation.
- Add the `st gui` command and installed-app diagnostics.
- Preserve keyboard navigation and common TUI shortcuts.

Exit criterion: normal daily stack work can be completed without returning to
the terminal.

### 3. Structural and destructive parity

- Extract rename, delete, move/reparent, and reorder operations.
- Add reorder preview/apply and destructive confirmation sheets.
- Surface receipts and undo availability.
- Add search, pane persistence, menu commands, and complete main-TUI keyboard
  parity.

Exit criterion: every operation in the main stack TUI is available through the
shared core and GUI.

### 4. macOS release hardening

- Add final iconography, metadata, bundle identifier, and app menus.
- Sign and notarize Apple Silicon and Intel app artifacts.
- Add packaged-app and `st gui` launch smoke tests.
- Complete keyboard, VoiceOver, focus, reduced-motion, and contrast checks.
- Update README, GUI documentation, command references, and `skills.md`.
- Measure large-stack rendering, diff scrolling, startup, and refresh behavior.

Exit criterion: downloadable release artifacts pass automated and manual
macOS acceptance checks.

## Testing Strategy

Every extracted operation receives:

- Happy-path coverage.
- Failure/precondition coverage.
- Boundary and stale-state coverage.
- Temp-repository integration coverage when Git behavior is involved.

GPUI tests cover:

- Snapshot-to-view-model transitions.
- Selection and detail-request generations.
- Rejection of stale async results.
- Action enablement and keyboard dispatch.
- Confirmation and cancellation paths.
- Progress, completion, and typed error presentation.
- Per-repository preference restoration.

Each phase uses focused unit and `cargo nextest` runs. Final verification uses
the repository-standard `make test`, then launches the packaged app on macOS,
opens a fixture repository, and verifies the `st gui` handoff.

## Distribution

GitHub Releases will publish signed and notarized macOS artifacts for Apple
Silicon and Intel. The CLI and app share a version but remain independently
buildable. The existing CLI distribution continues to work without GPUI.

The first release does not include an automatic updater. Installation
documentation explains how the app and CLI relate, and `st gui` provides a
direct installation hint when it cannot locate the app.

## Documentation Impact

Implementation must update:

- `README.md` for installation, launch, and the primary GUI capability.
- `docs/interface/gui.md` for the complete desktop workflow.
- Command documentation for `st gui`.
- `skills.md` for agent-visible command and workflow guidance.

The design-only pull request does not change user-visible behavior and
therefore requires no other documentation updates.

## Key Risks and Mitigations

- **GPUI API churn:** isolate and pin it in the GUI crate; keep framework types
  out of the shared core.
- **Behavior drift:** require GUI actions to use the same extracted operations
  as CLI/TUI.
- **Render-thread stalls:** perform Git, cache, and network work asynchronously
  and test responsiveness with large stacks and patches.
- **Stale async updates:** tag all detail requests and invalidate generations
  after selection or mutation changes.
- **Packaging complexity:** introduce signing, notarization, and architecture
  matrix work only after functional parity is stable.
- **Oversized first release:** preserve the four implementation phases and
  enforce each exit criterion before starting the next.
