# stax-new
Description: Create or reuse a STAX worktree lane and open Cursor inside it.

Run this command:
stax wt c "{{input}}" --run "cursor ."

This will:
- Create or reuse a stacked worktree lane in .worktrees/
- Start Cursor inside that lane
- Keep full STAX power (restack, undo, TUI, etc.)
