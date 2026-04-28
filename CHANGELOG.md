# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

## [0.62.0] - 2026-04-28

### Added
- Match `gt fold` semantics — preserve commits, reparent descendants, fix `--keep` (#344)

## [0.61.0] - 2026-04-28

### Changed
- Unify stack lane colors across ls and checkout
- White
- Bco colors

## [0.60.0] - 2026-04-28

### Fixed
- Avoid slow ls git scans (#342)

## [0.59.0] - 2026-04-27

### Added
- Add --no-verify for push hooks (#340)

### Changed
- Eliminate O(N) git work per branch (#341)

## [0.58.0] - 2026-04-26

### Added
- Add --no-verify flag (#337)

### Fixed
- Make --from and --below commits interruption-safe (#339)

## [0.57.0] - 2026-04-25

### Added
- Support non-interactive submit (#327)
- Add --below placement (#333)

### Changed
- Stop tracking Python bytecode from release prep (#321)
- Cover auto-stash-pop linked worktree flow (#326)
- Automate interactive menu paths (#328)
- Fix redundant merge PR base retargets (#329)

### Fixed
- Preserve remaining stack chain on rebase (#311) (#318)

## [0.56.0] - 2026-04-21

### Changed
- Cesar/rewrite readme (#313)
- Add refresh command for sync/restack/submit flow (#314)
- [codex] Add verbose refresh/restack timing diagnostics (#320)
- Fix release script

### Fixed
- Push remaining branches before retargeting PR base (#312) (#317)

### Documentation
- Rewrite all user-facing docs with consistent, tighter structure (#319)

## [0.55.0] - 2026-04-20

### Added
- `stax modify` and `stax create -m`: when nothing is staged in a TTY, replace the yes/no prompt with a Graphite-style menu offering `--patch` (selective `git add -p`), "continue without staging" (empty branch on `create`; amend message only on `modify`), and abort, in addition to the existing "stage all" option. Non-TTY behavior unchanged. (#309)
- `stax create` wizard: pick `--patch` alongside "stage all" and "empty branch".
## [0.54.0] - 2026-04-20

## [0.53.0] - 2026-04-17

## [0.52.0] - 2026-04-16

## [0.51.0] - 2026-04-16

## [0.50.2] - 2026-04-14

## [0.50.1] - 2026-04-14

## [0.50.0] - 2026-04-14

## [0.49.0] - 2026-04-12

### Summary
Major feature release introducing new commands (absorb, edit, upstack onto), enhanced TUI capabilities with worktree removal, improved submit workflow with PR title auto-update and draft/publish toggles, and comprehensive doctor checks. Also includes split command enhancements and numerous reliability improvements.

### Added
- `st absorb` command for automatic change distribution across stack
- `st edit` command for interactive commit editing
- `st upstack onto` for mass reparent with descendants
- `--insert` flag for `st create` to insert branches mid-stack
- `--file` flag for `st split` for pathspec-based splitting
- `--yolo` and `--agent-arg` flags to `st lane` and `st wt create`
- `--update-title` flag to gate PR title auto-update in submit
- `--publish`/`--draft` toggle for existing PRs in submit
- `--squash` flag for submit with roborev integration
- TUI: Force delete confirmation and removal progress tracking
- TUI: Two-stage confirmation for dirty worktree removal
- Conflict position indicator in restack stack view
- Post-operation next-step hints for better UX
- Doctor checks for diverged trunk, git config, and stale PR metadata
- Parameterized `make release` target with configurable version bump level

### Fixed
- Sync: Reparent tracked children before delete-upstream-gone (#280)
- Submit: Update PR titles even on no-op submits
- Split: Rollback file splits and cover the flow
- Restack: Hold auto-stashes until restack finishes
- Create: Rollback on metadata and git spawn failures
- Create: Rollback branch when commit fails during `st create -m`
- Sync: Only count metadata cleanup when it succeeds
- Sync: Warn on metadata deletion failures instead of silently ignoring
- Sync: Add ancestor check before trunk hard-reset
- Merge-queue: Rollback PR base on enqueue failure
- Push: Use --force-with-lease instead of -f for all force pushes
- Doctor: Count diverged trunk and stale metadata properly
- Absorb: Cover and clean up non-dry-run flow
- Persist reparent metadata only after successful restack
- Surface config load failures in modify hints

### Documentation
- VS Code / Cursor integration recipe for agent worktrees
- Added --publish/--draft flags to command reference
- Added st split --file to command reference and compatibility matrix

## [0.46.0] - 2026-04-10

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

## [0.45.0] - 2026-04-08

### Summary
A maintenance release focused on improving sync command reliability, particularly around worktree cleanup and handling of closed PRs. Also enhances cross-forge compatibility with better markdown link handling.

### Fixed
- Sync command now honors dirty worktree confirmation prompts (#229)
- Force cleanup of dirty linked worktrees during sync (#227)
- Use full markdown links for stack comments on GitLab and Gitea for better compatibility (#226)
- Ignore closed unmerged PRs during sync cleanup to avoid stale state (#207)

## [0.44.2] - 2026-04-07

### Summary
Quick patch release addressing tmux integration issues that affected the lanes workflow.

### Fixed
- Don't exec switch-client inside tmux, preserving user's shell on detach (#224)
- Handle tmux no-server state gracefully for lanes (#223)

## [0.44.1] - 2026-04-07

### Summary
This release improves the `generate` command's PR body handling and adds a quality-of-life feature for the `create` command.

### Added
- Prompt to stage files when nothing is staged during `stax create` (#211)

### Fixed
- `generate --pr-body` now has parity with submit for PR template selection (#220)
- Fixed dirty check logic
- Fixed broken tests

## [0.44.0] - 2026-04-05

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
[Unreleased]: https://github.com/cesarferreira/stax/compare/v0.62.0...HEAD
[0.62.0]: https://github.com/cesarferreira/stax/compare/v0.61.0...v0.62.0
[0.61.0]: https://github.com/cesarferreira/stax/compare/v0.60.0...v0.61.0
[0.60.0]: https://github.com/cesarferreira/stax/compare/v0.59.0...v0.60.0
[0.59.0]: https://github.com/cesarferreira/stax/compare/v0.58.0...v0.59.0
[0.58.0]: https://github.com/cesarferreira/stax/compare/v0.57.0...v0.58.0
[0.57.0]: https://github.com/cesarferreira/stax/compare/v0.56.0...v0.57.0
[0.56.0]: https://github.com/cesarferreira/stax/compare/v0.55.0...v0.56.0
[0.55.0]: https://github.com/cesarferreira/stax/compare/v0.54.0...v0.55.0
[0.54.0]: https://github.com/cesarferreira/stax/compare/v0.53.0...v0.54.0
[0.53.0]: https://github.com/cesarferreira/stax/compare/v0.52.0...v0.53.0
[0.52.0]: https://github.com/cesarferreira/stax/compare/v0.51.0...v0.52.0
[0.51.0]: https://github.com/cesarferreira/stax/compare/v0.50.2...v0.51.0
[0.50.2]: https://github.com/cesarferreira/stax/compare/v0.50.1...v0.50.2
[0.50.1]: https://github.com/cesarferreira/stax/compare/v0.50.0...v0.50.1
[0.50.0]: https://github.com/cesarferreira/stax/compare/v0.49.0...v0.50.0
[0.49.0]: https://github.com/cesarferreira/stax/compare/v0.48.0...v0.49.0
[0.46.0]: https://github.com/cesarferreira/stax/compare/v0.45.0...v0.46.0
[0.45.0]: https://github.com/cesarferreira/stax/compare/v0.44.2...v0.45.0
[0.44.2]: https://github.com/cesarferreira/stax/compare/v0.44.1...v0.44.2
[0.44.1]: https://github.com/cesarferreira/stax/compare/v0.44.0...v0.44.1
[0.44.0]: https://github.com/cesarferreira/stax/compare/v0.43.0...v0.44.0
