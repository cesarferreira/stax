# Install

```bash
# Homebrew (macOS/Linux)
brew install cesarferreira/tap/stax

# Or with cargo binstall
cargo binstall stax
```

Both `stax` and `st` (short alias) are installed automatically.

### Windows

Download `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases/latest), extract `stax.exe`, and place it in a directory on your `PATH`.

To create the `st` short alias, copy or symlink the binary:

```powershell
Copy-Item stax.exe st.exe
```

See [Windows notes](../reference/windows.md) for shell and worktree limitations.

## Verify

```bash
stax --version
```
