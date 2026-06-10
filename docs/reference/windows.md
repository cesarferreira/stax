# Windows

stax supports Windows (`x86_64-pc-windows-msvc`), but prebuilt Windows binaries are not published. Windows users should build and install from source with Rust.

## Install

Install Rust with the MSVC toolchain, then install from crates.io:

```powershell
cargo install stax --locked --features vendored-openssl
```

Or build from a local checkout:

```powershell
git clone https://github.com/cesarferreira/stax.git
cd stax
cargo install --path . --locked --features vendored-openssl
```

Cargo installs `stax.exe` and `st.exe` into `%USERPROFILE%\.cargo\bin`; make sure that directory is on your `PATH`.

If you install with `cargo install`, update notifications and `st cli upgrade` detect the Windows `.cargo\bin` path and show the matching cargo upgrade command.

## What works

All core stax features work on Windows without modification:

- Stacked branches: `st create`, `st ls`, `st ll`, `st restack`
- PR workflows: `st ss`, `st merge`, `st cascade`, `st update`, `st pr`
- Sync and cleanup: `st rs`, `st sync`
- Undo/redo: `st undo`, `st redo`
- Interactive TUI: bare `st`
- AI generation: `st create --ai`, `st gen` / `st generate`, `st standup --ai`
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
