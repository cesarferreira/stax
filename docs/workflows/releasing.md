# Release workflow

Cut a new `stax` release with an auto-generated changelog entry.

```bash
make release                  # defaults to a minor bump
make release LEVEL=patch      # patch bump
make release LEVEL=major      # major bump
```

During `cargo release`'s pre-release hook, [git-cliff](https://git-cliff.org/) regenerates the whole `CHANGELOG.md` from the git history, treating the commits since the latest `v<major>.<minor>.<patch>` tag as the version being released. Configuration lives in [`cliff.toml`](../../cliff.toml).

## What gets generated

git-cliff groups commits by their [conventional-commit](https://www.conventionalcommits.org) type:

| Commit prefix | Section |
|---|---|
| `feat:` | `🚀 Features` |
| `fix:` | `🐛 Bug Fixes` |
| `refactor:` | `🚜 Refactor` |
| `perf:` | `⚡ Performance` |
| `docs:` | `📚 Documentation` |
| `style:` | `🎨 Styling` |
| `test:` | `🧪 Testing` |
| `chore:` / `ci:` | `⚙️ Miscellaneous Tasks` |
| `revert:` | `◀️ Revert` |
| anything else | `💼 Other` |

PR references like `(#123)` become links to the GitHub repo. The automated `chore: Release stax version …` commits and `chore(deps…)` bumps are skipped. Non-conventional squash-merge subjects are kept (in `💼 Other`) rather than dropped, so no change silently disappears.

## Notes

- `make release` defaults to a minor bump. Use `LEVEL=patch`, `LEVEL=minor`, or `LEVEL=major` to control the semver bump.
- For a dry run, invoke `cargo release <level> --no-confirm` directly without `--execute`; cargo-release skips the version bump, tag, and push. git-cliff still rewrites `CHANGELOG.md` in place — run `git checkout CHANGELOG.md` to discard it.
- Preview the upcoming entry without touching the file: `git-cliff --unreleased --tag v<next-version>`.
- git-cliff must be installed locally (`cargo install git-cliff`) for `make release` to work.

## Native macOS app artifacts

The release workflow keeps the five existing CLI archives and adds two independent GUI archives:

- `Stax-aarch64-apple-darwin.zip` for Apple Silicon
- `Stax-x86_64-apple-darwin.zip` for Intel

These contain `Stax.app` with bundle id `com.cesarferreira.stax`. They are separate artifacts on the same GitHub Release, not a new crates.io package. Homebrew checksum updates remain CLI-only. The GUI package test validates the bundle metadata, executable architecture and version, and enforces an 80 MiB archive ceiling without increasing either CLI binary.

Ad-hoc-signed artifacts are the no-secret baseline. Optional Developer ID signing and notarization use these six repository secrets:

| Secret | Purpose |
|---|---|
| `MACOS_CERTIFICATE_P12` | Base64-encoded Developer ID Application certificate |
| `MACOS_CERTIFICATE_PASSWORD` | Password for the PKCS#12 certificate |
| `MACOS_SIGNING_IDENTITY` | Exact Developer ID Application identity passed to `codesign` |
| `APPLE_ID` | Apple account used by `notarytool` |
| `APPLE_TEAM_ID` | Apple Developer team id |
| `APPLE_APP_PASSWORD` | App-specific password used by `notarytool` |

The three `MACOS_*` certificate values are all-or-none. With all three, CI imports a temporary keychain and signs with the hardened runtime; with none, it ad-hoc signs the complete app bundle so its sealed resources remain valid. The three notarization values are also all-or-none and require a complete signing configuration. Partial configuration fails the release instead of silently downgrading it. With all six values, CI submits each signed archive, waits for acceptance, staples the ticket, validates it, and recreates the same artifact filename.

To inspect an extracted release locally:

```bash
codesign -dv --verbose=4 Stax.app
codesign --verify --deep --strict --verbose=2 Stax.app
spctl --assess --type execute --verbose=4 Stax.app
Stax.app/Contents/MacOS/Stax --version
```

Without Developer ID credentials, the first command reports an ad-hoc signature and `spctl` rejects the app; that is expected for the supported per-app **Privacy & Security → Open Anyway** flow. Strict `codesign` verification must still pass so Gatekeeper does not report the bundle as damaged. `make gui-release-test` exercises ad-hoc environment validation and a native architecture-specific package. `scripts/gui-release-workflow-tests.sh` checks the CI artifact and optional secret contract.
