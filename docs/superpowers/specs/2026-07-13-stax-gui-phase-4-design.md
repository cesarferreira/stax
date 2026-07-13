# Stax GUI Phase 4 Release Design

## Status

Approved for implementation on `cesar/gpui-gui-phase-4`, stacked on
`cesar/gpui-gui-phase-3`.

## Goal

Turn the native macOS GUI from a contributor-only developer preview into a
versioned GitHub Release artifact without changing the existing CLI package or
forcing maintainers to configure Apple signing credentials.

## Product decisions

- The GUI remains the existing private workspace package, `stax-gui`; it is
  not published to crates.io as a new end-user package.
- The CLI archives remain `stax-<target>.tar.gz` or `.zip` and continue to
  contain only `stax` and `st`.
- The GUI ships beside them on the same GitHub Release as two independent
  archives: `Stax-aarch64-apple-darwin.zip` and
  `Stax-x86_64-apple-darwin.zip`.
- There is no universal binary. Each app archive contains one native
  architecture so downloads stay smaller and release failures are isolated.
- GitHub Releases is the first GUI distribution channel. The existing
  Homebrew formula remains CLI-only in this phase.
- The final bundle identifier is `com.cesarferreira.stax`.
- The minimum supported system remains macOS 13.0.
- The app icon is the approved Strata “S” direction: a restrained graphite
  macOS tile with layered strata forming an `S` and one warm accent.

## Unsigned and signed distribution

Unsigned artifacts are the baseline and must always be buildable with no
secrets. macOS users may open an unsigned build by Control-clicking the app and
choosing Open, or by approving it under System Settings → Privacy & Security
after the first blocked launch. Documentation must explain that the warning is
Gatekeeper behavior and that users should download only from the project’s
GitHub Releases page.

Signing and notarization are optional release enhancements:

1. With no Apple secrets, CI produces the unsigned architecture-specific zip.
2. With a complete signing certificate configuration, CI signs the nested app
   bundle using the hardened runtime and a trusted timestamp.
3. With complete notarization credentials as well, CI submits the signed zip,
   waits for acceptance, staples the ticket to `Stax.app`, and recreates the
   final zip.
4. A partial credential set is an error. CI must never silently downgrade a
   partially configured signed release to unsigned.

The release artifact name does not change when signing is enabled. Consumers
can inspect the bundle with `codesign -dv --verbose=4 Stax.app` and
`spctl --assess --type execute Stax.app`.

## Bundle contract

Every packaged app contains:

```text
Stax.app/
└── Contents/
    ├── Info.plist
    ├── MacOS/Stax
    └── Resources/AppIcon.icns
```

`Info.plist` carries the final identifier, `Stax` display name, semantic
`CFBundleShortVersionString`, numeric `CFBundleVersion`, `AppIcon`, minimum
macOS version, high-resolution support, and the application package type.
The release tag supplies the semantic version; the GitHub run number supplies
the build number. Local developer bundles use the workspace version for both.

`st gui [path]` continues to launch a fresh instance through LaunchServices,
but switches to `com.cesarferreira.stax`. Its error copy points first to the
GitHub Release installation flow and still mentions `make install-gui-app` for
contributors.

## Build and archive boundary

`scripts/build-gui-app.sh` owns app assembly from a compiled executable. It
must support deterministic fixture injection through `STAX_GUI_BINARY`, copy
the icon, substitute validated metadata, optionally install locally, and never
perform signing or notarization.

`scripts/package-gui-release.sh` owns release-mode compilation, architecture
validation, optional code signing, optional notarization, archive creation,
archive extraction, and headless smoke verification. Keeping these concerns
out of the assembly script preserves fast fixture tests.

The packaged executable exposes `--version` and exits before GPUI starts. The
release smoke test extracts the uploaded zip, validates the plist and Mach-O
architecture, runs `Stax.app/Contents/MacOS/Stax --version`, and enforces a
configurable compressed-size ceiling. This provides a display-independent
test on hosted macOS runners.

## Accessibility hardening

GPUI 0.2.2 does not expose a stable macOS accessibility-node API for these
custom elements, so this phase hardens the supported interaction surface
instead of claiming unavailable screen-reader semantics:

- every visible enabled button is a keyboard tab stop;
- Enter and Space activate the focused button exactly once;
- disabled controls are skipped and retain an explicit visible reason;
- focus is restored after dialogs and operation completion;
- focus indication keeps its existing non-text contrast guarantee;
- controls retain visible text labels rather than icon-only affordances;
- no core action depends exclusively on pointer input.

Tests cover the toolbar, inspector, receipt, welcome, and dialog controls. The
GUI guide states the current screen-reader limitation plainly.

## Size and compatibility

The GUI app is a separate executable and archive, so the existing CLI binary
does not become larger as a consequence of GPUI packaging. The release job
records both uncompressed executable size and compressed archive size. A
default 80 MiB compressed ceiling catches accidental debug or universal builds
while leaving room for architecture and toolchain variance; callers may lower
the ceiling through `STAX_GUI_MAX_ARCHIVE_BYTES`.

## Out of scope

- Mac App Store distribution and sandbox entitlements.
- Sparkle or another in-app updater.
- A Homebrew cask.
- A universal macOS binary.
- Windows or Linux GUI builds.
- Automatic certificate procurement or storage outside GitHub Actions.
