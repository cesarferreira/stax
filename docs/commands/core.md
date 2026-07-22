# Core commands

Day-to-day commands you'll use most. For the exhaustive list of every command, subcommand, and flag, see the [full reference](reference.md).

## Stack view and creation

| Command | What it does |
|---|---|
| `st` | Launch the interactive TUI |
| `st gui [path]` | Launch a fresh native macOS GUI window for a repository |
| `st ls` | Show stack with PR, rebase, and metadata-repair status |
| `st ll` | Like `st ls` plus PR URLs and detail |
| `st create <name>` / `st add <name>` | Create a branch stacked on current |
| `st create --ai -a --yes` | Generate branch name + first commit message |
| `st create <name> --below` | Insert a new branch below current, carrying tracked/untracked prepared changes with it |
| `st get [branch|PR]` | Sync the current stack, or fetch a branch/PR stack from remote without overwriting local commits |

If you discover a hotfix while working upstack, keep the edits in place:

```bash
st create cve-hotfix --below
st create --below -am "fix: patch CVE-2026-0001"
```

`--below` auto-stashes prepared tracked and untracked changes before moving to the lower base, then reapplies them on the inserted branch. With `-am`, those changes are staged and committed on the new lower branch.
When `-m` or `--ai` derives a branch name that already exists, Stax stops instead of creating a suffixed duplicate; pass an explicit different name or checkout/reparent the existing branch.

## Submit and merge

| Command | What it does |
|---|---|
| `st ss` | Submit the whole stack — open or update linked PRs |
| `st stack link` | Register the current PR stack as a native GitHub Stack when `gh-stack` is available |
| `st stack unlink` | Unstack a locally tracked native GitHub Stack; stax-linked stacks may require `gh stack checkout <pr>` first |
| `st branch submit` | Submit only the current branch; if its parent is already synced to the remote, Stax may publish a temporary rebased head without moving your local branch |
| `st upstack submit` | Submit current branch and descendants; descendants are temporarily chained onto any temporary parent publish heads |
| `st draft [branch]` | Convert the current (or named) branch's PR to draft |
| `st undraft [branch]` | Mark the current (or named) branch's PR as ready for review |
| `st ready` | Open the interactive PR readiness dashboard for all tracked PRs: merge, ping, fix, wait, or draft |
| `st merge` | Cascade-merge from stack bottom up to current branch |
| `st merge --when-ready` | Wait for CI + approvals, then merge (alias: `st mwr`) |
| `st merge --downstack-only` / `--ds` | Merge ancestors below current, then rebase current branch |
| `st merge --stack` | Validate the selected tip PR once, retarget it to trunk, merge that PR, and let GitHub mark lower PRs merged when possible (`--full` includes descendants; GitHub only) |
| `st merge --remote` | Merge remotely via the GitHub API while you keep working |
| `st merge --all` | Merge the entire stack regardless of where you are |
| `st cascade` | Restack, push, and create/update PRs in one shot |

Scoped submit keeps local branch metadata unchanged when it prepares a temporary publish head. Plain `git commit` work on the branch is included; `st restack` remains the command that updates local branch tips and parent revisions.

On GitHub repos with native Stacked PRs enabled, `st ss`/`st bs` auto-register the submitted PRs with GitHub via `gh stack link` when the `github/gh-stack` extension is installed. Repos without access or users without the extension keep the normal stax stack links and see no behavior change. `gh-stack` v0.0.8+ supports normal GitHub CLI authentication, including token environment variables; stax keeps its token-stripping OAuth fallback only for known older versions. `st doctor` always reports the installed version and marks versions below v0.0.8 as out of date. `st stack unlink` delegates to `gh stack unstack`, which only works for stacks gh-stack tracks locally; if stax registered the stack, run `gh stack checkout <pr>` first or remove the native stack in the GitHub UI. See [Native GitHub Stacked PRs](../integrations/github-native-stacks.md).

## Sync, restack, update

| Command | What it does |
|---|---|
| `st rs` | Pull trunk, clean merged branches, reparent children |
| `st rs --restack` | `rs` **plus** rebase the current stack onto updated trunk |
| `st rs --delete-upstream-gone` | Also delete local branches whose upstream is gone |
| `st restack` | Rebase current stack onto parents locally (no fetch) |
| `st update` | Sync trunk without merged-branch cleanup, restack, then push and update PRs |
| `st update --force --yes --no-prompt` | Full update flow without sync or submit prompts |
| `st update --verbose` | Same as `st update`, with detailed sync/restack/submit timing |

## Branch housekeeping

| Command | What it does |
|---|---|
| `st sweep` | Classify all local branches: merged, upstream-gone, stale, active (read-only) |
| `st sweep --delete` | Delete merged branches (including tracked merged PRs) and upstream-gone branches with no unique work after confirmation |
| `st sweep --delete --include-stale` | Also delete stale branches (older than threshold) |
| `st sweep --delete --force` | Skip confirmation prompt |
| `st sweep --stale-days 60` | Override stale threshold (default 30, or `branch.stale_days` in config) |
| `st sweep --json` | Machine-readable output of all classified branches |

## Navigation and recovery

| Command | What it does |
|---|---|
| `st init` | Initialize stax or reconfigure the trunk |
| `st undo` / `st redo` | Rescue or reapply the last risky operation |
| `st resolve` | AI-resolve an in-progress rebase conflict and continue |
| `st abort` | Abort an in-progress rebase or conflict resolution |
| `st detach` | Remove a branch from the stack, reparent its children |

## Reporting and utility

| Command | What it does |
|---|---|
| `st standup` | Summarize recent activity (`--ai` for AI version, `--ai --style slack` for Slack-ready bullets) |
| `st pr` / `st pr body` / `st pr list` / `st pr list --ready` | Open current PR in browser · view/edit PR body · list open PRs · show stack PR readiness |
| `st issue list` | List open issues |
| `st changelog` | Generate changelog between refs or fuzzy-find commits with `find` / `--find` |
| `st open` | Open the repository in the browser |
| `st run <cmd>` | Run a command on each branch in the stack (alias: `st test <cmd>`) |
| `st doctor` / `st doctor --fix` | Check repo/config health; `--fix` applies safe local repairs after one confirmation |
| `st demo` | Interactive tutorial — no auth or repo required |

See also: [Navigation](navigation.md) · [Stack health](stack-health.md) · [Full reference](reference.md)

`st ready` and `st pr list --ready` launch a TUI in an interactive terminal and default to all tracked PRs, ordered newest changed PR first. Use `--current` or `--stack` for only the current stack. Use Up/Down or `j`/`k` to move, Enter or `o` to open the selected PR, `r` to refresh, `?` for help, and `q`/Esc to quit. Use `--plain` for the static table and `--json` for machine-readable output.

`st gui [path]` is macOS-only and launches the installed app with bundle id `com.cesarferreira.stax`. Download `Stax-aarch64-apple-darwin.zip` or `Stax-x86_64-apple-darwin.zip` from GitHub Releases and move `Stax.app` to `/Applications`; contributors can use `make install-gui-app` for `$HOME/Applications/Stax.app`. A pathless app launch restores the most recently opened project, and the toolbar project dropdown switches among recent repositories or adds another with the folder picker. The GUI can search and inspect stacks; check out, create, rename, delete, move, and reorder local branches; restack selected/all; submit Draft PRs; Open PR without checkout; and safely undo/redo fully local receipts. Pane visibility and widths persist per repository, while visible controls, native menus, and shortcuts use the same enabled-state rules.
