# stax-new
Description: Create or reuse a STAX worktree lane and run any launcher inside it.

Run this command:
stax wt c "{{input}}" --run "cursor ."

This will:
- Create or reuse a stacked worktree lane in .worktrees/
- Run your chosen launcher inside that lane
- Keep full STAX power (restack, undo, TUI, etc.)
