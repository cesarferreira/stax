# Release workflow

Cut a new `stax` release with an auto-generated changelog entry.

```bash
make release                  # defaults to a minor bump
make release LEVEL=patch

just release-patch
just release-minor
just release-major
```

During `cargo release`'s pre-release hook, the workflow rebuilds the upcoming release entry in `CHANGELOG.md` from commits since the latest `v<major>.<minor>.<patch>` tag, refreshes the compare links, and leaves a fresh empty `Unreleased` header at the top.

## What gets generated

| Commit prefix | Section |
|---|---|
| `feat:` (and related) | `### Added` |
| `fix:` | `### Fixed` |
| `docs:` | `### Documentation` |
| anything else | `### Changed` |

The generator strips the conventional-commit prefix, keeps PR references like `(#123)`, and overwrites any stale manual `Unreleased` body before `cargo release` commits the version bump — the release target stays deterministic.

## Failure mode

If no non-merge commits exist since the last version tag, release prep fails with `No commits found since last tag` and the release stops before `cargo release` would create an empty version entry.

## Notes

- `make release` defaults to a minor bump. Use `LEVEL=patch` or the `just release-*` targets to control the semver bump.
- `release-dry` is a plain `cargo release` dry run. The hook sees `DRY_RUN=true` and leaves `CHANGELOG.md` untouched.
