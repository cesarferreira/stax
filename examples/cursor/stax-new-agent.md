# stax-new
Description: Create a new parallel agent worktree with STAX and open it in a fresh Cursor window.

Run this command:
stax agent create "{{input}}" --open-cursor

This will:
- Create a new stacked branch + isolated worktree in .stax/trees/
- Open it automatically in a new Cursor window
- Keep full STAX power (restack, undo, TUI, etc.)
