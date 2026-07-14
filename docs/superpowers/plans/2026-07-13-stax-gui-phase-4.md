# Stax GUI Phase 4 Release Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the existing native macOS GUI as two versioned, architecture-specific GitHub Release app archives with an unsigned baseline, optional Apple signing/notarization, final identity and icon, accessible keyboard operation, and headless packaged smoke checks.

**Architecture:** Keep `stax-gui` as a private workspace package and keep the CLI release archives unchanged. A fast assembly script builds `Stax.app` from an injected executable; a release wrapper compiles one target, validates the Mach-O architecture, optionally signs/notarizes, archives the app, extracts it, and runs a headless `--version` smoke check. The existing release workflow uploads both app archives beside the CLI artifacts.

**Tech Stack:** Rust 1.96.1, GPUI 0.2.2, Bash, macOS `plutil`/`PlistBuddy`/`iconutil`/`lipo`/`codesign`/`xcrun notarytool`/`ditto`, GitHub Actions, cargo-nextest.

## Global Constraints

- Work only on `cesar/gpui-gui-phase-4`, stacked directly on `cesar/gpui-gui-phase-3`.
- Final bundle identifier: `com.cesarferreira.stax`.
- Minimum system version: macOS 13.0.
- App artifacts: `Stax-aarch64-apple-darwin.zip` and `Stax-x86_64-apple-darwin.zip`; do not create a universal binary.
- Existing CLI artifacts and Homebrew formula behavior remain unchanged.
- GitHub Releases is the only new GUI distribution channel in this phase.
- With no Apple secrets, the release must succeed and publish an unsigned app.
- Partial signing or notarization credentials must fail rather than silently downgrade.
- The default compressed app ceiling is 83,886,080 bytes (80 MiB), configurable with `STAX_GUI_MAX_ARCHIVE_BYTES`.
- Every non-trivial code change needs happy-path, error-path, and edge-case tests.
- Full-suite validation uses `make test`; never run the full suite with native `cargo test`.
- Update `README.md`, relevant `docs/` pages, and `skills.md` for every changed user-visible behavior.

---

## File Map

- Create `crates/stax-gui/src/startup.rs` for display-independent GUI executable argument parsing.
- Modify `crates/stax-gui/src/main.rs` and `crates/stax-gui/src/lib.rs` to expose and use `--version` without starting GPUI.
- Modify `src/commands/gui.rs`, `src/cli/args.rs`, and `tests/gui_command_tests.rs` for the final bundle identity and release installation copy.
- Create `crates/stax-gui/resources/AppIcon-1024.png` as the approved editable icon source.
- Create `crates/stax-gui/resources/AppIcon.icns` as the bundle-ready icon.
- Create `scripts/build-gui-icon.sh` to reproducibly convert the 1024px source into a complete macOS iconset.
- Modify `crates/stax-gui/resources/Info.plist.in` and `scripts/build-gui-app.sh` for final metadata and resources.
- Expand `scripts/gui-app-tests.sh` with assembly metadata, missing-resource, invalid-metadata, and install tests.
- Create `scripts/package-gui-release.sh` for release build, architecture validation, optional signing/notarization, archive creation, and extracted smoke verification.
- Create `scripts/gui-release-tests.sh` for release environment validation and a native-architecture package smoke test.
- Create `scripts/gui-release-workflow-tests.sh` for the GitHub Actions artifact and secret-gating contract.
- Modify `Makefile` with `gui-icon`, `gui-release`, and `gui-release-test` targets.
- Modify `.github/workflows/release.yml` and `.github/workflows/rust-tests.yml` to validate and publish the app artifacts.
- Modify `crates/stax-gui/src/views/app.rs`, `workspace.rs`, `inspector_pane.rs`, and tests so all visible enabled buttons are keyboard controls.
- Modify `README.md`, `docs/getting-started/install.md`, `docs/interface/gui.md`, `docs/commands/core.md`, `docs/commands/reference.md`, `docs/workflows/releasing.md`, and `skills.md` for public distribution, Gatekeeper, size, and accessibility behavior.
- Add `docs/superpowers/specs/2026-07-13-stax-gui-phase-4-design.md` and this plan as the phase contract.

---

### Task 1: Lock the public executable and LaunchServices contract

**Files:**
- Create: `crates/stax-gui/src/startup.rs`
- Modify: `crates/stax-gui/src/main.rs`
- Modify: `crates/stax-gui/src/lib.rs`
- Modify: `src/commands/gui.rs`
- Modify: `src/cli/args.rs`
- Modify: `tests/gui_command_tests.rs`

**Interfaces:**
- Produces: `pub enum StartupCommand { Run(Option<PathBuf>), PrintVersion }`.
- Produces: `pub fn parse_startup_command(args: impl IntoIterator<Item = OsString>) -> Result<StartupCommand, String>`.
- Produces: `stax-gui --version` output from `format!("stax-gui {}", env!("CARGO_PKG_VERSION"))`.
- Produces: CLI LaunchServices bundle id `com.cesarferreira.stax`.

- [ ] **Step 1: Write failing startup and launcher tests**

Add unit cases in `startup.rs` for no argument, one repository path containing spaces, `--version`, and two positional paths. Update the launcher expectations in `src/commands/gui.rs` and `tests/gui_command_tests.rs`:

```rust
assert_eq!(
    parse_startup_command([]).unwrap(),
    StartupCommand::Run(None)
);
assert_eq!(
    parse_startup_command([OsString::from("--version")]).unwrap(),
    StartupCommand::PrintVersion
);
assert!(parse_startup_command([
    OsString::from("/tmp/one"),
    OsString::from("/tmp/two")
])
.unwrap_err()
.contains("one repository path"));
assert!(runner.args().contains(&OsString::from("com.cesarferreira.stax")));
```

Also change `gui_missing_app_result_is_actionable` to require both `GitHub Releases` and `make install-gui-app` in the recovery copy.

- [ ] **Step 2: Run the focused tests and confirm the old contract fails**

Run:

```bash
cargo nextest run -p stax-gui startup::tests::
cargo nextest run gui_command_tests:: commands::gui::tests::
```

Expected: the startup module is missing and existing launcher assertions still observe `dev.stax.Stax` and developer-preview copy.

- [ ] **Step 3: Implement display-independent startup parsing**

Implement `startup.rs` with this behavior:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupCommand {
    Run(Option<PathBuf>),
    PrintVersion,
}

pub fn parse_startup_command(
    args: impl IntoIterator<Item = OsString>,
) -> Result<StartupCommand, String> {
    let mut args = args.into_iter();
    let command = match args.next() {
        None => StartupCommand::Run(None),
        Some(value) if value == "--version" || value == "-V" => {
            StartupCommand::PrintVersion
        }
        Some(value) => StartupCommand::Run(Some(PathBuf::from(value))),
    };
    if args.next().is_some() {
        return Err("stax-gui accepts at most one repository path".into());
    }
    Ok(command)
}
```

`main.rs` must print the parser error and exit with code 2, print the package version and exit 0 for `PrintVersion`, and call `stax_gui::run` only for `Run`.

- [ ] **Step 4: Finalize CLI identity and copy**

Set `BUNDLE_ID` to `com.cesarferreira.stax`, rename the Clap description to `Launch the native Stax macOS app`, and use this failure message:

```rust
format!(
    "failed to launch Stax.app with {}; install it from GitHub Releases or run `make install-gui-app` for a local contributor build",
    program.display()
)
```

Keep `open -n -b <bundle-id> --args <canonical-path>` unchanged.

- [ ] **Step 5: Verify the focused contract**

Run:

```bash
cargo nextest run -p stax-gui startup::tests::
cargo nextest run gui_command_tests:: commands::gui::tests::
```

Expected: startup parsing covers happy, error, and edge cases; all GUI launcher tests pass with the final identity.

- [ ] **Step 6: Commit the identity contract**

```bash
git add crates/stax-gui/src/startup.rs crates/stax-gui/src/main.rs crates/stax-gui/src/lib.rs src/commands/gui.rs src/cli/args.rs tests/gui_command_tests.rs
git commit -m "feat(gui): finalize app identity and startup contract"
```

---

### Task 2: Create and validate the Strata S icon

**Files:**
- Create: `crates/stax-gui/resources/AppIcon-1024.png`
- Create: `crates/stax-gui/resources/AppIcon.icns`
- Create: `scripts/build-gui-icon.sh`
- Modify: `scripts/gui-app-tests.sh`
- Modify: `Makefile`

**Interfaces:**
- Consumes: approved Strata “S” visual direction from the Phase 4 design.
- Produces: `scripts/build-gui-icon.sh [source.png] [output.icns]`.
- Produces: a 1024×1024 PNG source and a valid multi-resolution `.icns` resource.

- [ ] **Step 1: Add a failing icon resource test**

Extend `scripts/gui-app-tests.sh` before changing assembly:

```bash
icon="$repo_root/crates/stax-gui/resources/AppIcon.icns"
source_icon="$repo_root/crates/stax-gui/resources/AppIcon-1024.png"
test "$(sips -g pixelWidth "$source_icon" | awk '/pixelWidth/{print $2}')" = "1024"
test "$(sips -g pixelHeight "$source_icon" | awk '/pixelHeight/{print $2}')" = "1024"
iconutil --convert iconset --output "$fixture/roundtrip.iconset" "$icon"
test -f "$fixture/roundtrip.iconset/icon_16x16.png"
test -f "$fixture/roundtrip.iconset/icon_512x512@2x.png"
```

- [ ] **Step 2: Run the app fixture test and confirm resources are absent**

Run: `make gui-app-test`

Expected: FAIL because neither icon resource exists.

- [ ] **Step 3: Generate the approved source image**

Use the `imagegen` skill and image generation tool with this exact art direction:

```text
Create a production macOS application icon at 1024x1024. A graphite rounded-square tile with generous optical padding. Four crisp layered geological strata bend into a bold abstract capital S, readable at 16px, with one restrained warm coral-orange accent layer. Premium native developer-tool aesthetic, subtle material depth, no text, no letters drawn literally, no gradients that muddy small sizes, no mockup, no surrounding scene, centered, symmetrical optical weight.
```

Inspect the result at full resolution, keep one approved result as
`crates/stax-gui/resources/AppIcon-1024.png`, and do not generate alternate
brand directions after the approved composition is readable at 16px.

- [ ] **Step 4: Implement reproducible icon conversion**

`build-gui-icon.sh` must reject non-macOS hosts, missing files, and non-1024px sources. It creates all required representations:

```bash
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$source" --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$source" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil --convert icns --output "$output" "$iconset"
```

Use a temporary iconset directory and clean it with a trap. Add `gui-icon` to `.PHONY` and make it call the script with the committed source/output paths.

- [ ] **Step 5: Build and round-trip the icon**

Run:

```bash
make gui-icon
make gui-app-test
```

Expected: conversion succeeds and the round-tripped iconset contains every 1× and 2× representation.

- [ ] **Step 6: Commit icon assets and tooling**

```bash
git add crates/stax-gui/resources/AppIcon-1024.png crates/stax-gui/resources/AppIcon.icns scripts/build-gui-icon.sh scripts/gui-app-tests.sh Makefile
git commit -m "feat(gui): add the Strata app icon"
```

---

### Task 3: Assemble a versioned final app bundle

**Files:**
- Modify: `crates/stax-gui/resources/Info.plist.in`
- Modify: `scripts/build-gui-app.sh`
- Modify: `scripts/gui-app-tests.sh`
- Modify: `Makefile`

**Interfaces:**
- Consumes: `AppIcon.icns` from Task 2.
- Consumes environment: `STAX_GUI_BINARY`, `STAX_GUI_OUTPUT`, `STAX_GUI_VERSION`, `STAX_GUI_BUILD_NUMBER`, and `STAX_GUI_LSREGISTER`.
- Produces: final `Stax.app` with executable, plist, and icon.

- [ ] **Step 1: Write failing final-bundle fixture assertions**

Update the fixture invocation to pass version `1.2.3` and build `456`, then assert:

```bash
test "$($buddy -c 'Print :CFBundleIdentifier' "$plist")" = "com.cesarferreira.stax"
test "$($buddy -c 'Print :CFBundleDisplayName' "$plist")" = "Stax"
test "$($buddy -c 'Print :CFBundleShortVersionString' "$plist")" = "1.2.3"
test "$($buddy -c 'Print :CFBundleVersion' "$plist")" = "456"
test "$($buddy -c 'Print :CFBundleIconFile' "$plist")" = "AppIcon"
cmp "$repo_root/crates/stax-gui/resources/AppIcon.icns" \
  "$app/Contents/Resources/AppIcon.icns"
```

Add bad paths for `STAX_GUI_VERSION='1.2.<bad>'`, `STAX_GUI_BUILD_NUMBER='abc'`, and a missing `STAX_GUI_ICON`; each must fail without leaving a partially assembled output.

- [ ] **Step 2: Run the fixture suite and confirm provisional metadata fails**

Run: `make gui-app-test`

Expected: FAIL on the old bundle id, developer-preview display name, absent version keys, or absent icon.

- [ ] **Step 3: Extend the plist contract**

Add these keys to `Info.plist.in`:

```xml
<key>CFBundleShortVersionString</key><string>@VERSION@</string>
<key>CFBundleVersion</key><string>@BUILD_NUMBER@</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
```

Set `CFBundleDisplayName` to `Stax` and retain `LSMinimumSystemVersion` at `13.0`.

- [ ] **Step 4: Make assembly atomic and metadata-driven**

In `build-gui-app.sh`:

- default the version to `cargo pkgid -p stax-gui`’s semantic version;
- default the build number to the numeric `major * 1_000_000 + minor * 1_000 + patch` value;
- accept prerelease semantic versions but reject XML metacharacters;
- require a numeric build number;
- assemble into `$(dirname "$output")/.Stax.app.tmp.$$`;
- copy the executable, `AppIcon.icns`, and substituted plist;
- validate the plist before replacing the old output;
- remove the temporary bundle on every failure.

Use `bundle_id="com.cesarferreira.stax"` and keep `--install` as the only positional option.

- [ ] **Step 5: Verify assembly and local installation**

Run:

```bash
make gui-app-test
make gui-app
```

Expected: fixture happy/error/edge cases pass; `target/gui/Stax.app` has final metadata and a valid icon while remaining unsigned.

- [ ] **Step 6: Commit final bundle assembly**

```bash
git add crates/stax-gui/resources/Info.plist.in scripts/build-gui-app.sh scripts/gui-app-tests.sh Makefile
git commit -m "feat(gui): assemble versioned app bundles"
```

---

### Task 4: Package, sign, notarize, and smoke-test one architecture

**Files:**
- Create: `scripts/package-gui-release.sh`
- Create: `scripts/gui-release-tests.sh`
- Modify: `Makefile`

**Interfaces:**
- Consumes: `scripts/build-gui-app.sh` and `stax-gui --version`.
- Consumes optional signing environment: `STAX_GUI_SIGNING_IDENTITY`.
- Consumes optional notarization environment: `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`.
- Produces: `Stax-<target>.zip` containing exactly one `Stax.app`.
- Produces: `scripts/package-gui-release.sh --validate-environment` for fast secret-contract tests.

- [ ] **Step 1: Write failing release environment tests**

Create `gui-release-tests.sh` with a table that runs `--validate-environment` for:

```text
no signing or notarization variables                     -> success, unsigned
signing identity only                                    -> success, signed
all three notarization variables plus signing identity   -> success, notarized
one or two notarization variables                        -> failure
notarization variables without signing identity          -> failure
unsupported target                                       -> failure
```

The script must also run a native target package smoke using the actual release binary, selecting `aarch64-apple-darwin` for `arm64` and `x86_64-apple-darwin` for `x86_64`.

- [ ] **Step 2: Run release tests and confirm the packager is missing**

Run: `bash scripts/gui-release-tests.sh`

Expected: FAIL because `package-gui-release.sh` does not exist.

- [ ] **Step 3: Implement target and credential validation**

Support only these mappings:

```bash
case "$target" in
  aarch64-apple-darwin) expected_arch=arm64 ;;
  x86_64-apple-darwin) expected_arch=x86_64 ;;
  *) fail "unsupported GUI release target: $target" ;;
esac
```

Treat `APPLE_ID`, `APPLE_TEAM_ID`, and `APPLE_APP_PASSWORD` as all-or-none.
Reject notarization unless `STAX_GUI_SIGNING_IDENTITY` is set. Print exactly
`unsigned`, `signed`, or `notarized` from `--validate-environment`.

- [ ] **Step 4: Implement release build and archive creation**

For normal execution accept `--target`, `--version`, `--build-number`, and
optional `--output-dir`. Run:

```bash
cargo build -p stax-gui --release --locked --target "$target"
STAX_GUI_BINARY="$repo_root/target/$target/release/stax-gui" \
STAX_GUI_OUTPUT="$stage/Stax.app" \
STAX_GUI_VERSION="$version" \
STAX_GUI_BUILD_NUMBER="$build_number" \
  "$repo_root/scripts/build-gui-app.sh"
```

Assert `lipo -archs` is exactly the mapped architecture. When an identity is
present, run:

```bash
codesign --force --deep --options runtime --timestamp \
  --sign "$STAX_GUI_SIGNING_IDENTITY" "$stage/Stax.app"
codesign --verify --deep --strict --verbose=2 "$stage/Stax.app"
```

Create the zip with `ditto -c -k --sequesterRsrc --keepParent`. If notarization
credentials are present, submit with `xcrun notarytool submit --wait`, staple
the accepted ticket, verify with `stapler validate`, and recreate the zip.

- [ ] **Step 5: Implement extracted smoke and size checks**

Extract the final zip into a new temporary directory and assert:

```bash
test "$($buddy -c 'Print :CFBundleIdentifier' "$plist")" = "com.cesarferreira.stax"
test "$($buddy -c 'Print :CFBundleShortVersionString' "$plist")" = "$version"
test "$(lipo -archs "$executable")" = "$expected_arch"
test "$($executable --version)" = "stax-gui $version"
test "$(find "$extracted" -maxdepth 1 -name '*.app' | wc -l | tr -d ' ')" = "1"
```

Reject archives larger than `${STAX_GUI_MAX_ARCHIVE_BYTES:-83886080}` and print
both executable and archive byte counts in the success summary.

- [ ] **Step 6: Verify native packaging and the unsigned baseline**

Run:

```bash
bash scripts/gui-release-tests.sh
make gui-release-test
```

Expected: all credential cases pass, a native release archive is created and extracted, its version command exits without a display, and its compressed size is under 80 MiB.

- [ ] **Step 7: Commit the release packager**

```bash
git add scripts/package-gui-release.sh scripts/gui-release-tests.sh Makefile
git commit -m "feat(gui): package architecture-specific releases"
```

---

### Task 5: Publish both app archives from GitHub Actions

**Files:**
- Create: `scripts/gui-release-workflow-tests.sh`
- Modify: `.github/workflows/release.yml`
- Modify: `.github/workflows/rust-tests.yml`

**Interfaces:**
- Consumes: `scripts/package-gui-release.sh`.
- Consumes optional secrets: `MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`, `MACOS_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`.
- Produces: two GUI artifacts downloaded into the existing release job.

- [ ] **Step 1: Write a failing workflow contract test**

Create `gui-release-workflow-tests.sh` that requires all of these literal
contracts in `.github/workflows/release.yml`:

```text
aarch64-apple-darwin
x86_64-apple-darwin
Stax-aarch64-apple-darwin.zip
Stax-x86_64-apple-darwin.zip
scripts/package-gui-release.sh
MACOS_CERTIFICATE_P12
security create-keychain
codesign
APPLE_APP_PASSWORD
```

It must also assert that the existing five CLI artifact names remain present
and that `release.needs` includes both `build` and `gui-build`.

- [ ] **Step 2: Run the contract and confirm GUI release jobs are absent**

Run: `bash scripts/gui-release-workflow-tests.sh`

Expected: FAIL because the release workflow currently knows only the five CLI archives.

- [ ] **Step 3: Add the GUI release matrix**

Add a `gui-build` job on `macos-15` with the two Apple targets. Install Rust
1.96.1 and the matrix target, restore a target-specific cache, and derive
`VERSION` from the `stax-gui` package returned by `cargo pkgid -p stax-gui`.
For a `v*` tag, strip the `v` and fail if the tag version differs from that
package version. For `workflow_dispatch`, retain the package version. Set
`BUILD_NUMBER=${GITHUB_RUN_NUMBER}`.

The certificate setup step must implement all-or-none handling for
`MACOS_CERTIFICATE_P12`, `MACOS_CERTIFICATE_PASSWORD`, and
`MACOS_SIGNING_IDENTITY`. With all empty, write an empty
`STAX_GUI_SIGNING_IDENTITY` to `$GITHUB_ENV`. With all present, create a
temporary keychain, import the decoded p12, configure key partition access,
and export the identity. Any partial set exits 1.

- [ ] **Step 4: Build, upload, and safely clean signing state**

Invoke the packager with the matrix target and pass notarization secrets only
as environment variables. Upload exactly `Stax-${{ matrix.target }}.zip`.
Add an `if: always()` keychain cleanup step that deletes only the temporary
keychain created by this job.

- [ ] **Step 5: Extend the release gate without changing CLI/Homebrew behavior**

Set `release.needs: [build, gui-build]`, add the two app zips to
`expected_artifacts`, and leave CLI checksum and Homebrew formula steps scoped
to the existing CLI filenames.

- [ ] **Step 6: Add release packaging to macOS quality CI**

In `.github/workflows/rust-tests.yml`, run `make gui-release-test` after the
developer bundle test. This verifies the unsigned, native-architecture release
path on pull requests without accessing release secrets.

- [ ] **Step 7: Verify workflow and local release contracts**

Run:

```bash
bash scripts/gui-release-workflow-tests.sh
make gui-release-test
git diff --check
```

Expected: the workflow contract passes, local unsigned packaging passes, and no existing CLI artifact expectation changed.

- [ ] **Step 8: Commit release automation**

```bash
git add .github/workflows/release.yml .github/workflows/rust-tests.yml scripts/gui-release-workflow-tests.sh
git commit -m "ci(gui): publish macOS app artifacts"
```

---

### Task 6: Make every visible action keyboard-operable

**Files:**
- Modify: `crates/stax-gui/src/views/app.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Modify: `crates/stax-gui/src/views/inspector_pane.rs`
- Modify: `crates/stax-gui/src/views/operation_tests.rs`
- Modify: `crates/stax-gui/src/views/tests.rs`

**Interfaces:**
- Consumes: existing `control_button` and `activate_control` keyboard click synthesis.
- Produces: no `mouse_control_button`; all enabled visible buttons are tab stops with the existing focus ring.

- [ ] **Step 1: Write failing toolbar and inspector keyboard tests**

Add GPUI tests that focus each enabled debug selector through `window.focus_next()`
and activate with Enter or Space. Cover:

```text
toolbar-create-branch
toolbar-submit-stack
inspector-checkout
inspector-restack
inspector-rename
inspector-delete
inspector-move
inspector-reorder
inspector-open-pr
operation-receipt-undo
operation-receipt-redo
operation-banner-dismiss
```

Each test asserts exactly one overlay, operation request, browser URL, or
presentation transition. Add an edge case proving a disabled inspector control
is skipped by `focus_next` and still renders its textual reason.

- [ ] **Step 2: Run the focused tests and confirm mouse-only controls fail**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests::keyboard_ views::tests::keyboard_
```

Expected: the new toolbar/inspector cases fail because `mouse_control_button` does not register a tab stop.

- [ ] **Step 3: Remove the mouse-only button path**

Delete `mouse_control_button` from `app.rs` and its exports/imports. Replace all
toolbar and inspector uses with `control_button`. Keep the existing 28px desktop
target height, visible label, disabled reason, contrast-tested focus border,
and click handlers unchanged.

- [ ] **Step 4: Verify keyboard, pointer, and operation behavior together**

Run:

```bash
cargo nextest run -p stax-gui views::operation_tests:: views::tests::keyboard_ theme::tests::keyboard_focus_
```

Expected: new keyboard tests pass; existing click and shortcut tests still dispatch each action exactly once; focus contrast remains passing.

- [ ] **Step 5: Commit accessibility hardening**

```bash
git add crates/stax-gui/src/views/app.rs crates/stax-gui/src/views/workspace.rs crates/stax-gui/src/views/inspector_pane.rs crates/stax-gui/src/views/operation_tests.rs crates/stax-gui/src/views/tests.rs
git commit -m "feat(gui): make visible actions keyboard-operable"
```

---

### Task 7: Document installation, Gatekeeper, size, and release operations

**Files:**
- Modify: `README.md`
- Modify: `docs/getting-started/install.md`
- Modify: `docs/interface/gui.md`
- Modify: `docs/commands/core.md`
- Modify: `docs/commands/reference.md`
- Modify: `docs/workflows/releasing.md`
- Modify: `skills.md`

**Interfaces:**
- Consumes: final artifact names, bundle id, and credential contract from Tasks 1–5.
- Produces: one consistent public install and maintainer release story.

- [ ] **Step 1: Replace developer-preview public copy**

Document these commands for Apple Silicon and Intel:

```bash
curl -fLO https://github.com/cesarferreira/stax/releases/latest/download/Stax-aarch64-apple-darwin.zip
ditto -x -k Stax-aarch64-apple-darwin.zip .
mv Stax.app /Applications/
```

Use the `x86_64` filename for Intel. State that the app zip is a separate
artifact on the same release, not a new crates.io package, and that installing
it does not enlarge the `stax` or `st` CLI binaries.

- [ ] **Step 2: Add safe unsigned Gatekeeper guidance**

Explain the baseline flow precisely:

1. Download only from the project GitHub Releases page.
2. Move `Stax.app` to `/Applications`.
3. Control-click `Stax.app`, choose Open, then choose Open again.
4. If macOS blocks the first launch, open System Settings → Privacy & Security and choose Open Anyway for Stax.

Do not recommend globally disabling Gatekeeper. Explain that signed/notarized
builds open normally when the optional release secrets are configured.

- [ ] **Step 3: Document maintainer signing and artifact checks**

In `docs/workflows/releasing.md`, list the six optional Apple secrets, the
all-or-none certificate/notarization rules, both app filenames, the unsigned
fallback, and verification commands:

```bash
codesign -dv --verbose=4 Stax.app
codesign --verify --deep --strict --verbose=2 Stax.app
spctl --assess --type execute --verbose=4 Stax.app
```

Keep the existing CLI and Homebrew release instructions intact.

- [ ] **Step 4: Document accessibility scope honestly**

Update the GUI guide to say all visible actions are keyboard-operable with
visible focus and textual labels. Also state that GPUI 0.2.2 does not yet expose
the stable accessibility-node integration needed to claim complete VoiceOver
support.

- [ ] **Step 5: Run documentation consistency checks**

Run:

```bash
rg -n "com\.cesarferreira\.stax|Stax-aarch64|Stax-x86_64|Gatekeeper|Open Anyway|VoiceOver|80 MiB|new package|CLI" README.md docs/getting-started/install.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md docs/workflows/releasing.md skills.md
! rg -n "Phase 3 developer preview|dev\.stax\.Stax|packaged distribution remains Phase 4" README.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md skills.md
git diff --check
```

Expected: all public surfaces use final identity/artifact copy and stale preview language is absent.

- [ ] **Step 6: Commit public release documentation**

```bash
git add README.md docs/getting-started/install.md docs/interface/gui.md docs/commands/core.md docs/commands/reference.md docs/workflows/releasing.md skills.md
git commit -m "docs(gui): document macOS app releases"
```

---

### Task 8: Run the phase-wide release gate and inspect the stack

**Files:**
- Modify only files required by verification failures.

**Interfaces:**
- Consumes: all Phase 4 tasks.
- Produces: a clean, fully verified `cesar/gpui-gui-phase-4` branch.

- [ ] **Step 1: Run focused app and packaging checks**

Run:

```bash
make gui-icon
make gui-app-test
make gui-app
make gui-release-test
bash scripts/gui-release-workflow-tests.sh
cargo nextest run gui_command_tests::
cargo nextest run -p stax-gui --locked
cargo check -p stax-gui --locked
```

Expected: icon, developer bundle, native release archive, launcher, and all GUI tests pass.

- [ ] **Step 2: Run strict GUI Clippy**

Run:

```bash
cargo clippy -p stax-gui --all-targets --locked -- \
  -D warnings \
  -A clippy::assertions_on_constants \
  -A clippy::bool_assert_comparison \
  -A clippy::clone_on_copy \
  -A clippy::collapsible_if \
  -A clippy::collapsible_match \
  -A clippy::double_comparisons \
  -A clippy::if_same_then_else \
  -A clippy::items_after_test_module \
  -A clippy::len_zero \
  -A clippy::let_and_return \
  -A clippy::manual_checked_ops \
  -A clippy::needless_borrow \
  -A clippy::needless_lifetimes \
  -A clippy::too_many_arguments \
  -A clippy::to_string_in_format_args \
  -A clippy::type_complexity \
  -A clippy::unnecessary_map_or \
  -A clippy::unnecessary_sort_by \
  -A clippy::useless_format \
  -A clippy::useless_vec
```

Expected: no unallowed warnings.

- [ ] **Step 3: Run repository lint and the required full suite**

Run:

```bash
make lint
make test
```

Expected: lint passes and the Docker-backed full suite passes with zero failures.

- [ ] **Step 4: Inspect artifacts and prove CLI separation**

Run:

```bash
unzip -l target/gui-release/Stax-$(rustc -vV | sed -n 's/^host: //p').zip
tar tzf stax-$(rustc -vV | sed -n 's/^host: //p').tar.gz 2>/dev/null || true
find target/gui-release -maxdepth 1 -type f -name 'Stax-*.zip' -exec ls -lh {} \;
git diff cesar/gpui-gui-phase-3...HEAD --stat
git status --short
stax status
```

Expected: the app zip contains one `Stax.app`; no CLI packaging path contains
the GUI executable; the branch is clean and directly stacked on Phase 3.

- [ ] **Step 5: Commit verification-only fixes if present**

If verification required tracked changes, commit only those files:

```bash
git add -u
git commit -m "fix(gui): close release verification gaps"
```

If no tracked changes were required, do not create an empty commit.

---

## Phase Boundary

Stop after Phase 4 verification. Do not push, create pull requests, merge the
stack, configure repository secrets, or publish a release without explicit
user authorization. Keep Phase 3 and Phase 4 as separate stax branches so each
can be reviewed and submitted independently.
