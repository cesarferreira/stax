# Core commands

Day-to-day commands you'll use most. For the exhaustive list of every command, subcommand, and flag, see the [full reference](reference.md).

## Stack view and creation

| Command | What it does |
|---|---|
| `st` | Launch the interactive TUI |
| `st ls` | Show stack with PR, rebase, and metadata-repair status |
| `st ll` | Like `st ls` plus PR URLs and detail |
| `st create <name>` / `st add <name>` | Create a branch stacked on current |
| `st create --ai -a --yes` | Generate branch name + first commit message |
| `st create <name> --below` | Insert a new branch below current, carrying tracked/untracked prepared changes with it |

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
| `st draft [branch]` | Convert the current (or named) branch's PR to draft |
| `st undraft [branch]` | Mark the current (or named) branch's PR as ready for review |
| `st merge` | Cascade-merge from stack bottom up to current branch |
| `st merge --when-ready` | Wait for CI + approvals, then merge (alias: `st mwr`) |
| `st merge --downstack-only` / `--ds` | Merge ancestors below current, then rebase current branch |
| `st merge --remote` | Merge remotely via the GitHub API while you keep working |
| `st merge --all` | Merge the entire stack regardless of where you are |
| `st cascade` | Restack, push, and create/update PRs in one shot |

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
| `st pr` / `st pr body` / `st pr list` | Open current PR in browser · view/edit PR body · list open PRs |
| `st issue list` | List open issues |
| `st changelog` | Generate changelog between refs or fuzzy-find `CHANGELOG.md` entries with `find` / `--find` |
| `st open` | Open the repository in the browser |
| `st run <cmd>` | Run a command on each branch in the stack (alias: `st test <cmd>`) |
| `st doctor` / `st doctor --fix` | Check repo/config health; `--fix` applies safe local repairs after one confirmation |
| `st demo` | Interactive tutorial — no auth or repo required |

See also: [Navigation](navigation.md) · [Stack health](stack-health.md) · [Full reference](reference.md)
