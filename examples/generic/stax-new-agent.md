# stax-new
Description: Create a new parallel agent worktree with STAX and open it in your default editor.

Run this command:
stax agent create "{{input}}" --open

This will:
- Create a new stacked branch + isolated worktree in .stax/trees/
- Open it automatically in your configured editor (auto-detects cursor, then code)
- Keep full STAX power (restack, undo, TUI, etc.)

To configure your preferred editor:

  [agent]
  default_editor = "cursor"  # or "codex" / "code"

in ~/.config/stax/config.toml
