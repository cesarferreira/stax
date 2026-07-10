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
- Pull requests compile every release target in `.github/workflows/cross-platform.yml`; tag builds then package the same five targets.
- The root `action.yml` composite action downloads those release archives. Keep its platform map, the release matrix, the artifact verification list, and `scripts/install-action.sh` aligned when adding a target.
