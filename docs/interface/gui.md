# GUI

The native Stax GUI is a Phase 2 developer preview for macOS. It shares the same typed repository operations as the CLI/TUI adapters, but it is currently distributed only as an unsigned local app bundle for contributors.

## Developer preview install

Build the unsigned local bundle:

```bash
make gui-app
```

Install and register the developer preview for the current user:

```bash
make install-gui-app
```

The install target writes `$HOME/Applications/Stax.app`. The provisional bundle id is `dev.stax.Stax`. Because this is unsigned, macOS may show normal local-app security prompts; final signing, notarization, release metadata, and packaged distribution are intentionally deferred.

## Launch and windows

Launch the GUI from a repository:

```bash
st gui
```

Or launch it for an explicit repository path:

```bash
st gui /path/to/repo
```

When `[path]` is omitted, Stax uses the current directory. The launcher canonicalizes the chosen path, then invokes LaunchServices with:

```bash
open -n -b dev.stax.Stax --args <canonical-path>
```

The `-n` flag is part of the contract. Every `st gui [path]` invocation opens a fresh app process/window and forwards exactly one canonical repository path after `--args`. If LaunchServices cannot find or start the bundle, run `make install-gui-app` and confirm `$HOME/Applications/Stax.app` exists.

## Workspace

The workspace shows the repository stack, the selected branch changes, and an inspector for branch actions and status. Background hydration refreshes CI, PR, and diff data without blocking normal browsing. Selecting a branch changes the visible details; it does not check out the branch until you explicitly run Checkout.

## Confirmed mutations

The GUI exposes a conservative subset of stack operations:

- **Checkout** checks out the selected tracked branch in the opened repository.
- **Create** creates an explicit-name empty child branch under the selected parent.
- **Restack** restacks the selected branch scope, while **Restack All** restacks all tracked non-trunk branches.
- **Stash-and-restack** appears only after an explicit dirty-worktree confirmation; Stax tells you which worktrees need stashing and keeps stashes if a conflict stops the rebase.
- **Submit Stack** first shows an explicit confirmation with the affected branches, `New pull requests: Draft`, and the remote warning. Confirming pushes branches and creates or updates PRs as Draft.

Submit does not show CLI prompts and does not auto-open PR pages. To inspect a PR, use Open PR on the selected branch.

## Shortcuts

Workspace shortcuts:

| Shortcut | Action |
|---|---|
| Enter | Check out selected branch |
| `n` | Create explicit-name child branch |
| `r` | Restack selected branch scope |
| Shift-R | Restack all tracked branches |
| `s` | Confirm submit current stack as Draft |
| `p` | Open PR for selected branch without checkout |
| Cmd-R | Refresh repository snapshot |
| Cmd-O | Open another repository |
| Up / Down | Move selection |

Overlay shortcuts are Enter to confirm and Escape to dismiss. Text input has priority over workspace shortcuts, so typing in the branch-name field does not trigger `n`, `r`, Shift-R, `s`, or `p`.

## Progress and receipts

Operations report structured progress with stage, branch, completed count, and total when available. Warnings are data, not terminal text, and remain visible in the operation banner.

Successful submit receipts show Created, Updated, or Unchanged PR rows with clickable HTTP(S) URLs. The banner can copy diagnostics for failures and can be dismissed after terminal success or failure. Dismissing the banner changes only GUI presentation; it does not remove persisted operation receipts or refresh data.

## Safety and recovery

Only one mutating operation runs at a time. While a mutation is active, checkout, create, restack, submit, Open Repository, Refresh, and navigation controls are disabled because a rebase or push cannot be cancelled safely once started. The GUI intentionally exposes no cancel control during an active mutation.

On success, and on failures that may have changed local or remote state, the GUI refreshes the repository snapshot and preserves the receipt or error. On guaranteed no-side-effect failures, it leaves the current snapshot in place. If a rebase stops for conflicts, resolve them in the repository with the normal CLI flow: inspect the worktree, fix conflicts, then run `st continue`, `st abort`, or `st resolve` as appropriate before retrying in the GUI.

## Current limits

The Phase 2 GUI intentionally leaves advanced workflows in the CLI:

- AI-generated branch names and PR details.
- Staging, commit creation, `--below`, `--insert`, custom branch prefixes, and other advanced create modes.
- Advanced submit options such as reviewers, labels, templates, AI prompting, ready-for-review mode, and auto-open behavior.

Icon work, final metadata, signing, notarization, universal binaries, release artifacts, and packaged distribution are Phase 4 work. The current app remains an unsigned developer preview installed locally with `make install-gui-app`.
