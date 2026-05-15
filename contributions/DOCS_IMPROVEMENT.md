```markdown
<details>
<summary><strong>Other installation methods</strong> — cargo-binstall, prebuilt binaries, Windows, from source</summary>

### cargo-binstall

```bash
cargo binstall stax
```

### Prebuilt binaries (macOS/Linux)

Download and extract the archive for your platform, then move both binaries (`stax` and `st`) into your PATH.

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
chmod +x ~/.local/bin/stax ~/.local/bin/st
```

If `~/.local/bin` is not on your `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc   # or ~/.zshrc
source ~/.bashrc                                           # or source ~/.zshrc
```

### Windows (PowerShell)

Download the latest `stax-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/cesarferreira/stax/releases), extract it, and add the folder to your `PATH`:

```powershell
$bin = "$HOME\bin"
New-Item -ItemType Directory -Force -Path $bin | Out-Null
Move-Item .\stax.exe $bin
Move-Item .\st.exe   $bin
[Environment]::SetEnvironmentVariable("Path", $env:Path + ";$bin", "User")
```

Restart your terminal after updating `PATH`.

### From source (Rust toolchain required)

```bash
cargo install stax
```

### Verify install

```bash
st --version
```

</details>
```