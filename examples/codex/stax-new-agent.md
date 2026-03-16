# stax-new
Description: Create or reuse a STAX worktree lane and open Codex inside it.

Run this command:
stax wt c "{{input}}" --agent codex

This will:
- Create or reuse a stacked worktree lane in .worktrees/
- Start Codex inside that lane
- Keep full STAX power (restack, undo, TUI, etc.)
