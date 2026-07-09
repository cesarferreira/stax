# Desktop app

Stax for macOS is a native three-pane workspace for the daily inspect-and-act loop around a branch stack. It is built with Native SDK and uses the same Rust engine as the CLI.

## Platform and installation

The first release target is Apple Silicon macOS 11 or newer. Intel macOS, Linux, Windows, notarization, a DMG, automatic updates, and App Store distribution are not part of this version.

To build it from source, install Rust, Node.js/npm, Zig 0.16, and `jq`, then run:

```bash
make desktop-package
open desktop/dist/Stax.app
```

This creates an ad-hoc-signed app at `desktop/dist/Stax.app` and runs its package smoke test. The bundle contains both the Native SDK application and a release `st` engine at `Contents/Resources/bin/st`; the app never substitutes an ambient `st` from `PATH`.

For development with `.native` markup hot reload:

```bash
make desktop-dev
```

## Choose a repository

On first launch, Stax opens the native macOS folder picker. Choose a Git repository; invalid or missing repositories produce an actionable error and let you choose another folder. The app remembers up to ten recent repository paths in `~/Library/Application Support/Stax/` and reopens the latest one on the next launch. It does not store GitHub tokens or duplicate stax repository state.

Use the folder button in the titlebar to switch repositories at any time.

## The Workshop

The window is divided into three resizable panes:

1. **Stack** lists branches in deterministic stack order. The current and selected branches are marked, distance from the parent is shown, and the filter narrows branch names.
2. **Branch** shows the selected branch's parent, ahead/behind distance, pull request, CI state, recommended next step, and available actions.
3. **Patch** lazily loads the structured diff for the selected branch. Additions, deletions, files, and hunks use distinct semantic colors. Very large patches are capped at 448 KiB and display a visible truncation notice.

The app refreshes after opening a repository, when it becomes active, after an action finishes, and when Refresh is requested. Late responses for a previous selection are ignored.

## Actions

The desktop app deliberately exposes four operations:

- **Checkout** switches to the selected branch.
- **Restack** rebases the selected branch through existing stax safeguards and requires confirmation.
- **Submit Stack** pushes and creates or updates the stack's pull requests and requires confirmation.
- **Open Pull Request** opens the selected branch's existing PR.

Only one mutating action can run at a time. The app invokes an allow-listed bundled-engine command with an argument array; it has no shell or arbitrary command surface.

Branch creation, reorder/split/detach, merge and cascade, worktrees and AI lanes, metadata repair, undo/redo, authentication, configuration, and other advanced workflows remain available in the CLI and TUI.

## Keyboard controls

| Shortcut | Action |
| --- | --- |
| `Command-R` | Refresh repository |
| `Command-F` | Focus the branch filter |
| `Command-Shift-R` | Restack selected branch |
| `Command-Shift-S` | Submit the stack |
| `Command-O` | Open the selected pull request |
| `Escape` | Dismiss the active dialog or error |
| `Up` / `K` | Select the previous branch |
| `Down` / `J` | Select the next branch |
| `Return` | Checkout the selected branch |

The Stack pane also exposes the standard accessible tree navigation when its rows have keyboard focus.

## Errors and recovery

Errors stay in the workspace rather than replacing it. The dialog explains the failure and offers the relevant recovery actions:

- **Retry** refreshes the repository.
- **Choose Repository** opens the folder picker.
- **Copy Diagnostics** places the bounded engine error text on the clipboard.

An incompatible protocol schema or missing bundled engine is treated as a damaged app bundle; rebuild or reinstall the app. Authentication and network behavior remain owned by existing stax configuration. A malformed, interrupted, or truncated engine response is rejected rather than partially adopted.
