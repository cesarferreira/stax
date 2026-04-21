# Install

The shortest path on macOS and Linux:

```bash
brew install cesarferreira/tap/stax
```

Or with [`cargo binstall`](https://github.com/cargo-bins/cargo-binstall):

```bash
cargo binstall stax
```

Both `stax` and the short alias `st` are installed.

## Prebuilt binaries

From [GitHub Releases](https://github.com/cesarferreira/stax/releases/latest):

```bash
# macOS (Apple Silicon)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-aarch64-apple-darwin.tar.gz | tar xz
# macOS (Intel)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-apple-darwin.tar.gz | tar xz
# Linux (x86_64)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-x86_64-unknown-linux-gnu.tar.gz | tar xz
# Linux (arm64)
curl -fsSL https://github.com/cesarferreira/stax/releases/latest/download/stax-aarch64-unknown-linux-gnu.tar.gz | tar xz

mkdir -p ~/.local/bin
mv stax st ~/.local/bin/
```

Ensure `~/.local/bin` is on your `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zshrc   # or ~/.bashrc
```

## Windows

Download `stax-x86_64-pc-windows-msvc.zip` from [Releases](https://github.com/cesarferreira/stax/releases/latest), extract `stax.exe` and `st.exe`, and place them on your `PATH`. See [Windows notes](../reference/windows.md) for shell and tmux limitations.

## Build from source

Requires Rust and system OpenSSL (Debian/Ubuntu: `libssl-dev pkg-config` · Fedora/RHEL: `openssl-devel` · Arch: `openssl pkg-config`).

```bash
cargo install --path . --locked
```

Without system OpenSSL:

```bash
cargo install --path . --locked --features vendored-openssl
```

## Verify and onboard

```bash
stax --version
st setup --yes       # shell integration, AI skills, and auth from gh (if available)
st cli upgrade       # later, upgrade via the install method you used
```

Next: [Quick start →](quick-start.md)
