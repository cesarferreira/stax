# Install

```bash
# Homebrew (macOS/Linux)
brew install cesarferreira/tap/stax

# Or with cargo binstall
cargo binstall stax
```

Both `stax` and `st` (short alias) are installed automatically.

### Manual install from GitHub Releases

```bash
# macOS (Apple Silicon)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-aarch64-apple-darwin.tar.gz | tar xz
# macOS (Intel)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-apple-darwin.tar.gz | tar xz
# Linux (x86_64)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-unknown-linux-gnu.tar.gz | tar xz
# Linux (arm64 / aarch64, e.g. Raspberry Pi)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-aarch64-unknown-linux-gnu.tar.gz | tar xz

mkdir -p ~/.local/bin
mv stax st ~/.local/bin/
```

### Windows

Download `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases/latest), extract both `stax.exe` and `st.exe`, and place them in a directory on your `PATH`.

See [Windows notes](../reference/windows.md) for shell and worktree limitations.

## Verify

```bash
stax --version
# One-shot onboarding: shell integration + skills + auth import from gh when available
st setup --yes
# Later, upgrade via the same installation method stax was installed with
st cli upgrade
```
