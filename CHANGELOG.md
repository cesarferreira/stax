# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

### Added
- Parameterized `make release` target with configurable version bump level (minor/patch/major)

## [0.46.0] - 2025-01-XX

### Summary
This release focuses on improving PR workflows with better metadata handling, smarter merge behavior, and expanded support for GitHub merge queues and GitLab merge trains. The `modify` command now supports automatic restacking, and the split TUI received important bug fixes.

### Added
- `--restack` flag to `stax modify` for automatic restacking after modifications (#237)
- `--queue` flag for `stax merge` to support GitHub merge queue and GitLab merge train (#236)
- New `stack` command group with `sr` and `ss` aliases for improved ergonomics (#230)

### Fixed
- PR and comments commands now fall back to forge lookup when PR metadata is missing (#239)
- Merge command now correctly retargets dependent PRs after merge, not before (#238)
- Split TUI scrolling and patch application issues (#234)

### Changed
- Collapsed non-macOS install instructions in README for better readability

## [0.45.0] - 2025-01-XX

### Summary
A maintenance release focused on improving sync command reliability, particularly around worktree cleanup and handling of closed PRs. Also enhances cross-forge compatibility with better markdown link handling.

### Fixed
- Sync command now honors dirty worktree confirmation prompts (#229)
- Force cleanup of dirty linked worktrees during sync (#227)
- Use full markdown links for stack comments on GitLab and Gitea for better compatibility (#226)
- Ignore closed unmerged PRs during sync cleanup to avoid stale state (#207)

## [0.44.2] - 2025-01-XX

### Summary
Quick patch release addressing tmux integration issues that affected the lanes workflow.

### Fixed
- Don't exec switch-client inside tmux, preserving user's shell on detach (#224)
- Handle tmux no-server state gracefully for lanes (#223)

## [0.44.1] - 2025-01-XX

### Summary
This release improves the `generate` command's PR body handling and adds a quality-of-life feature for the `create` command.

### Added
- Prompt to stage files when nothing is staged during `stax create` (#211)

### Fixed
- `generate --pr-body` now has parity with submit for PR template selection (#220)
- Fixed dirty check logic
- Fixed broken tests

## [0.44.0] - 2025-01-XX

### Summary
Major release introducing per-feature AI agent and model configuration with an improved first-use experience.

### Added
- Per-feature AI agent and model configuration system (#215)
- Enhanced first-use UX for AI features
- Lane branch submit tests (#218)

### Changed
- CI no longer blocks releases on Windows test failures (#217)
- Expanded Linux arm64 prebuilt install instructions (#216)

### Documentation
- Expanded `st lane` guide with more examples and use cases (#214)

<!-- next-url -->
[Unreleased]: https://github.com/cesarferreira/stax/compare/v0.46.0...HEAD
[0.46.0]: https://github.com/cesarferreira/stax/compare/v0.45.0...v0.46.0
[0.45.0]: https://github.com/cesarferreira/stax/compare/v0.44.2...v0.45.0
[0.44.2]: https://github.com/cesarferreira/stax/compare/v0.44.1...v0.44.2
[0.44.1]: https://github.com/cesarferreira/stax/compare/v0.44.0...v0.44.1
[0.44.0]: https://github.com/cesarferreira/stax/compare/v0.43.0...v0.44.0
