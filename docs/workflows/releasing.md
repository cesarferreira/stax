# Release Workflow

Use the repo-level release targets when cutting a new `stax` release:

```bash
make release
make release LEVEL=patch

just release-patch
just release-minor
just release-major
```

Before `cargo release` runs, the workflow now regenerates the `Unreleased` section in `CHANGELOG.md` from commits since the latest `v<major>.<minor>.<patch>` tag.

## What Gets Generated

- `feat:` and related additions go under `### Added`
- `fix:` commits go under `### Fixed`
- `docs:` commits go under `### Documentation`
- everything else falls back to `### Changed`

The generator strips the conventional-commit prefix, keeps PR references like `(#123)`, and overwrites any stale manual `Unreleased` body so the release target stays deterministic.

## Failure Mode

If there are no non-merge commits since the last version tag, release prep fails with `No commits found since last tag` and the release stops before `cargo release` can create an empty version entry.

## Notes

- `make release` still defaults to a minor bump. Use `LEVEL=patch` or the `just release-*` targets to control the semver bump.
- `release-dry` remains a plain `cargo release` dry run and does not rewrite the changelog.
