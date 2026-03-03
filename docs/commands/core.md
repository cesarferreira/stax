# Core Commands

| Command | What it does |
|---|---|
| `stax` | Launch interactive TUI |
| `stax ls` | Show stack with PR and rebase status |
| `stax ll` | Show stack with PR URLs and full details |
| `stax create <name>` | Create branch stacked on current |
| `stax ss` | Submit stack and create/update PRs |
| `stax merge` | Merge PRs from stack bottom to current |
| `stax merge --when-ready` | Merge in explicit wait-for-ready mode (legacy alias: `stax merge-when-ready`) |
| `stax rs` | Pull trunk and clean merged branches |
| `stax rs --restack` | Sync and rebase full stack |
| `stax rs --delete-upstream-gone` | Also delete local branches whose upstream is gone |
| `stax cascade` | Restack, push, and create/update PRs |
| `stax standup` | Summarize recent engineering activity |
| `stax changelog` | Generate changelog between refs |
| `stax open` | Open repository in browser |
| `stax undo` | Undo last risky operation |
| `stax redo` | Re-apply undone operation |
| `stax resolve` | Resolve in-progress rebase conflicts using AI |
| `stax abort` | Abort in-progress rebase/conflict resolution |
| `stax detach` | Remove branch from stack, reparent children |
| `stax test <cmd>` | Run a command on each branch in the stack |
| `stax demo` | Interactive tutorial (no auth/repo needed) |

For the complete CLI list and aliases, see [Full Reference](reference.md).
