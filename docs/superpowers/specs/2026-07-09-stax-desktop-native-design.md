# Stax Desktop with Native SDK — Design

**Date:** 2026-07-09
**Status:** Approved
**Target:** macOS MVP
**Framework:** [vercel-labs/native](https://github.com/vercel-labs/native), pinned to Native SDK v0.4.1

## Summary

Build a self-contained macOS desktop application for stax using Native SDK. The app is a focused daily workspace for inspecting a stack, understanding the selected branch, reviewing its patch, and running four common actions: checkout, restack, submit stack, and open pull request.

The application uses a three-pane, keyboard-first “Workshop” interface rendered by Native SDK. It bundles the existing Rust `st` executable inside `Stax.app` and communicates with it through a narrow, versioned JSON protocol. The Rust engine remains the source of truth for Git, stack, pull-request, and CI behavior; the Zig application owns presentation and interaction state.

The MVP targets Apple Silicon and produces an ad-hoc-signed `.app`. Universal builds, Developer ID signing, notarization, and a DMG are follow-up distribution work.

## Goals

- Provide a useful desktop workspace for the most frequent stax inspection and action loops.
- Preserve the behavior and safety guarantees of the existing Rust engine.
- Ship as a self-contained app without requiring `st` on `PATH`.
- Render a real native interface with no browser or WebView.
- Keep the Rust–Zig boundary explicit, versioned, and independently testable.
- Make loading, mutations, conflicts, and failures understandable from the interface.
- Retain keyboard efficiency while making stax approachable without memorizing TUI bindings.

## Non-goals

- Full CLI or TUI feature parity.
- Branch creation, rename, delete, reorder, split, merge, sweep, undo, redo, or worktree management.
- An embedded terminal or arbitrary command execution.
- Background Git or GitHub polling.
- New credential storage or authentication behavior.
- Linux, Windows, Intel macOS, automatic updates, notarization, or App Store distribution in the MVP.
- Reimplementing stack or Git behavior in Zig.

## Chosen Architecture

### App bundle

`Stax.app` contains two executables:

- `Contents/MacOS/Stax` — the Native SDK application, written in Zig.
- `Contents/Resources/bin/st` — the existing Rust stax engine, built in release mode and copied into the app bundle during packaging.

The desktop process resolves the engine from the bundle. It never searches `PATH` and never substitutes an ambient `st` installation. A missing, non-executable, or incompatible bundled engine is reported as a bundle-integrity error.

### Native application

The Zig application follows Native SDK's model/message/update architecture:

- The model owns the selected repository, current snapshot, selected branch, selected diff, loading state, confirmation state, active operation, errors, and recent repositories.
- Messages represent user intent and engine responses.
- The update function is the only place state changes and effects are requested.
- Native SDK effects launch the engine asynchronously and return output to the update loop.
- Compiled `.native` markup renders the release interface; Debug builds retain markup hot reload.

### Rust engine

The Rust binary gains an internal, hidden `desktop` command family:

```text
st desktop snapshot --schema-version 1 --request-id <id>
st desktop diff --schema-version 1 --request-id <id> --branch <name>
st desktop action --schema-version 1 --request-id <id> --action <action> [--branch <name>]
```

Every invocation receives the selected repository through `--repo <path>`. Native SDK v0.4.1 subprocess effects do not expose a child working-directory option, so the app never relies on its own process directory. The command family adapts existing `GitRepo`, `Stack`, forge, diff, and command-handler behavior into stable machine output. It does not become a second implementation of those behaviors.

The allowed actions are:

- `checkout` — checkout the selected branch.
- `restack` — restack the selected branch using existing stax safeguards.
- `submit-stack` — submit the selected branch's stack using existing stax safeguards.
- `open-pr` — open the selected branch's pull request using existing stax behavior.

No request accepts a shell command, free-form executable, or arbitrary stax argument list.

## Engine Protocol

### Transport

The Native app invokes the bundled engine with an argument array, never through a shell. Snapshot and diff use Native SDK collect mode because each returns one terminal JSON document. Actions use line mode so progress events stream before the terminal result. Standard error is retained only as diagnostic context and is never parsed as application state.

Native SDK v0.4.1 caps collect-mode output at 512 KiB. Structured patch text is therefore capped at 448 KiB before serialization and returns `truncated: true` when clipped; the remaining space is reserved for the envelope and metadata. The Patch pane renders a visible truncation notice, so transport truncation is never mistaken for a complete diff.

All protocol events include:

```json
{
  "schema_version": 1,
  "request_id": "req-42",
  "type": "progress"
}
```

A progress event adds a stable phase and user-facing message:

```json
{
  "schema_version": 1,
  "request_id": "req-42",
  "type": "progress",
  "phase": "pushing",
  "message": "Pushing feat/dashboard"
}
```

A successful terminal event contains typed data:

```json
{
  "schema_version": 1,
  "request_id": "req-42",
  "type": "result",
  "ok": true,
  "data": {}
}
```

A failed terminal event contains a stable error code, a concise message, optional details, and recovery guidance:

```json
{
  "schema_version": 1,
  "request_id": "req-42",
  "type": "result",
  "ok": false,
  "error": {
    "code": "dirty_repository",
    "message": "The repository has uncommitted changes.",
    "details": "Commit or stash the changes before restacking.",
    "recovery": "refresh"
  }
}
```

Domain failures produce a terminal error event and a non-zero process exit. The app still parses the terminal event before consulting the exit status. A crash, signal, missing terminal event, malformed JSON, or incompatible schema becomes a `bridge_failure` in the UI.

### Snapshot data

The snapshot response contains:

- Canonical repository path and display name.
- Trunk and current branch.
- Repository operation state: normal, rebase in progress, merge conflict, or unavailable.
- Branches in deterministic display order.
- For each branch: name, parent, display depth/column, current/trunk markers, ahead/behind counts, restack state, remote state, pull-request summary, CI summary, and recommended next action.
- A snapshot generation identifier used to associate lazy detail responses.

The engine, not the UI, computes branch order and recommended actions. This prevents the desktop and TUI from developing different stack semantics.

### Diff data

The diff response contains:

- Branch and parent/base branch.
- Total additions, deletions, and changed files.
- Files in display order, each with path and additions/deletions.
- Structured diff lines with a kind (`file`, `hunk`, `context`, `addition`, `deletion`, or `metadata`) and text.
- The snapshot generation identifier used for the request.

The UI does not parse ANSI terminal output. If a diff response belongs to an older generation or a no-longer-selected branch, it is ignored.

### Concurrency

- Snapshot and diff reads may be in flight together.
- Only one mutating action may run at a time.
- Starting a mutation suspends automatic reads and disables other mutation controls.
- When a mutation completes, the app loads a fresh snapshot and then a fresh diff for the surviving selection.
- Request IDs and snapshot generations prevent late responses from overwriting newer state.

## Product Experience

### First launch and repository selection

On first launch, the app presents a native folder picker. A chosen folder is accepted only if the engine can open it as a Git repository and produce a snapshot. Invalid folders remain visible with a specific error and a “Choose another folder” action.

The app stores the canonical paths of the ten most recent valid repositories in its application-support directory. Later launches reopen the last valid repository. The titlebar repository menu exposes recent repositories and “Open Repository…”. If the last repository no longer exists, startup falls back to the picker without treating that as a crash.

### Window and visual system

- Initial size: 1180 × 760 points.
- Minimum size: 880 × 560 points.
- Hidden-inset macOS titlebar with the content header acting as the drag surface.
- Dark Workshop palette: charcoal surfaces, warm amber selection/action color, green success, red failure, restrained borders, and dense monospaced data.
- System font for branch titles and primary labels; monospaced font for branches, status values, and diffs.
- Three resizable panes, initially approximately 32%, 29%, and 39% of the content width.
- The app follows macOS appearance for chrome, but the MVP workspace remains dark to preserve the chosen Workshop identity.

### Stack pane

The left pane owns navigation and shows:

- Stack tree in deterministic engine order.
- Current branch, trunk, and parent relationships.
- Ahead/behind, restack, pull-request, and CI indicators.
- Keyboard and click selection.
- Branch-name filtering with Command-F.

Selecting a row changes the inspected branch and starts a lazy diff request. Selection never checks out a branch. Return or the explicit Checkout action performs checkout.

### Branch inspector

The center pane owns decisions and shows:

- Selected branch and parent.
- Ahead/behind and clean/dirty state.
- Pull-request and CI summaries.
- Recommended next action calculated by the engine.
- Checkout, Restack, Submit Stack, Open PR, and Refresh controls.
- Current operation phase and progress message.

Restack and Submit Stack require confirmation dialogs. Checkout and Open PR do not. Controls explain why they are disabled when an operation or repository state makes them unavailable.

### Patch pane

The right pane owns evidence and shows:

- Diffstat totals and changed-file count.
- File sections with per-file additions/deletions.
- A virtualized, scrollable structured patch.
- Clear loading, empty, binary-file, and unavailable states.

The initial snapshot renders without waiting for the diff. Changing selection cancels logically by invalidating the previous request; a late result is discarded.

### Refresh behavior

The app refreshes:

- When a repository opens.
- When the app regains focus.
- When the user presses Command-R or selects Refresh.
- After every completed engine action.

There is no timer-based polling in the MVP. A focus refresh that detects an active mutation waits for that mutation to finish.

### Keyboard behavior

- Up/Down or J/K selects the previous/next branch while the stack pane is focused.
- Return checks out the selected branch.
- Command-F focuses branch filtering.
- Command-R refreshes the repository.
- Command-Shift-S submits the stack after confirmation.
- Command-Shift-R restacks after confirmation.
- Command-O opens the selected pull request.
- Tab and Shift-Tab move focus among panes and controls.
- Escape closes a filter, confirmation, or error details before affecting the window.

All actions remain reachable through visible controls; shortcuts are accelerators, not hidden functionality.

## Safety and Error Handling

### Execution safety

- The engine path is resolved inside the app bundle.
- Engine requests use argument arrays with no shell interpolation.
- Action names are an enum on both sides of the protocol.
- The UI never accepts arbitrary arguments or commands.
- Existing stax repository-state policies and transactional safeguards remain authoritative.
- The desktop app does not read, copy, or persist tokens. Existing stax authentication and forge clients are reused.

### Error presentation

Errors preserve the last valid snapshot whenever it remains safe to display it. The affected pane shows a concise error; an expandable details region provides diagnostics without overwhelming the primary view.

Recovery actions are selected from a fixed set:

- Retry the failed read or action.
- Refresh the repository.
- Choose another repository.
- Copy diagnostics.
- Copy an appropriate CLI recovery command for an in-progress rebase or conflict.

Important states behave as follows:

| State | Desktop behavior |
| --- | --- |
| Invalid or missing repository | Show repository error and folder picker action. |
| Dirty repository blocks action | Preserve data, explain the precondition, offer refresh. |
| Rebase or conflict in progress | Disable unsafe actions and show the exact state plus CLI recovery guidance. |
| GitHub unauthenticated | Preserve local stack data, mark remote data unavailable, provide existing stax auth guidance. |
| Network or forge failure | Preserve local data, mark remote sections stale, allow retry. |
| Engine crash or malformed output | Show bundle/bridge failure, capture exit and stderr details, allow copy diagnostics. |
| Schema mismatch | Refuse to continue with that engine and report an incompatible app bundle. |

## Persistence

The Native app persists only UI preferences:

- Last valid repository.
- Up to ten recent repositories.
- Pane widths.
- Last focused pane.

It does not duplicate branch, stack, CI, or authentication state. Repository state remains in the existing stax/Git data locations.

## Packaging

The desktop project owns an explicit build/package step because the Rust sidecar must be staged into the final bundle.

The packaging flow is:

1. Build the Rust `st` release binary for Apple Silicon.
2. Run `native check`, `native test`, and `native build` for the Zig app.
3. Run `native package --target macos` to create `Stax.app`.
4. Copy the exact Rust binary built in step 1 to `Contents/Resources/bin/st` and mark it executable.
5. Ad-hoc sign the complete bundle after the sidecar is present.
6. Run a package smoke test that launches the app and proves the bundled engine can inspect a fixture repository.

The manifest uses a stable bundle identifier and declares macOS 11 as the minimum supported system, matching Native SDK's current macOS packaging floor. The app icon uses the chosen Workshop language: a simple warm-amber `st` mark on charcoal.

Developer ID signing, notarization, a universal Apple Silicon/Intel bundle, and DMG production are explicitly deferred.

## Testing Strategy

### Rust engine tests

Protocol serialization and parsing receive unit tests for:

- Snapshot, diff, progress, success, and error envelopes.
- Schema-version rejection.
- Unknown actions and missing action fields.
- Stable error codes and recovery values.
- Structured diff-line conversion, including empty and binary diffs.

End-to-end tests run the real `st` binary in temporary repositories and cover:

- Snapshot happy path for a multi-branch stack.
- Snapshot outside a Git repository.
- Diff happy path, empty diff, missing branch, and binary file.
- Checkout happy path and dirty-worktree rejection.
- Restack happy path and conflict/rebase failure state.
- Submit request validation and an isolated remote failure path.
- Open-PR behavior when no PR exists.

The new integration-test module is registered in `tests/all_tests.rs`. Tight loops use `cargo nextest run desktop_tests::`; final repository validation uses `make test` as required by project policy.

### Native SDK tests

Pure Zig tests cover:

- Every protocol event parser and malformed input.
- Model transitions for loading, success, progress, failure, and retry.
- Request-ID and generation handling for stale responses.
- One-mutation-at-a-time enforcement.
- Recent-repository ordering, de-duplication, truncation, and missing paths.

Native SDK `TestHarness` tests cover:

- Snapshot rendering into the three panes.
- Mouse and keyboard branch selection.
- Lazy diff loading and stale diff rejection.
- Confirmation behavior for restack and submit.
- Action progress and post-action refresh.
- Invalid repository, dirty state, conflicts, auth failure, network failure, malformed engine output, and schema mismatch.
- Empty stacks, long branch names, many branches, large diffs, and the minimum window size.
- Accessibility labels and keyboard reachability for all controls.

The desktop gate runs `native check`, `native test`, and `native build`.

### Packaged-app smoke test

The packaging test verifies:

- Both executables exist at their specified bundle paths.
- The sidecar is executable and reports the expected protocol version.
- The bundle passes an ad-hoc code-sign verification.
- The packaged app launches against a fixture repository through Native SDK automation.
- A snapshot appears and selecting a branch loads its patch using the bundled engine rather than an ambient `st`.

## Documentation

The implementation updates:

- `README.md` with the desktop app capability and build/run entry point.
- A new desktop interface page under `docs/interface/` covering installation, repository selection, panes, actions, keyboard controls, limitations, and recovery behavior.
- `skills.md` with the desktop surface, supported actions, and the boundary between desktop and CLI-only workflows.

The internal `st desktop` protocol is documented for contributors, but it is hidden from the normal user command reference because it is not a supported human-facing command surface.

## Acceptance Criteria

The MVP is complete when:

1. A user can launch a self-contained Apple Silicon `Stax.app` without installing `st` separately.
2. The user can choose a valid repository and reopen it on the next launch.
3. The three-pane Workshop UI displays the stack, selected branch status, and a lazy structured patch.
4. The user can checkout, restack, submit the stack, and open the selected PR through visible controls and documented shortcuts.
5. Restack and submit require confirmation, mutations cannot overlap, and action completion refreshes the workspace.
6. Invalid repositories, dirty states, conflicts, auth/network failures, bridge failures, and schema mismatches produce actionable errors without corrupting UI state.
7. Rust protocol tests, Native SDK tests, the full `make test` suite, Native SDK checks/build, and the packaged-app smoke test pass.
8. `Stax.app` contains the exact bundled Rust engine used by the smoke test and passes ad-hoc signature verification.
9. README, desktop documentation, and `skills.md` describe the shipped behavior and MVP limitations.

## Deferred Follow-up

- Full TUI/CLI command coverage.
- Worktree dashboard and stacked-PR readiness views.
- Live filesystem or remote polling.
- Universal macOS builds and Intel validation.
- Developer ID signing, notarization, DMG distribution, and updates.
- Linux and Windows ports after the macOS architecture and protocol prove stable.
