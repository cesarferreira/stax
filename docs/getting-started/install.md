# Install

Fastest path:

```bash
# Homebrew (macOS/Linux)
brew install cesarferreira/tap/stax

# Or with cargo binstall
cargo binstall stax

# Verify
st --version
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

## Build from source

If you want to install from the repo:

Prerequisites:

- Debian/Ubuntu: `sudo apt-get install libssl-dev pkg-config`
- Fedora/RHEL: `sudo dnf install openssl-devel`
- Arch Linux: `sudo pacman -S openssl pkg-config`
- macOS: OpenSSL is available by default

Then build with:

```bash
# Using cargo
cargo install --path . --locked

# Or using make
make install
```

If you want to avoid system OpenSSL dependencies, use the vendored feature instead. The first build is slower, but it is self-contained:

```bash
cargo install --path . --locked --features vendored-openssl
```
