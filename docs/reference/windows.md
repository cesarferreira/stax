# Windows

stax ships pre-built Windows binaries (`x86_64-pc-windows-msvc`) starting from the release that includes this page. Unit tests run on Windows in CI alongside Linux.

## Install

Download `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases/latest), extract both `stax.exe` and `st.exe`, and place them in a directory on your `PATH`.

## What works

All core stax features work on Windows without modification:

- Stacked branches: `st create`, `st ls`, `st ll`, `st restack`
- PR workflows: `st ss`, `st merge`, `st cascade`, `st refresh`, `st pr`
- Sync and cleanup: `st rs`, `st sync`
- Undo/redo safety: `st undo`, `st redo`
- Interactive TUI: `st` (no arguments)
- AI generation: `st generate --pr-body`, `st standup --summary`
- Worktree management: `st wt c`, `st wt go`, `st wt ls`, `st wt ll`, `st wt cleanup`, `st wt rm <name>`, `st wt prune`, `st wt restack`
- Browser opening: `st pr`, `st open` (uses `cmd /c start`)
- Auth: `st auth`, `st auth --from-gh`, `STAX_GITHUB_TOKEN` env var

## Shell integration limitations

`st setup` generates shell functions for **bash, zsh, and fish** only. There is no PowerShell or CMD equivalent. This means:

| Feature | Unix (bash/zsh/fish) | Windows (PowerShell/CMD) |
|---|---|---|
| `st wt c` / `st wt go` auto-`cd` | works | worktree created/found, but shell stays in current directory — manually `cd` to the printed path |
| `sw <name>` quick alias | works | not available |
| `st wt rm` (no argument, remove current worktree) | relocates shell, then removes | use `st wt rm <name>` with an explicit name instead |
| `STAX_SHELL_INTEGRATION` env var | set by shell function | not set |

Worktree commands themselves (`create`, `go`, `ls`, `ll`, `cleanup`, `rm <name>`, `prune`, `restack`) work identically — only the parent-shell directory change is missing.

## tmux

The `--tmux` flag and the worktree dashboard's tmux session management assume a Unix `tmux` binary. On native Windows these are unavailable. If you use WSL, tmux works normally inside the WSL environment.

## Config path

stax uses `dirs::home_dir()` joined with `.config/stax`. On Windows the config directory is typically:

```text
C:\Users\<you>\.config\stax\config.toml
```

Override with the `STAX_CONFIG_DIR` environment variable if needed. Credentials and shell integration files live under the same parent directory.
