# Core Commands

| Command | What it does |
|---|---|
| `st` | Launch interactive TUI |
| `st ls` | Show stack with PR and rebase status |
| `st ll` | Show stack with PR URLs and full details |
| `st create <name>` | Create branch stacked on current |
| `st ss` | Submit stack and create/update PRs |
| `st merge` | Merge PRs from stack bottom to current |
| `st merge --when-ready` | Merge in explicit wait-for-ready mode (legacy alias: `st merge-when-ready`) |
| `st rs` | Pull trunk and clean merged branches (no rebasing; confirmed cleanup can remove safe linked worktrees) |
| `st rs --restack` | Pull trunk, clean merged branches, **then** rebase current stack |
| `st restack` | Rebase current stack onto parents (local only, no fetch/delete; `--stop-here` skips descendants) |
| `st rs --delete-upstream-gone` | Also delete local branches whose upstream is gone |
| `st cascade` | Restack, push, and create/update PRs |
| `st init` | Initialize stax or reconfigure the repo trunk |
| `st standup` | Summarize recent engineering activity |
| `st pr` | Open the current branch PR in the browser |
| `st pr list` | Show open pull requests in the current repo |
| `st issue list` | Show open issues in the current repo |
| `st changelog` | Generate changelog between refs (auto-resolves last tag) |
| `st open` | Open repository in browser |
| `st undo` | Undo last risky operation |
| `st redo` | Re-apply undone operation |
| `st resolve` | Resolve in-progress rebase conflicts using AI |
| `st abort` | Abort in-progress rebase/conflict resolution |
| `st detach` | Remove branch from stack, reparent children |
| `st run <cmd>` (alias: `st test <cmd>`) | Run a command on each branch in the stack |
| `st demo` | Interactive tutorial (no auth/repo needed) |

For the complete CLI list and aliases, see [Full Reference](reference.md).
