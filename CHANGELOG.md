# Changelog

All notable changes to this project will be documented in this file.

## [0.93.0] - 2026-07-06

### 🚀 Features

- *(worktree)* Auto-seed dependencies into new worktrees ([#570](https://github.com/cesarferreira/stax/issues/570))

### 🐛 Bug Fixes

- Let GitHub mark downstack stack PRs merged ([#574](https://github.com/cesarferreira/stax/issues/574))
- Fail closed on release build failures ([#576](https://github.com/cesarferreira/stax/issues/576))
- Speed up native full-test path ([#578](https://github.com/cesarferreira/stax/issues/578))

### 💼 Other

- Add native GitHub Stacked PR integration ([#564](https://github.com/cesarferreira/stax/issues/564))
- Revert "Add native GitHub Stacked PR integration ([#564](https://github.com/cesarferreira/stax/issues/564))" ([#566](https://github.com/cesarferreira/stax/issues/566))
- Increase GitHub Pages deploy timeout to handle queued deployments ([#569](https://github.com/cesarferreira/stax/issues/569))
- Fix full stack temporary submit restacks ([#572](https://github.com/cesarferreira/stax/issues/572))

### 📚 Documentation

- Remove nonexistent task install path ([#575](https://github.com/cesarferreira/stax/issues/575))
- Document merge when-ready queue conflict ([#580](https://github.com/cesarferreira/stax/issues/580))
- Cover visible commands in reference ([#582](https://github.com/cesarferreira/stax/issues/582))

### 🧪 Testing

- Cover copy pr clipboard fallback ([#577](https://github.com/cesarferreira/stax/issues/577))
## [0.92.0] - 2026-07-02

### 🐛 Bug Fixes

- Keep skills.md version marker in sync and clarify update output ([#561](https://github.com/cesarferreira/stax/issues/561))
- Add TUI pane visibility toggles ([#555](https://github.com/cesarferreira/stax/issues/555))
- Don't flag empty never-pushed branches as merged ([#562](https://github.com/cesarferreira/stax/issues/562))
## [0.91.2] - 2026-07-02

### 🐛 Bug Fixes

- Reset local trunk to remote before post-merge sync to avoid diverged warning
- Guard trunk reset against wrong-branch and push-failure edge cases

### 💼 Other

- Improve merge prompts and blocked PR feedback ([#556](https://github.com/cesarferreira/stax/issues/556))
- Streamline merge command output ([#557](https://github.com/cesarferreira/stax/issues/557))
## [0.91.1] - 2026-07-01

### 💼 Other

- Switch changelog and release notes to git-cliff ([#551](https://github.com/cesarferreira/stax/issues/551))
## [0.91.0] - 2026-06-29

### 💼 Other

- Fix scoped submit PR default ranges ([#544](https://github.com/cesarferreira/stax/issues/544))
- Fix get sync for existing local branches ([#546](https://github.com/cesarferreira/stax/issues/546))

### 📚 Documentation

- Add issue list to skills ([#548](https://github.com/cesarferreira/stax/issues/548))
## [0.89.0] - 2026-06-27

### 💼 Other

- Allow scoped submit with temporary restack ([#541](https://github.com/cesarferreira/stax/issues/541))

### 🧪 Testing

- Cover copy fallback without clipboard ([#538](https://github.com/cesarferreira/stax/issues/538))
## [0.88.0] - 2026-06-24

### 🐛 Bug Fixes

- Mark absorbed stack PRs explicitly ([#513](https://github.com/cesarferreira/stax/issues/513))
- Show git push hook output on submit failure
- Show git push hook output on submit failure ([#518](https://github.com/cesarferreira/stax/issues/518))
- Reject zero merge polling interval ([#526](https://github.com/cesarferreira/stax/issues/526))
- Actionable non-interactive error for dirty wt rm ([#527](https://github.com/cesarferreira/stax/issues/527))
- Treat latest cancelled rollup check as CI failure ([#528](https://github.com/cesarferreira/stax/issues/528))
- Correct misleading branch fold --keep help text ([#525](https://github.com/cesarferreira/stax/issues/525))
- Require explicit --yes for quiet stack merges ([#529](https://github.com/cesarferreira/stax/issues/529))
- Print value when clipboard unavailable in copy command ([#532](https://github.com/cesarferreira/stax/issues/532))
- Detect squash-merged branches in sweep ([#524](https://github.com/cesarferreira/stax/issues/524))
- Protect upstream-gone branches with local-only commits in sync ([#533](https://github.com/cesarferreira/stax/issues/533))

### 💼 Other

- Revert "fix: show git push hook output on submit failure ([#518](https://github.com/cesarferreira/stax/issues/518))"
- Reapply "fix: show git push hook output on submit failure ([#518](https://github.com/cesarferreira/stax/issues/518))"

### 📚 Documentation

- Fix status alias reference ([#512](https://github.com/cesarferreira/stax/issues/512))
- Clarify sync --prune is a no-op ([#521](https://github.com/cesarferreira/stax/issues/521))
- Fix broken links in CONTRIBUTING and tmux page ([#522](https://github.com/cesarferreira/stax/issues/522))
- Document branch.stale_days and git.rerere config keys ([#523](https://github.com/cesarferreira/stax/issues/523))

### 🧪 Testing

- Use real trunk command in tests and assert success ([#530](https://github.com/cesarferreira/stax/issues/530))
## [0.87.2] - 2026-06-19

### 🐛 Bug Fixes

- Remove blocking git ls-files call and add progress spinner on worktree create
## [0.87.1] - 2026-06-17

### 🐛 Bug Fixes

- Align sweep cleanup with sync ([#506](https://github.com/cesarferreira/stax/issues/506))
- Respect pending PR rollup in ci watch ([#507](https://github.com/cesarferreira/stax/issues/507))
## [0.87.0] - 2026-06-16

### 🚀 Features

- Add stack fast-forward merge ([#502](https://github.com/cesarferreira/stax/issues/502))

### 🐛 Bug Fixes

- *(test)* Stabilize scripted TUI delays ([#499](https://github.com/cesarferreira/stax/issues/499))

### 💼 Other

- Treat st get branches as imported read-only bases ([#501](https://github.com/cesarferreira/stax/issues/501))
## [0.86.3] - 2026-06-13

### 💼 Other

- Apple container testing ([#495](https://github.com/cesarferreira/stax/issues/495))
- Revert "[codex] Disable Windows release builds ([#488](https://github.com/cesarferreira/stax/issues/488))" ([#497](https://github.com/cesarferreira/stax/issues/497))

### ⚡ Performance

- *(ci)* Use mold linker and test-container profile in Rust Tests ([#498](https://github.com/cesarferreira/stax/issues/498))
## [0.86.2] - 2026-06-10

### 💼 Other

- Update cargo lock ([#486](https://github.com/cesarferreira/stax/issues/486))
- Bump versions
- [codex] Disable Windows release builds ([#488](https://github.com/cesarferreira/stax/issues/488))

### ⚡ Performance

- *(build)* Cut test link time with line-tables-only debuginfo ([#489](https://github.com/cesarferreira/stax/issues/489))
- *(test)* Consolidate integration tests into a single binary ([#490](https://github.com/cesarferreira/stax/issues/490))

### ⚙️ Miscellaneous Tasks

- *(test)* Track latest stable Rust in the Docker test fast-path ([#491](https://github.com/cesarferreira/stax/issues/491))
## [0.86.1] - 2026-06-08

### 💼 Other

- Show remote indicator for pushed branches ([#477](https://github.com/cesarferreira/stax/issues/477))
## [0.86.0] - 2026-06-08

### 🐛 Bug Fixes

- Exclude current branch from sweep candidates ([#474](https://github.com/cesarferreira/stax/issues/474))

### 💼 Other

- [codex] fix sweep upstream-gone safety ([#476](https://github.com/cesarferreira/stax/issues/476))

### ⚙️ Miscellaneous Tasks

- Bump to Rust edition 2024 ([#470](https://github.com/cesarferreira/stax/issues/470))
## [0.85.1] - 2026-06-04

### 🐛 Bug Fixes

- *(github)* Resolved changes-requested reviews no longer block PRs (re-fix #376) ([#469](https://github.com/cesarferreira/stax/issues/469))
## [0.85.0] - 2026-06-04

### 🚀 Features

- Add stax sweep command for branch housekeeping ([#468](https://github.com/cesarferreira/stax/issues/468))

### 🐛 Bug Fixes

- Replace stale `stax shell-setup --install` hints with `stax setup` ([#465](https://github.com/cesarferreira/stax/issues/465))
- *(ready)* Treat no-review-required PRs as ready to merge ([#466](https://github.com/cesarferreira/stax/issues/466))
- *(tui)* Render Mergeable detail with human label ([#467](https://github.com/cesarferreira/stax/issues/467))

### 💼 Other

- Potential fix for code scanning alert no. 2: Workflow does not contain permissions ([#463](https://github.com/cesarferreira/stax/issues/463))
## [0.84.1] - 2026-06-03

### 💼 Other

- Configure Dependabot for Cargo and GitHub Actions
## [0.84.0] - 2026-06-02

### 💼 Other

- Add get command for remote branches ([#449](https://github.com/cesarferreira/stax/issues/449))
## [0.83.0] - 2026-06-02

### 🚀 Features

- Add PR readiness view ([#447](https://github.com/cesarferreira/stax/issues/447))

### 💼 Other

- Show PR approval status in watch oneline view ([#443](https://github.com/cesarferreira/stax/issues/443))
## [0.82.0] - 2026-05-28

### 🚀 Features

- Add doctor fix mode
- *(ci)* Add --oneline compact whole-stack view ([#440](https://github.com/cesarferreira/stax/issues/440))

### 🐛 Bug Fixes

- Search commits in changelog find ([#438](https://github.com/cesarferreira/stax/issues/438))

### 💼 Other

- Add codegraph
- Added cursor codegraph mdc

### 🚜 Refactor

- *(cli)* Split 2987-line cli.rs into focused submodules
## [0.81.2] - 2026-05-27

### 🐛 Bug Fixes

- Ensure update check cache is written before process exits
## [0.81.1] - 2026-05-27

### 🐛 Bug Fixes

- Skip full PR scan in read-only lookups, cloud icon means has PR
- Bound sync PR metadata refresh

### 💼 Other

- Recover existing PR on duplicate submit create ([#435](https://github.com/cesarferreira/stax/issues/435))
## [0.81.0] - 2026-05-27

### 💼 Other

- Reformat command code for consistency ([#427](https://github.com/cesarferreira/stax/issues/427))
- Make tmux branch display Unicode-safe and truncate trunk names ([#428](https://github.com/cesarferreira/stax/issues/428))
- Paginate open issues to avoid missing results on PR-heavy pages ([#429](https://github.com/cesarferreira/stax/issues/429))
- Always restack branches when moving to a new parent ([#434](https://github.com/cesarferreira/stax/issues/434))
## [0.80.0] - 2026-05-23

### 🚀 Features

- Support repo-local config ([#360](https://github.com/cesarferreira/stax/issues/360))

### 🐛 Bug Fixes

- Skip reparenting doomed merged branches ([#425](https://github.com/cesarferreira/stax/issues/425))
## [0.79.0] - 2026-05-21

### 🚀 Features

- *(config)* Add submit.single_stack option ([#421](https://github.com/cesarferreira/stax/issues/421))
## [0.78.2] - 2026-05-21

### 🐛 Bug Fixes

- Sync PR state and base from GitHub during sync ([#420](https://github.com/cesarferreira/stax/issues/420))
## [0.78.1] - 2026-05-20

### 💼 Other

- Fix stack link context per PR ([#422](https://github.com/cesarferreira/stax/issues/422))
## [0.78.0] - 2026-05-20

### 🐛 Bug Fixes

- Avoid duplicate fetch during update ([#419](https://github.com/cesarferreira/stax/issues/419))

### 📚 Documentation

- Remove stale just target references ([#418](https://github.com/cesarferreira/stax/issues/418))
## [0.77.0] - 2026-05-19

### 🐛 Bug Fixes

- Skip merged cleanup during update ([#415](https://github.com/cesarferreira/stax/issues/415))

### 💼 Other

- Guard create --below collisions and scoped stack links ([#413](https://github.com/cesarferreira/stax/issues/413))
- Removed unnessessary files
## [0.76.0] - 2026-05-19

### 🚀 Features

- Add release-aware changelog search ([#410](https://github.com/cesarferreira/stax/issues/410))
- Add create alias ([#414](https://github.com/cesarferreira/stax/issues/414))
## [0.75.0] - 2026-05-19

### 💼 Other

- Renamed refresh to update
## [0.74.2] - 2026-05-19

### 🐛 Bug Fixes

- Prevent PR body diff truncation from panicking on UTF-8 boundary ([#412](https://github.com/cesarferreira/stax/issues/412))
## [0.74.1] - 2026-05-17

### 🐛 Bug Fixes

- Fixed colors for tmux

### 💼 Other

- Update tmux
## [0.74.0] - 2026-05-16

### 🐛 Bug Fixes

- Verify remote draft state before undraft no-op ([#406](https://github.com/cesarferreira/stax/issues/406))

### 💼 Other

- Add PR body view and editing ([#407](https://github.com/cesarferreira/stax/issues/407))
## [0.73.0] - 2026-05-14

### 🐛 Bug Fixes

- Fail closed on review lookup errors ([#401](https://github.com/cesarferreira/stax/issues/401))
- Detect Windows cargo installs ([#397](https://github.com/cesarferreira/stax/issues/397))

### ⚡ Performance

- Reuse loaded stack for cascade navigation ([#398](https://github.com/cesarferreira/stax/issues/398))
- Parallelize status JSON line stats ([#399](https://github.com/cesarferreira/stax/issues/399))
- Attribute absorb files with one log walk ([#400](https://github.com/cesarferreira/stax/issues/400))
- Batch submit branch pushes ([#402](https://github.com/cesarferreira/stax/issues/402))
- Parallelize submit PR discovery ([#403](https://github.com/cesarferreira/stax/issues/403))

### 🧪 Testing

- Test fix
## [0.72.0] - 2026-05-14

### 💼 Other

- Add live stack watch view ([#391](https://github.com/cesarferreira/stax/issues/391))
- Add draft and undraft commands for tracked PRs ([#390](https://github.com/cesarferreira/stax/issues/390))
- Add tmux status bar and popup support ([#393](https://github.com/cesarferreira/stax/issues/393))
- Refresh PR draft state from GitHub during rs ([#394](https://github.com/cesarferreira/stax/issues/394))
- New screenshot

### ⚡ Performance

- *(ci)* Parallelize fetch_ci_statuses with join_all ([#395](https://github.com/cesarferreira/stax/issues/395))
## [0.71.1] - 2026-05-13

### 💼 Other

- Show pull request changes-requested status correctly ([#376](https://github.com/cesarferreira/stax/issues/376))
## [0.71.0] - 2026-05-12

### 💼 Other

- Add downstack-only merge scope ([#372](https://github.com/cesarferreira/stax/issues/372))
- Auto-stash dirty worktrees for create --below ([#374](https://github.com/cesarferreira/stax/issues/374))
## [0.69.2] - 2026-05-07

### 💼 Other

- Fix CI watch terminal-state handling ([#368](https://github.com/cesarferreira/stax/issues/368))
## [0.69.1] - 2026-05-07

### 💼 Other

- Fix worktree create for remote branches ([#362](https://github.com/cesarferreira/stax/issues/362))
## [0.69.0] - 2026-05-07

### 🐛 Bug Fixes

- *(ci)* Stop using end_offset_secs for ETA — it's polluted by main-branch builds

### 💼 Other

- Show checkout progress while switching branches ([#367](https://github.com/cesarferreira/stax/issues/367))
- Add regression coverage for restack provenance and trunk churn ([#363](https://github.com/cesarferreira/stax/issues/363))
- Warn before restack when provenance boundary drifts ([#364](https://github.com/cesarferreira/stax/issues/364))
- Warn before restack replays the wrong boundary ([#365](https://github.com/cesarferreira/stax/issues/365))
## [0.68.0] - 2026-05-05

### 🐛 Bug Fixes

- Use resolve_model guard in resolve command to prevent cross-agent model bleed

### 💼 Other

- Compact checkout divergence labels ([#357](https://github.com/cesarferreira/stax/issues/357))
- Optimize install
- Make checkout, restack, and sync faster ([#358](https://github.com/cesarferreira/stax/issues/358))
- Add AI generation hub and rename standup flag ([#359](https://github.com/cesarferreira/stax/issues/359))
## [0.67.1] - 2026-05-05

### 💼 Other

- Add GPT-5.5 model defaults ([#356](https://github.com/cesarferreira/stax/issues/356))
## [0.67.0] - 2026-05-05

### 🚀 Features

- *(ci)* Add watch completion alerts ([#354](https://github.com/cesarferreira/stax/issues/354))

### 💼 Other

- Cache TUI diffs across sessions ([#355](https://github.com/cesarferreira/stax/issues/355))
## [0.66.1] - 2026-05-01

### 🐛 Bug Fixes

- *(skills)* Compare against PKG_VERSION instead of stale upstream marker

### 📚 Documentation

- *(skills)* Drop per-agent install snippets from skill body
## [0.66.0] - 2026-05-01

### 💼 Other

- Track branches from the merge-base for safer restacks ([#352](https://github.com/cesarferreira/stax/issues/352))
## [0.65.1] - 2026-04-30

### 🐛 Bug Fixes

- Keep TUI responsive while loading branch data ([#351](https://github.com/cesarferreira/stax/issues/351))
## [0.65.0] - 2026-04-30

### 💼 Other

- Standup improvements ([#347](https://github.com/cesarferreira/stax/issues/347))
- Fix cli upgrade for cargo-binstall installs ([#346](https://github.com/cesarferreira/stax/issues/346))
- Stack health ([#349](https://github.com/cesarferreira/stax/issues/349))
- Add AI-assisted branch creation and PR drafting ([#350](https://github.com/cesarferreira/stax/issues/350))

### 🧪 Testing

- *(refresh)* Cover squash-merged parent restack ([#348](https://github.com/cesarferreira/stax/issues/348))
## [0.64.0] - 2026-04-29

### 💼 Other

- Align TUI stack tree with ls colors and BCO selection
## [0.63.0] - 2026-04-29

### 💼 Other

- Style checkout picker rows with active background ([#345](https://github.com/cesarferreira/stax/issues/345))
## [0.62.1] - 2026-04-28

### 💼 Other

- Handle squash-merged parents during restack
## [0.62.0] - 2026-04-28

### 🚀 Features

- *(fold)* Match `gt fold` semantics — preserve commits, reparent descendants, fix `--keep` ([#344](https://github.com/cesarferreira/stax/issues/344))
## [0.61.0] - 2026-04-28

### 💼 Other

- Unify stack lane colors across ls and checkout
- White
- Bco colors
## [0.60.0] - 2026-04-28

### 🐛 Bug Fixes

- *(status)* Avoid slow ls git scans ([#342](https://github.com/cesarferreira/stax/issues/342))
## [0.59.0] - 2026-04-27

### 🚀 Features

- *(submit)* Add --no-verify for push hooks ([#340](https://github.com/cesarferreira/stax/issues/340))

### ⚡ Performance

- *(restack)* Eliminate O(N) git work per branch ([#341](https://github.com/cesarferreira/stax/issues/341))
## [0.58.0] - 2026-04-26

### 🚀 Features

- *(create)* Add --no-verify flag ([#337](https://github.com/cesarferreira/stax/issues/337))

### 🐛 Bug Fixes

- *(create)* Make --from and --below commits interruption-safe ([#339](https://github.com/cesarferreira/stax/issues/339))
## [0.57.0] - 2026-04-25

### 🚀 Features

- *(refresh)* Support non-interactive submit ([#327](https://github.com/cesarferreira/stax/issues/327))
- *(create)* Add --below placement ([#333](https://github.com/cesarferreira/stax/issues/333))

### 🐛 Bug Fixes

- *(merge-when-ready)* Preserve remaining stack chain on rebase ([#311](https://github.com/cesarferreira/stax/issues/311)) ([#318](https://github.com/cesarferreira/stax/issues/318))

### 💼 Other

- Fix redundant merge PR base retargets ([#329](https://github.com/cesarferreira/stax/issues/329))

### 🧪 Testing

- *(refresh)* Cover auto-stash-pop linked worktree flow ([#326](https://github.com/cesarferreira/stax/issues/326))
- *(staging)* Automate interactive menu paths ([#328](https://github.com/cesarferreira/stax/issues/328))

### ⚙️ Miscellaneous Tasks

- Stop tracking Python bytecode from release prep ([#321](https://github.com/cesarferreira/stax/issues/321))
## [0.56.0] - 2026-04-21

### 🐛 Bug Fixes

- *(merge)* Push remaining branches before retargeting PR base ([#312](https://github.com/cesarferreira/stax/issues/312)) ([#317](https://github.com/cesarferreira/stax/issues/317))
- Fix release script

### 💼 Other

- Cesar/rewrite readme ([#313](https://github.com/cesarferreira/stax/issues/313))
- Add refresh command for sync/restack/submit flow ([#314](https://github.com/cesarferreira/stax/issues/314))
- [codex] Add verbose refresh/restack timing diagnostics ([#320](https://github.com/cesarferreira/stax/issues/320))

### 📚 Documentation

- Rewrite all user-facing docs with consistent, tighter structure ([#319](https://github.com/cesarferreira/stax/issues/319))
## [0.55.0] - 2026-04-20

### 🚀 Features

- *(modify,create)* Graphite-style menu when no files staged ([#310](https://github.com/cesarferreira/stax/issues/310))

### 🐛 Bug Fixes

- *(submit)* Surface git fetch errors instead of silent --force-with-lease against stale refs ([#307](https://github.com/cesarferreira/stax/issues/307))
## [0.54.0] - 2026-04-20

### 🐛 Bug Fixes

- *(tui)* Sort move picker candidates to match CLI picker ([#304](https://github.com/cesarferreira/stax/issues/304))
- *(merge)* Surface GitHub API details and wait for push to propagate ([#305](https://github.com/cesarferreira/stax/issues/305))

### 💼 Other

- Add install-aware st cli upgrade ([#301](https://github.com/cesarferreira/stax/issues/301))
- Added average to CI
- Improving CI
- Tiny cleanup
## [0.53.0] - 2026-04-17

### 🚀 Features

- *(tui)* Render move picker candidates with tree connectors ([#298](https://github.com/cesarferreira/stax/issues/298))
## [0.52.0] - 2026-04-16

### 💼 Other

- Improve st setup onboarding flow ([#296](https://github.com/cesarferreira/stax/issues/296))
## [0.51.0] - 2026-04-16

### 🚀 Features

- Auto-enable git rerere and rename shell-setup to setup
- Add atomic pushes to prevent partial push failures
- Enable checkout by PR number with --pr flag
- Auto-rebase children when parent is squash-merged
- Add standardized exit codes infrastructure
- Show live CI progress in TUI
- Add `st move`/`mv` alias and TUI move picker (gt move parity) ([#295](https://github.com/cesarferreira/stax/issues/295))

### 🐛 Bug Fixes

- Make st setup install by default
- Update shell_setup tests for new install-by-default behavior
- Run TUI CI loader inside tokio runtime ([#290](https://github.com/cesarferreira/stax/issues/290))
- *(create)* Run commit before creating branch so interrupts leave no orphan ([#292](https://github.com/cesarferreira/stax/issues/292))

### 💼 Other

- Initial plan
- Skills update command

### 🚜 Refactor

- Remove shell-setup alias, use 'st setup' only

### 📚 Documentation

- Add exit code documentation and usage guide
- Remove deprecated st submit --force from command reference
## [0.50.2] - 2026-04-14

### 🐛 Bug Fixes

- *(split)* Hard-fail `--file` on multi-commit branches ([#281](https://github.com/cesarferreira/stax/issues/281))

### 🚜 Refactor

- Model rebase safety as command policy

### ⚙️ Miscellaneous Tasks

- *(split)* Address self-review -- fail faster, reuse test helpers
## [0.50.0] - 2026-04-14

### 🐛 Bug Fixes

- Conflict handling exit code, rebase guard, resume loop, and auth error messages

### 💼 Other

- Updated changelog
## [0.49.0] - 2026-04-12

### 🚀 Features

- *(makefile)* Add parameterized release target for cargo-release
- *(release)* Add CHANGELOG.md with automated updates
- *(edit)* Add `st edit` command for interactive commit editing
- *(doctor)* Add checks for diverged trunk, git config, stale PR metadata
- *(restack)* Show conflict position indicator in stack
- *(ux)* Add post-operation next-step hints
- *(submit)* Add --squash flag and document roborev integration
- *(create)* Add --insert flag for mid-stack branch insertion
- *(absorb)* Add `st absorb` command for automatic change distribution
- *(upstack)* Add `st upstack onto` for mass reparent with descendants
- *(tui)* Add ConfirmForceDelete mode and RemovalUpdate enum
- *(tui)* Add removal operation state fields to WorktreeApp
- *(tui)* Implement removal operation methods
- *(tui)* Implement removal progress handling
- *(tui)* Update UI for two-stage confirmation and removal progress
- *(tui)* Add force delete key handling
- *(submit)* Add --publish/--draft toggle for existing PRs
- *(submit)* Auto-update PR title from commit message
- *(submit)* Gate PR title auto-update behind --update-title flag
- *(lane)* Add --yolo and --agent-arg flags to st lane and st wt create
- *(split)* Add --file flag for pathspec-based splitting

### 🐛 Bug Fixes

- *(changelog)* Update release dates with actual dates from git tags
- *(push)* Use --force-with-lease instead of -f for all force pushes
- *(sync)* Warn on metadata deletion failures instead of silently ignoring
- *(sync)* Only count metadata cleanup when it succeeds
- *(sync)* Add ancestor check before trunk hard-reset
- *(create)* Rollback branch when commit fails during `st create -m`
- *(create)* Rollback on metadata and git spawn failures
- *(doctor)* Count diverged trunk and stale metadata
- *(ux)* Surface config load failures in modify hints
- *(restack)* Hold auto-stashes until restack finishes
- *(merge-queue)* Rollback PR base on enqueue failure
- *(absorb)* Cover and clean up non-dry-run flow
- Persist reparent metadata only after successful restack
- *(submit)* Update PR titles even on no-op submits
- *(test)* Disambiguate PATCH requests in submit body-mode tests
- *(split)* Rollback file splits and cover the flow
- *(sync)* Reparent tracked children before delete-upstream-gone
- Fix release script
- Fix release script

### 💼 Other

- Updated docs and skills
- Add design spec for worktree removal UX improvements
- Add implementation plan for worktree removal UX improvements

### 🚜 Refactor

- *(push)* Clarify --force-with-lease semantics
- *(sync)* Clarify diverged trunk guidance
- *(edit)* Address review findings
- *(edit)* Clarify non-interactive limits and child restacks
- *(create)* Address review findings in rollback fix
- *(restack)* Use literal conflict markers
- *(restack)* Harden stash restoration and normalize paths consistently
- *(absorb)* Address review findings
- *(upstack-onto)* Fix rebase, guard edges, improve tests
- *(tui)* Remove PendingCommand::Remove variant
- *(submit)* Skip no-op draft toggles
- *(lane-yolo)* Address review -- opencode, tmux re-attach, cleanup
- *(split)* Deduplicate rollback with try_or_rollback macro
- *(sync)* Address review -- extract helper, skip doomed ancestors
- *(sync)* Finish dedup -- merged-branch path now uses shared helper

### 📚 Documentation

- Add st edit to command reference and compatibility matrix
- Add --insert flag to command reference and compatibility matrix
- Add st absorb to command reference and compatibility matrix
- *(agent-worktrees)* Add VS Code / Cursor integration recipe
- *(vscode)* Address review -- add post_go, drop post_create framing, caveats
- Add --publish/--draft flags to command reference
- Add st split --file to command reference and compatibility matrix

### 🧪 Testing

- *(create)* Cover trunk insert reparenting

### ⚙️ Miscellaneous Tasks

- Opt into Node.js 24 for GitHub Actions
- Format code and fix clippy warnings
## [0.46.0] - 2026-04-10

### 🚀 Features

- *(cli)* Add stack command group with sr and ss aliases ([#230](https://github.com/cesarferreira/stax/issues/230))
- *(merge)* Add --queue flag for GitHub merge queue and GitLab merge train support ([#236](https://github.com/cesarferreira/stax/issues/236))
- *(modify)* Add `--restack` flag to auto-restack after modify ([#237](https://github.com/cesarferreira/stax/issues/237))

### 🐛 Bug Fixes

- *(split)* Scrolling + patch application in hunk split TUI ([#234](https://github.com/cesarferreira/stax/issues/234))
- *(merge)* Retarget dependent PR after merge, not before ([#238](https://github.com/cesarferreira/stax/issues/238))
- *(pr,comments,copy)* Fall back to forge lookup when PR metadata is missing ([#239](https://github.com/cesarferreira/stax/issues/239))

### 📚 Documentation

- Collapse non-macOS install instructions in README
## [0.45.0] - 2026-04-07

### 🐛 Bug Fixes

- *(sync)* Ignore closed unmerged PRs during cleanup ([#207](https://github.com/cesarferreira/stax/issues/207))
- Use full markdown links for stack comments on GitLab and Gitea ([#226](https://github.com/cesarferreira/stax/issues/226))
- *(sync)* Force dirty linked worktree cleanup ([#227](https://github.com/cesarferreira/stax/issues/227))
- *(sync)* Honor dirty worktree confirmation ([#229](https://github.com/cesarferreira/stax/issues/229))
## [0.44.2] - 2026-04-07

### 🐛 Bug Fixes

- Handle tmux no-server state for lanes ([#223](https://github.com/cesarferreira/stax/issues/223))
- Don't exec switch-client inside tmux, preserving user's shell on detach ([#224](https://github.com/cesarferreira/stax/issues/224))
- Fix merge
## [0.44.1] - 2026-04-07

### 🚀 Features

- *(create)* Prompt to stage files when nothing is staged ([#211](https://github.com/cesarferreira/stax/issues/211))

### 🐛 Bug Fixes

- Generate --pr-body parity with submit for PR template selection ([#220](https://github.com/cesarferreira/stax/issues/220))
- Fix dirty check
- Fixed broken tests
## [0.44.0] - 2026-04-05

### 🚀 Features

- Per-feature AI agent+model config with first-use UX ([#215](https://github.com/cesarferreira/stax/issues/215))
## [0.43.0] - 2026-04-05

### 🐛 Bug Fixes

- *(ci)* Don't block releases on Windows test failures ([#217](https://github.com/cesarferreira/stax/issues/217))

### 💼 Other

- Add lane branch submit tests ([#218](https://github.com/cesarferreira/stax/issues/218))

### 📚 Documentation

- Add Linux arm64 prebuilt install instructions ([#216](https://github.com/cesarferreira/stax/issues/216))
- Expand st lane guide ([#214](https://github.com/cesarferreira/stax/issues/214))
## [0.42.2] - 2026-04-04

### 💼 Other

- Fix Windows release workflow by adding nextest installation fallback
## [0.42.1] - 2026-04-04

### 💼 Other

- Fix Windows build by increasing stack size to 8MB ([#209](https://github.com/cesarferreira/stax/issues/209))
- Document OpenSSL prerequisites for building from source
## [0.42.0] - 2026-04-04

### 🚀 Features

- Add st lane workflow for AI worktrees ([#203](https://github.com/cesarferreira/stax/issues/203))

### 🐛 Bug Fixes

- Enable vendored-openssl feature for CI cross-compilation ([#202](https://github.com/cesarferreira/stax/issues/202))

### 💼 Other

- Ignore configured model for worktree agent launches ([#204](https://github.com/cesarferreira/stax/issues/204))
- Sync can delete safe linked worktrees ([#205](https://github.com/cesarferreira/stax/issues/205))
- Fix st lane picker under shell integration ([#206](https://github.com/cesarferreira/stax/issues/206))
- Improve lane picker labels and colors
## [0.41.0] - 2026-04-03

### 💼 Other

- Add sync footer stats ([#196](https://github.com/cesarferreira/stax/issues/196))
- Improve worktree labels and dashboard contrast ([#197](https://github.com/cesarferreira/stax/issues/197))
- Colorized sync
- Locked bins debug
- Removed openssl
- Faster compilation ([#199](https://github.com/cesarferreira/stax/issues/199))
## [0.40.0] - 2026-04-02

### 🚀 Features

- Add --no-verify flag to stax modify ([#195](https://github.com/cesarferreira/stax/issues/195))

### 🐛 Bug Fixes

- Fix worktree routing ([#190](https://github.com/cesarferreira/stax/issues/190))
- Doctor command checks correct forge token instead of always GitHub ([#193](https://github.com/cesarferreira/stax/issues/193))

### 💼 Other

- Route checkout through shell integration for worktree cd ([#191](https://github.com/cesarferreira/stax/issues/191))
## [0.39.0] - 2026-04-01

### 💼 Other

- Fix binstall release packaging ([#185](https://github.com/cesarferreira/stax/issues/185))
- Reorganize worktree docs ([#186](https://github.com/cesarferreira/stax/issues/186))
- Fix zsh shell wrapper PATH lookup ([#187](https://github.com/cesarferreira/stax/issues/187))
- Limit shell integration to worktree flows ([#189](https://github.com/cesarferreira/stax/issues/189))
- Speed up worktree dashboard startup ([#188](https://github.com/cesarferreira/stax/issues/188))
## [0.38.1] - 2026-03-31

### 🐛 Bug Fixes

- Fix windows error
## [0.38.0] - 2026-03-31

### 🚀 Features

- *(split)* Add hunk-based branch splitting (`stax split --hunk`) ([#165](https://github.com/cesarferreira/stax/issues/165))
- *(remote)* Add explicit `forge` config to override auto-detection ([#176](https://github.com/cesarferreira/stax/issues/176))
- *(modify)* Respect staged files ([#171](https://github.com/cesarferreira/stax/issues/171))
- *(worktree)* Add cleanup command with dry-run ([#179](https://github.com/cesarferreira/stax/issues/179))
- *(forge)* Make `pr list` and `issue list` forge-aware ([#180](https://github.com/cesarferreira/stax/issues/180))
- *(forge)* Make `branch track --all-prs` forge-aware ([#181](https://github.com/cesarferreira/stax/issues/181))
- *(forge)* Make `standup` remote activity forge-aware ([#184](https://github.com/cesarferreira/stax/issues/184))

### 🐛 Bug Fixes

- *(pr-template)* Discover root-level PULL_REQUEST_TEMPLATE.md ([#158](https://github.com/cesarferreira/stax/issues/158))
- *(shell-setup)* Parse Fish shell output by newlines only ([#159](https://github.com/cesarferreira/stax/issues/159))
- Fix doctor auth: surface GitLab/Gitea alongside GitHub ([#162](https://github.com/cesarferreira/stax/issues/162))
- Fix open: warn when browser launcher fails ([#163](https://github.com/cesarferreira/stax/issues/163))
- *(github)* Over-fetch issues list to offset PR pollution ([#161](https://github.com/cesarferreira/stax/issues/161))
- *(remote)* Strip ssh:// port from host for HTTPS/API derivation ([#160](https://github.com/cesarferreira/stax/issues/160))
- *(split-hunk)* E2e tests, docs, and bug fixes for hunk splitting ([#174](https://github.com/cesarferreira/stax/issues/174))

### 💼 Other

- Fix worktree checkout routing in interactive selector
- Remove warnings
- Fix shell-setup drift from inline wrappers ([#178](https://github.com/cesarferreira/stax/issues/178))
- Fix saved token reuse for GitLab and Gitea ([#177](https://github.com/cesarferreira/stax/issues/177))
- Fix zsh shell wrapper lookup ([#182](https://github.com/cesarferreira/stax/issues/182))
- Refresh installed shell snippets after upgrades

### 📚 Documentation

- Surface standout README features ([#166](https://github.com/cesarferreira/stax/issues/166))

### ⚙️ Miscellaneous Tasks

- *(release)* Build aarch64-unknown-linux-gnu on ubuntu-24.04-arm ([#164](https://github.com/cesarferreira/stax/issues/164))
## [0.37.0] - 2026-03-26

### 🐛 Bug Fixes

- Fixed tui on large repos
## [0.36.0] - 2026-03-26

### 🚀 Features

- *(merge)* Add --remote for API-only stack merge on GitHub ([#153](https://github.com/cesarferreira/stax/issues/153))

### 🐛 Bug Fixes

- Guard stax modify on fresh branches ([#146](https://github.com/cesarferreira/stax/issues/146))
- Preflight dashboard input reader ([#148](https://github.com/cesarferreira/stax/issues/148))
- Make worktree cleanup hints runnable ([#156](https://github.com/cesarferreira/stax/issues/156))

### 💼 Other

- Faster rs ([#142](https://github.com/cesarferreira/stax/issues/142))
- Faster submit ([#143](https://github.com/cesarferreira/stax/issues/143))
- Route checkout to linked worktrees ([#155](https://github.com/cesarferreira/stax/issues/155))
- Refine the main TUI layout ([#154](https://github.com/cesarferreira/stax/issues/154))
- Fix worktree cleanup recovery flow ([#157](https://github.com/cesarferreira/stax/issues/157))

### 📚 Documentation

- Add benchmark speed comparisons ([#149](https://github.com/cesarferreira/stax/issues/149))

### ⚙️ Miscellaneous Tasks

- Simplify PR template ([#145](https://github.com/cesarferreira/stax/issues/145))
- Run Windows tests only on release ([#147](https://github.com/cesarferreira/stax/issues/147))
## [0.35.0] - 2026-03-25

### 🚀 Features

- Windows support ([#119](https://github.com/cesarferreira/stax/issues/119))
- Add GitLab forge implementation (2/3) ([#133](https://github.com/cesarferreira/stax/issues/133))
- Add Gitea forge implementation (3/3) ([#134](https://github.com/cesarferreira/stax/issues/134))

### 🐛 Bug Fixes

- Not being able to delete branch ([#126](https://github.com/cesarferreira/stax/issues/126))

### 💼 Other

- Faster sync ([#139](https://github.com/cesarferreira/stax/issues/139))

### 🚜 Refactor

- Introduce ForgeClient abstraction (1/3) ([#132](https://github.com/cesarferreira/stax/issues/132))
## [0.34.0] - 2026-03-23

### 🐛 Bug Fixes

- Fix reparent ([#136](https://github.com/cesarferreira/stax/issues/136))

### 💼 Other

- Bump node ci
- Set trunk
## [0.33.0] - 2026-03-22

### 🚀 Features

- Add restack --stop-here ([#124](https://github.com/cesarferreira/stax/issues/124))
- Add restack --stop-here

### 🐛 Bug Fixes

- Resume the rest of the stack from stax continue ([#123](https://github.com/cesarferreira/stax/issues/123))
- Restack causing slow cleanup of all repo merged branches ([#127](https://github.com/cesarferreira/stax/issues/127))
- Deduplicate check runs before evaluating CI status ([#128](https://github.com/cesarferreira/stax/issues/128))
- Harden log tree traversal against cycles ([#112](https://github.com/cesarferreira/stax/issues/112))
- Fix changelog alignment
- Sync --restack now restacks the entire stack, not just the first stale branch ([#118](https://github.com/cesarferreira/stax/issues/118))
- Prevent ghost commits when reparenting after squash-merge ([#120](https://github.com/cesarferreira/stax/issues/120))

### 💼 Other

- Show linked worktree branch removal hints
- Fix TUI input mode swallowing shortcut letters ([#43](https://github.com/cesarferreira/stax/issues/43))
- New ci settings ([#129](https://github.com/cesarferreira/stax/issues/129))
- Enhancements ([#130](https://github.com/cesarferreira/stax/issues/130))
- Bump libs ([#131](https://github.com/cesarferreira/stax/issues/131))
- Updated readme
- Updated readme
- Updated readme

### ⚡ Performance

- Cache remote branch refs during checkout/sync scans ([#121](https://github.com/cesarferreira/stax/issues/121))

### ⚙️ Miscellaneous Tasks

- Remove lint job from CI workflow
## [0.32.0] - 2026-03-18

### 🐛 Bug Fixes

- Fix

### 💼 Other

- Report sync checkout failures from linked worktrees
- Works?
## [0.31.0] - 2026-03-17

### 🐛 Bug Fixes

- Fix shell setup
- Fix shell setup ([#115](https://github.com/cesarferreira/stax/issues/115))

### 💼 Other

- Worktrees enhanced ([#114](https://github.com/cesarferreira/stax/issues/114))
- Fix shell integration instructions
- Pr list and issue list
## [0.30.0] - 2026-03-13

### 🚀 Features

- Auto-resolve changelog from latest tag with --tag-prefix support

### 💼 Other

- Improvements in --restack
- Updated readme
- Updated readme
- Add jit repo link to docs
## [0.29.8] - 2026-03-12

### 🚜 Refactor

- Remove dead refs_have_no_diff and harden test assertions
## [0.29.7] - 2026-03-12

### 🐛 Bug Fixes

- *(restack)* Detect squash-merged parents after trunk advances ([#110](https://github.com/cesarferreira/stax/issues/110))
## [0.29.6] - 2026-03-12

### 🐛 Bug Fixes

- Handle CI history ref updates safely ([#108](https://github.com/cesarferreira/stax/issues/108))
- Prevent checkout stack overflow (issue 106) ([#109](https://github.com/cesarferreira/stax/issues/109))
## [0.29.5] - 2026-03-12

### 💼 Other

- Fix TUI stack overflow on repos with many branches ([#105](https://github.com/cesarferreira/stax/issues/105))
## [0.29.4] - 2026-03-10

### 💼 Other

- Add --no-prompt to PR body generation
## [0.29.3] - 2026-03-10

### 💼 Other

- Improve restack conflict diagnostics ([#103](https://github.com/cesarferreira/stax/issues/103))
## [0.29.2] - 2026-03-10

### 🐛 Bug Fixes

- Prevent stax ls stack overflow ([#102](https://github.com/cesarferreira/stax/issues/102))
## [0.29.1] - 2026-03-09

### 🐛 Bug Fixes

- Fixed collision names
## [0.29.0] - 2026-03-09

### 🚀 Features

- *(init)* Add explicit trunk reconfiguration command ([#93](https://github.com/cesarferreira/stax/issues/93))
- *(submit)* Support configurable stack links placement ([#96](https://github.com/cesarferreira/stax/issues/96))

### 🐛 Bug Fixes

- *(metadata)* Tolerate missing parent fields in branch metadata ([#91](https://github.com/cesarferreira/stax/issues/91))
- *(create)* Do not auto-stage untracked files when using -m ([#89](https://github.com/cesarferreira/stax/issues/89))
- *(merge)* Preserve remaining stack chain after partial merge ([#90](https://github.com/cesarferreira/stax/issues/90))
- *(merge)* Retarget dependent PRs before parent merge ([#94](https://github.com/cesarferreira/stax/issues/94))
- *(merge)* Retarget dependent PRs before parent merge ([#95](https://github.com/cesarferreira/stax/issues/95))

### 💼 Other

- Reset --ai
- Add prebuilt binary install instructions to README ([#92](https://github.com/cesarferreira/stax/issues/92))

### 🧪 Testing

- *(docs)* Cover stack links config modes ([#97](https://github.com/cesarferreira/stax/issues/97))
## [0.27.0] - 2026-03-05

### 🚀 Features

- Stax worktree — developer worktree management + shell integration ([#88](https://github.com/cesarferreira/stax/issues/88))
## [0.26.0] - 2026-03-04

### 💼 Other

- New readme ([#83](https://github.com/cesarferreira/stax/issues/83))
- Run commands per stacked branch
## [0.25.1] - 2026-03-03

### 🐛 Bug Fixes

- Fix rebase
- Resolve pipe deadlock in patch-id provenance inference ([#82](https://github.com/cesarferreira/stax/issues/82))

### 💼 Other

- Jit summary
- Added resolve to solve git conflicts
- Simplification of code
- Delete upstream gone
## [0.25.0] - 2026-03-03

### 🚀 Features

- Use FuzzySelect for all branch selection prompts ([#78](https://github.com/cesarferreira/stax/issues/78))
- Add 8 new commands (abort, detach, reorder, demo, stack validate/fix/test, --rerequest-review) ([#79](https://github.com/cesarferreira/stax/issues/79))

### 💼 Other

- Agents ([#80](https://github.com/cesarferreira/stax/issues/80))
- Standup ai summary ([#81](https://github.com/cesarferreira/stax/issues/81))
- Bumped version
- Bumped version
## [0.22.2] - 2026-03-02

### 💼 Other

- Better example for multi stacks
- Timeouts on github clients
- Removed repetead prints
## [0.22.1] - 2026-03-02

### 💼 Other

- Restack prompts to submit stack ([#76](https://github.com/cesarferreira/stax/issues/76))
- Syncs post merge
## [0.22.0] - 2026-03-02

### 💼 Other

- Merge when ready is now mixed with merge
## [0.21.1] - 2026-02-27

### 💼 Other

- Draft is now default
## [0.21.0] - 2026-02-27

### 🐛 Bug Fixes

- Fix for merge when ready ([#74](https://github.com/cesarferreira/stax/issues/74))
- Reparent when orphan

### 💼 Other

- Updated readme
- Updated readme ([#68](https://github.com/cesarferreira/stax/issues/68))
- Updated readme ([#69](https://github.com/cesarferreira/stax/issues/69))
- Restack prediction ([#70](https://github.com/cesarferreira/stax/issues/70))
- Command ([#71](https://github.com/cesarferreira/stax/issues/71))
- Cesar/land/rename-command ([#72](https://github.com/cesarferreira/stax/issues/72))
- Faster tests ([#75](https://github.com/cesarferreira/stax/issues/75))
## [0.20.2] - 2026-02-26

### 🐛 Bug Fixes

- Fix bug

### 💼 Other

- Updated readme
- Proper fix for cache
## [0.20.1] - 2026-02-26

### 💼 Other

- Updated charts
- Progress displays in a background thread
- Tiny fix
## [0.20.0] - 2026-02-26

### 💼 Other

- Skip fetching ([#60](https://github.com/cesarferreira/stax/issues/60))
- Optimizations for fetch ([#61](https://github.com/cesarferreira/stax/issues/61))
- Updated readme
- New CI design
- Clearer way to see CI failed/succeeded
- Putting emoji back on
- Looks even better
- Updated readme
- New text
- Repo sync with progress ([#62](https://github.com/cesarferreira/stax/issues/62))
## [0.19.0] - 2026-02-24

### 💼 Other

- Add provenance-aware --onto restack for merged middle branches ([#56](https://github.com/cesarferreira/stax/issues/56))
- Document provenance-aware restack behavior ([#57](https://github.com/cesarferreira/stax/issues/57))
- Capture provenance boundary lesson for sync reparenting ([#58](https://github.com/cesarferreira/stax/issues/58))
- --open as an option
## [0.18.0] - 2026-02-23

### 💼 Other

- Fix restack cascading and auto-stash behavior. ([#50](https://github.com/cesarferreira/stax/issues/50))
## [0.17.0] - 2026-02-23

### 💼 Other

- Add OpenCode support for AI PR generation ([#55](https://github.com/cesarferreira/stax/issues/55))
## [0.16.0] - 2026-02-20

### 💼 Other

- Gh auth support ([#49](https://github.com/cesarferreira/stax/issues/49))
- Dedicated docs ([#52](https://github.com/cesarferreira/stax/issues/52))
- Dark mode
- Adding support for gemini cli ([#53](https://github.com/cesarferreira/stax/issues/53))
## [0.15.0] - 2026-02-18

### 💼 Other

- Add --no-prs flag to cascade command ([#48](https://github.com/cesarferreira/stax/issues/48))
- Make restack and sync worktree-safe ([#40](https://github.com/cesarferreira/stax/issues/40))
## [0.14.0] - 2026-02-13

### 🚀 Features

- Add AI-powered PR body generation ([#46](https://github.com/cesarferreira/stax/issues/46))

### 💼 Other

- Improving TUI ([#47](https://github.com/cesarferreira/stax/issues/47))
- Make sure merged state is correct
- Make sure merged state is correct
## [0.13.0] - 2026-02-12

### 🚀 Features

- Configurable branch name format template and st alias ([#44](https://github.com/cesarferreira/stax/issues/44))
## [0.12.2] - 2026-02-11

### 🐛 Bug Fixes

- *(sync)* Restack only current stack during sync
## [0.12.1] - 2026-02-11

### 💼 Other

- Freephite-parity ([#42](https://github.com/cesarferreira/stax/issues/42))
## [0.11.0] - 2026-02-09

### 🚀 Features

- Add cascade command to restack and submit stack"

### 🐛 Bug Fixes

- Fix delete branch

### 💼 Other

- Improving submit speed ([#35](https://github.com/cesarferreira/stax/issues/35))
- Added justfile and fixed tests
- Simplify checkout picker and prevent line wrap ([#37](https://github.com/cesarferreira/stax/issues/37))
- Checkout pulls up
- Add stax branch untrack command
## [0.10.6] - 2026-01-27

### 💼 Other

- Changelog ([#34](https://github.com/cesarferreira/stax/issues/34))
- New screenshot
## [0.10.4] - 2026-01-21

### ⚙️ Miscellaneous Tasks

- Ci watch
## [0.10.3] - 2026-01-21

### 💼 Other

- Revamped ls
## [0.10.2] - 2026-01-21

### 🐛 Bug Fixes

- Remove unreliable GitHub API head filter for finding PRs
## [0.10.1] - 2026-01-20

### 🐛 Bug Fixes

- Fix submit stack
## [0.10.0] - 2026-01-19

### 🐛 Bug Fixes

- Use correct head parameter format for finding PRs in same repository

### 💼 Other

- Improve CI command ([#33](https://github.com/cesarferreira/stax/issues/33))
## [0.9.2] - 2026-01-15

### 🐛 Bug Fixes

- Fix the order of fetching trunk
## [0.9.1] - 2026-01-15

### 💼 Other

- Shorter ls ([#31](https://github.com/cesarferreira/stax/issues/31))
## [0.9.0] - 2026-01-15

### 🚀 Features

- Add PR template discovery with multi-template support
- Add interactive template selection with fuzzy search
- Add --template, --no-template, --edit flags to submit
- Integrate template selector into submit flow per-branch

### 🐛 Bug Fixes

- Warn user when specified template name not found
- Add missing test and update template discovery test to match spec
- Fixed tests

### 💼 Other

- Inverted behind ahead
- Emoji for cloud
- New default for ls

### 🚜 Refactor

- Add error context to PR template discovery

### 📚 Documentation

- Document PR template selection feature

### 🧪 Testing

- Add integration tests for PR template selection

### ⚙️ Miscellaneous Tasks

- Format code with cargo fmt
## [0.8.1] - 2026-01-12

### 💼 Other

- Add stax skills.md for Claude Code integration
- Update skills installation path to ~/.claude/skills/
## [0.8.0] - 2026-01-12

### 💼 Other

- Add stax branch track --all-prs to import open PRs
## [0.7.0] - 2026-01-08

### 🚀 Features

- Feature/split-pr ([#24](https://github.com/cesarferreira/stax/issues/24))

### 🐛 Bug Fixes

- Fix

### 💼 Other

- Added a create wizard ([#25](https://github.com/cesarferreira/stax/issues/25))
- Still compiling dependencies. The code implementation is complete. Let me summarize what I've done:
- Add tests for stax ci command
- Great improvement
- Add stax copy command for clipboard support ([#29](https://github.com/cesarferreira/stax/issues/29))
- Add stax open command to open repo in browser
- Add stax standup command ([#27](https://github.com/cesarferreira/stax/issues/27))
## [0.6.0] - 2026-01-06

### 💼 Other

- Github client mock ([#23](https://github.com/cesarferreira/stax/issues/23))
- Added command to go to previous branch

### 🧪 Testing

- Test coverage ([#22](https://github.com/cesarferreira/stax/issues/22))
## [0.5.5] - 2026-01-04

### 💼 Other

- Added tests to the TUI
- Fix edge cases: circular deps, detached HEAD, fold recovery, add --yes flags
## [0.5.4] - 2026-01-04

### 💼 Other

- Check for certain types of install
## [0.5.3] - 2026-01-04

### 💼 Other

- Added update checker
## [0.5.2] - 2026-01-04

### 💼 Other

- Added tests to auth
## [0.5.1] - 2026-01-02

### 💼 Other

- Updated readme
- Updated readme
- Fetch doesnt fail to ss
## [0.5.0] - 2026-01-01

### 🐛 Bug Fixes

- Fix sync
- Fix warnings
- Fix squaring ([#21](https://github.com/cesarferreira/stax/issues/21))

### 💼 Other

- Ui tips
- Ui tips
- Updated screenshot
- Updated readme
- Update ls benchmark visualization in README
- Remove non github support
- Rewire
- Reparent orphan prs
- Block github client
- Merge stacks ([#19](https://github.com/cesarferreira/stax/issues/19))
- Leaner CI
- Improved tests ([#20](https://github.com/cesarferreira/stax/issues/20))

### ⚙️ Miscellaneous Tasks

- Chore/fix warnings
## [0.4.0] - 2025-12-29

### 🚀 Features

- Feature/colored branch names
- Feature/cloud alignment

### 🐛 Bug Fixes

- Fixed diff preview
- Fixed test
- Fix rs for ambiguous argument errors

### 💼 Other

- Updated readme with stats
- Added 'll'
- Without colors
- Improve branch checkout
- Submit output improv
- Submit all elements of the stack on submit
- Ls lists where we are
- Added more tests
- Update readme
- Update readme
- Proper color fix
- Handle empty repo
- Better branching error handling
- Undo
- Fold fix-upstack into feature/undo
- Bugfix/upstack-restack
- Submit stack comments on all PRs now
- Push harder
- Repo sync deletes merged PRs
- Ls cleanup
- Now when stax rs deletes a merged branch, it will:
## [0.3.0] - 2025-12-27

### 🐛 Bug Fixes

- Fix tracking
- Fixing bco
- Fix for icon in remote branches
- Fix release
- Fixed pipeline

### 💼 Other

- Initial commit: gt MVP
- Add GitHub integration and freephite-compatible CLI
- Rename to stax
- Fix remaining gt references and improve branch track UX
- Fix help output order, add tests and Makefile
- Add fuzzy finder to checkout showing all branches
- Add fp-style tree hierarchy to status and checkout
- Fix current branch indicator for trunk position
- Add merged branch cleanup to restack command
- Add Graphite-style config for branch naming
- Separate credentials from config for dotfiles safety
- Fix token priority and add config tests
- Speed up CLI tests 7x by using pre-built binary
- Add first-run initialization like Freephite
- Add detailed log view like fp l
- Show ALL stacks, not just current branch's stack
- Improve tree display and add PR stack comments
- Fix tree alignment using ASCII characters
- Fix tree alignment with proper pipe tracking
- Simplify tree display with consistent depth-based indentation
- Fix tree display with proper lines and alignment
- Sort children alphabetically for consistent tree ordering
- Add tree alignment tests and verify correct indentation
- Implement fp-style tree display for status/log/checkout
- Show │ for current stack only, matching fp behavior
- Fix │ line visibility and add ─┘ trunk connector
- Match fp tree format exactly
- Add colors to branch checkout menu
- Remove colors from checkout menu (incompatible with dialoguer)
- Add bu (branch up) and bdown (branch down) navigation
- Change bd shortcut from branch delete to branch down
- Update README with bu/bd navigation commands
- Improve error messages for missing/invalid git remote
- Add repo sync (rs) command and branch create -m flag
- Fix test for rs alias (now sync instead of restack)
- Fix config path to use ~/.config/stax on all platforms
- Add config command to show config path and contents
- Update README with styled header and badges
- Improve submit command with fp-style UX
- Fix PrInfo deserialization for legacy metadata
- Add fetch before submit and verify base branch exists
- Improve submit UX: collect PR details before pushing
- Update stack comment format to match freephite style
- Improve status display with colored tree like freephite
- Simplify status display with clean indentation
- Add remote icon, commit count, and improved badges to status
- Fix stack alignment with proper column-based layout
- Bump versions
- Finally working
- Looks gorgeous now
- Looks nice
- Nested layers
- Aligned branch text
- Updated readme and sync
- Moved up/down to the branch subfeature
- Override prefix
- Showing trunk when only trunk is available
- Proper indenting
- Git code change between branches in ls
- Updated readme
- Updated readme
- Better status display
- Bc now commits the changes after creating the branch
- Prints better
- Up and down arrows for commits
- Correct icon
- Added trunk command
- Show difference in trunk
- Stax modify
- Added integration tests
- Added more integration tests
- Added hook
- Updated integration tests
- Updated readme
- Create command
- Added tests for feature parity with fp
- Optimizations
- Better performance by using caches
- Interactive-checkout
- Improved bco
- Tags publish
- Bumped version
- Improved test accuracy
- Workflow trigger
- Added locked openssl for cross compilation
- Bumped octocrab
- Clearer message when deleting merged branches
- Updated comments on PRs
- Rename branch command
- New-readme
- Re-added screenshot
- Updated readme
- Added tests to rename
- Tui
- Reorder panes
- Edit works properly
- Details view
- Updated SS
- Remote comparisons
- Stack reorder
- Reordering preview
- Updated readme
- Updated readme
- Added screenshots

### 📚 Documentation

- Documented new features
