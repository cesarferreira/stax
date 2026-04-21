# Core commands

Day-to-day commands you'll use most. For the exhaustive list of every command, subcommand, and flag, see the [full reference](reference.md).

## Stack view and creation

| Command | What it does |
|---|---|
| `st` | Launch the interactive TUI |
| `st ls` | Show stack with PR and rebase status |
| `st ll` | Like `st ls` plus PR URLs and detail |
| `st create <name>` | Create a branch stacked on current |

## Submit and merge

| Command | What it does |
|---|---|
| `st ss` | Submit the whole stack — open or update linked PRs |
| `st merge` | Cascade-merge from stack bottom up to current branch |
| `st merge --when-ready` | Wait for CI + approvals, then merge (alias: `st mwr`) |
| `st merge --remote` | Merge remotely via the GitHub API while you keep working |
| `st merge --all` | Merge the entire stack regardless of where you are |
| `st cascade` | Restack, push, and create/update PRs in one shot |

## Sync, restack, refresh

| Command | What it does |
|---|---|
| `st rs` | Pull trunk, clean merged branches, reparent children |
| `st rs --restack` | `rs` **plus** rebase the current stack onto updated trunk |
| `st rs --delete-upstream-gone` | Also delete local branches whose upstream is gone |
| `st restack` | Rebase current stack onto parents locally (no fetch) |
| `st refresh` | `sync --restack` **plus** push and update PRs |

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
| `st standup` | Summarize recent activity (`--summary` for AI version) |
| `st pr` / `st pr list` | Open current PR in browser · list open PRs |
| `st issue list` | List open issues |
| `st changelog` | Generate changelog between refs (auto-resolves last tag) |
| `st open` | Open the repository in the browser |
| `st run <cmd>` | Run a command on each branch in the stack (alias: `st test <cmd>`) |
| `st demo` | Interactive tutorial — no auth or repo required |

See also: [Navigation](navigation.md) · [Stack health](stack-health.md) · [Full reference](reference.md)
