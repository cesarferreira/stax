# Windows

stax ships prebuilt Windows binaries (`x86_64-pc-windows-msvc`). Unit tests run on Windows in CI alongside Linux.

## Install

Download `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases/latest), extract `stax.exe` and `st.exe`, and place them on your `PATH`.

## What works

All core stax features work on Windows without modification:

- Stacked branches: `st create`, `st ls`, `st ll`, `st restack`
- PR workflows: `st ss`, `st merge`, `st cascade`, `st refresh`, `st pr`
- Sync and cleanup: `st rs`, `st sync`
- Undo/redo: `st undo`, `st redo`
- Interactive TUI: bare `st`
- AI generation: `st generate --pr-body`, `st standup --summary`
- Worktree commands: `st wt c/go/ls/ll/cleanup/rm <name>/prune/restack`
- Browser opening: `st pr`, `st open` (uses `cmd /c start`)
- Auth: `st auth`, `st auth --from-gh`, `STAX_GITHUB_TOKEN`

## Shell integration limitations

`st setup` generates shell functions for **bash, zsh, and fish** only — no PowerShell or CMD equivalent:

| Feature | Unix (bash/zsh/fish) | Windows (PowerShell/CMD) |
|---|---|---|
| `st wt c` / `st wt go` auto-`cd` | works | shell stays in place — manually `cd` to the printed path |
| `sw <name>` alias | works | unavailable |
| `st wt rm` (no argument) | relocates then removes | pass an explicit name: `st wt rm <name>` |
| `STAX_SHELL_INTEGRATION` env var | set by shell function | not set |

Worktree commands themselves still work — only the parent-shell `cd` is missing.

## tmux

The `--tmux` flag and the worktree dashboard's tmux session management assume a Unix `tmux` binary. Not available on native Windows. Works normally inside WSL.

## Config path

stax uses `dirs::home_dir()` joined with `.config/stax`. On Windows that's typically:

```text
C:\Users\<you>\.config\stax\config.toml
```

Override with the `STAX_CONFIG_DIR` environment variable. Credentials and shell integration files live under the same parent directory.
