<!-- stax-skills-version: 0.98.0 -->
# Stax Skills for AI Coding Agents

This document teaches AI coding agents (Claude Code, Codex, Cursor, Gemini CLI, OpenCode) how to use `stax` to manage stacked Git branches and PRs.

> Installing this skill: run `stax skills update` (or `st setup --install-skills`). Per-agent setup details live in `docs/integrations/`.

## What is Stax?

Stax manages stacked branches: small focused branches layered on top of each other. Each branch maps to one PR targeting its parent branch.

## Core Concepts

- **Stack**: A chain of branches where each branch builds on its parent
- **Trunk**: The main branch (`main` or `master`)
- **Parent**: The branch a stacked branch is based on
- **Tracked branch**: A branch with stax metadata (parent and PR linkage)

## Command Map

```bash
stax status|ls                # Stack status (tree)
stax ll                        # Stack status with PR URLs/details
stax log|l                     # Stack status with commits + PR info
stax gui [path]                # Launch fresh native macOS GUI preview for one repository

stax submit|ss                 # Submit full stack
stax stack link                # Register current PR stack as native GitHub Stack (GitHub + gh-stack)
stax stack unlink <stack-number> # Unstack a native GitHub Stack remotely; omit number for active local tracking
stax merge                     # Merge PRs from stack bottom upward
stax sync|rs                   # Sync trunk + clean merged branches
stax sweep                     # Classify + optionally delete merged/gone/stale branches
stax restack                   # Rebase branch/stack onto parents
stax cascade                   # Restack bottom-up and submit updates

stax get [branch|PR]           # Sync current stack, or fetch/checkout a remote branch or PR stack
stax checkout|co|bco           # Checkout branch (interactive by default)
stax trunk|t                   # Checkout trunk
stax trunk <branch>            # Set trunk branch
stax up|u [n]                  # Move to child branch
stax down|d [n]                # Move to parent branch
stax top                       # Move to stack tip
stax bottom                    # Move to first branch above trunk
stax prev|p                    # Checkout previous branch

stax branch ...|b              # Branch subcommands
stax upstack ...|us            # Descendant-scope commands
stax downstack ...|ds          # Ancestor-scope commands

stax create|c|add              # Create stacked branch (--ai can name it from changes)
stax modify|m                  # Amend current commit (menu when nothing staged)
stax rename                    # Rename current branch
stax detach                    # Remove branch from stack, reparent children
stax reorder                   # Interactive stack reorder
stax split                     # Interactive branch split into stack

stax continue|cont             # Continue after conflict resolution
stax abort                     # Abort in-progress rebase/conflict flow
stax undo [op-id]              # Undo last/specific operation
stax redo [op-id]              # Redo last/specific undone operation

stax pr                        # Open current branch PR
stax pr body                   # Print current PR description
stax pr body --edit            # Edit current PR description in $EDITOR
stax ready                     # Interactive PR readiness dashboard for all tracked PRs, newest changed first (merge/ping/fix/wait/draft)
stax ready --current           # Readiness dashboard for current stack only
stax ready --stack             # Same as --current
stax ready --plain             # Static readiness table for captured/non-interactive output
stax pr list --ready           # Same readiness view under PR list
stax draft [branch]            # Mark current or named branch PR as draft
stax draft --stack             # Mark every PR in the current stack as draft
stax undraft [branch]          # Mark current or named branch PR ready for review
stax undraft --stack           # Mark every PR in the current stack ready for review
stax ready --all               # Explicit all tracked branch PRs (default)
stax issue list                # List open issues
stax open                      # Open repo in browser
stax comments                  # Show current PR comments
stax reviews --stack           # Review/comment inbox; GitHub review comments include inline file/line locations
stax reviews --all --json      # Machine-readable inbox for every tracked PR
stax copy [--pr]               # Copy branch name or PR URL
stax ci [--oneline|-1]         # CI status (per-check table; --oneline / multi-branch = one line per branch)
stax standup                   # Recent activity summary
stax standup --ai              # AI-generated spoken standup update (colored card)
stax standup --ai --style slack  # AI-generated Slack-ready Yesterday/Today bullets
stax standup --ai --jit   # AI standup plus Jira next-up context via jit (github.com/cesarferreira/jit)
stax changelog <from> [to]     # Changelog between refs
stax changelog find [query]    # Fuzzy-find commits in the changelog range
stax changelog --find [query]  # Flag form of commit fuzzy-find
stax generate                  # Interactive picker: PR body, PR title, or commit message (AI)
stax gen --pr-body             # Non-interactive: refresh open PR body from diff
stax gen --pr-title            # Non-interactive: refresh open PR title from diff
stax gen --commit-msg          # Non-interactive: amend HEAD commit message from diff

stax auth [status]             # GitHub auth setup/status
stax config                    # Print config path + contents
stax cli upgrade               # Detect the install method and run the matching upgrade flow
stax doctor                    # Health checks (also reports stale skill files)
stax doctor --fix              # Show one repair plan, then apply safe local fixes after confirmation
stax validate                  # Validate stack metadata
stax fix                       # Auto-repair metadata
stax test <cmd...>             # Run command on each branch
stax demo                      # Interactive tutorial

stax skills                    # List installed AI agent skill files + version status
stax skills list               # Same as above
stax skills update             # Download latest skills from GitHub and update all installed files
stax skills update --dry-run   # Preview what would be updated without writing

stax lane [name] [prompt]      # Open interactive lane picker, or start/resume named AI lane
stax absorb                    # Absorb staged changes into correct stack branches
stax edit|e                    # Interactively edit commits (reword, squash, fixup, drop)

stax worktree create [branch]  # Create a worktree for an existing local/fetched remote/new branch
stax worktree list             # List all worktrees (* = current)
stax worktree ll               # Richer worktree status (managed/prunable/conflict state)
stax worktree go <name>        # Navigate to a worktree (requires shell integration)
stax worktree path <name>      # Print absolute path of a worktree (for scripting)
stax worktree remove <name>    # Remove a worktree
stax worktree promote          # Retire current lane + check its branch out in main worktree
stax worktree cleanup          # Prune stale bookkeeping + bulk-remove merged/detached worktrees
stax worktree restack          # Restack all stax-managed worktrees
stax setup                     # Install shell integration, then optionally offer AI agent skills + auth onboarding
stax setup --yes               # Accept shell setup defaults, install skills, and import auth from gh when available
stax setup --install-skills    # Install shell integration and accept the skills install automatically
stax setup --skip-skills       # Install shell integration without the skills prompt
stax setup --auth-from-gh      # Install shell integration and import GitHub auth from gh without prompting
stax setup --skip-auth         # Install shell integration without the auth onboarding step
stax setup --print             # Print shell integration snippet for manual install

# Worktree shortcuts
stax wt                        # Open worktree dashboard (TTY) or print worktree help
stax w                         # List worktrees
stax wtc [branch]              # Create worktree (local branch, fetched remote branch, or new branch)
stax wtls                      # List worktrees
stax wtll                      # Long worktree list
stax wtgo <name>               # Navigate to worktree path
stax wtrm <name>               # Remove worktree
stax wtrs                      # Restack all stax-managed worktrees
sw <name>                      # Quick-switch (shell alias installed by stax setup)
```

## High-Value Commands and Flags

### Contributor Release Workflow

```bash
make release                     # Run cargo release (minor); git-cliff regenerates CHANGELOG.md inside the release commit
make release LEVEL=patch         # Same flow with a patch bump
make release LEVEL=major         # Same flow with a major bump
cargo release patch --no-confirm # Dry-run cargo release only (no bump/tag/push)
```

Release prep regenerates `CHANGELOG.md` with [git-cliff](https://git-cliff.org/) (config in `cliff.toml`) inside `cargo release`'s pre-release hook, grouping the commits since the latest `v*` tag under the new version. Conventional prefixes map to grouped sections (`feat` → Features, `fix` → Bug Fixes, `docs` → Documentation, etc.); non-conventional subjects land in `Other` rather than being dropped. git-cliff must be installed locally (`cargo install git-cliff`).

### Native macOS GUI

```bash
make gui-app                     # Build target/gui-app/Stax.app
make install-gui-app             # Install $HOME/Applications/Stax.app
make gui-release                 # Package the native architecture release zip
make gui-release-test            # Validate ad-hoc/signed/notarized configuration + package
stax gui                         # Launch GUI for the current directory
stax gui /path/to/repo           # Launch GUI for an explicit repository
```

Public GitHub Releases include `Stax-aarch64-apple-darwin.zip` and `Stax-x86_64-apple-darwin.zip`; extract the matching archive and move `Stax.app` to `/Applications`, or run `curl -fsSL https://cesarferreira.com/stax/install-gui.sh | sh` on macOS. The app is a separate artifact, not a new package, so it does not enlarge the CLI binaries. Ad-hoc-signed builds must be opened once to trigger Gatekeeper, then approved with **Privacy & Security → Open Anyway**; never disable Gatekeeper globally. The final bundle id is `com.cesarferreira.stax`. The GUI restores its most recent window size and clamps it to the active display after monitor changes.

`stax gui [path]` is macOS-only. It canonicalizes the supplied path, defaults to the current directory, and launches exactly `open -n -b com.cesarferreira.stax --args <canonical-path>`. The `-n` fresh-instance behavior is intentional: every invocation opens a new app process/window for one repository.

GUI operations are typed and repository-scoped. It can search branches; checkout, create, rename, delete, move, and reorder eligible local branches; restack selected/all; submit the current stack as Draft; Open PR without checkout; and undo/redo receipts whose transaction is fully local. Rename does not push. Delete shows descendants. Move/reorder/restack dirty-worktree recovery requires an explicit auto-stash confirmation. `/` focuses search, `1`/`2`/`3` toggle panes, draggable widths and visibility persist per canonical repository, and all enabled visible actions are keyboard-operable with visible focus and textual labels. GUI submit has no CLI prompts and does not auto-open PR pages. Keep AI naming, staging/commit creation, `--below`, `--insert`, custom prefixes, advanced submit options, and remote-effect recovery in CLI workflows.

### Create and Edit Branches

```bash
stax create <name>                 # Create branch stacked on current
stax add <name>                    # Alias for create
stax create -m "message"           # Use commit message (TTY menu if nothing staged)
stax create -a                     # Stage all before creating
stax create -am "message"          # Stage all + commit (bypasses menu)
stax create --ai                   # Generate a branch name from local changes
stax create --ai -a --yes          # Generate branch name + first commit message, stage all
stax create <name> --ai -a         # Keep branch name, generate first commit message
stax create --ai -m "message"      # Keep message, generate branch name
stax create -n -am "message"       # Stage all + commit, skipping hooks
stax create --from <branch>        # Create from explicit base
stax create --prefix feature/      # Override branch prefix
stax create <name> --below         # Insert below current; auto-stashes tracked/untracked work
stax create --below -am "message"  # Auto-stash/apply, stage all, commit on new lower branch
stax bc <name>                     # Hidden shortcut alias
# create -m/-am commits before branch creation, including --from/--below,
# so hook failures or interrupts do not leave orphan branches or -2 retries.
# -m/--ai derived branch names refuse collisions instead of creating -2 duplicates.
# --below keeps prepared work in place by stashing before moving downstack,
# then applying it on the inserted lower branch.

stax m                             # Amend current commit (TTY menu if nothing staged)
stax m -a                          # Stage all + amend (bypasses menu)
stax m -m "new msg"                # Amend with a new commit message

# When nothing is staged and a TTY is attached, `stax create -m` and
# `stax modify` show a menu: Stage all / Select --patch / Continue without
# staging (empty branch OR amend message only) / Abort. Non-TTY callers bail
# with guidance to use `-a` or `git add` first.

stax rename <name>                 # Rename current branch
stax rename --edit                 # Edit commit message while renaming
stax rename --push                 # Push renamed branch + cleanup remote

stax detach [branch] --yes         # Remove branch from stack, keep descendants
stax reorder --yes                 # Reorder stack interactively
stax split                         # Split current branch into multiple stacked branches
```

### Submit, Merge, Sync, Restack

```bash
stax submit                        # Submit full stack
stax ss                            # Alias for submit
stax submit --plan                 # Read-only action plan (no fetch/push/metadata writes)
stax submit --plan --json          # Versioned v2 plan for automation (action strings are extensible)
                                      # Live remote heads are read without fetching; chained restacks and unresolved PR/link decisions are runtime-evaluated
stax submit --draft                # Create draft PRs
stax submit --no-pr                # Push only (no PR create/update)
stax submit --no-fetch             # Skip git fetch
stax submit --no-verify            # Skip pre-push hooks while pushing
stax submit -n                     # Short for --no-verify
stax submit --open                 # Open current PR after submit
stax submit --reviewers a,b        # Set reviewers
stax submit --labels bug,urgent    # Set labels
stax submit --assignees alice      # Set assignees
stax submit --template backend     # Use named PR template
stax submit --no-template          # Skip template picker
stax submit --edit                 # Always edit PR body
stax submit --ai                   # Generate PR title/body with AI
stax submit --ai --title           # Generate/update PR title only
stax submit --ai --body            # Generate/update PR body only
stax submit --ai --yes             # Accept generated new-PR details
stax submit --rerequest-review     # Re-request existing reviewers on update
stax submit --native-stack         # Force-attempt native GitHub Stack registration for this run
stax submit --no-native-stack      # Skip native GitHub Stack registration for this run
stax completions zsh               # Generate completions: bash|zsh|fish|powershell|elvish

# ~/.config/stax/config.toml; repo-root stax.toml overlays shared values
[submit]
stack_links = "body"               # "comment" | "body" | "both" | "off"
single_stack = "on"                # "on" | "off" — when "off", skip stack-link sync while only one PR exists; populates on all PRs as soon as the stack reaches 2
native_stack = "auto"              # "auto" | "off" | "link" — auto-register native GitHub Stacked PRs when gh-stack + repo access are available
stack_links_when_native = "keep"   # "keep" | "off" — keep stax body/comment links when native registration succeeds

# Native GitHub Stacked PRs are additive. Repos/users without access or without
# `github/gh-stack` installed behave exactly as normal stax. `stax doctor --fix`
# can offer `gh extension install github/gh-stack` when `gh` is installed.
# `stax submit --native-stack` still keeps submit non-blocking, but prints an
# actionable note when `gh`, `github/gh-stack`, or `gh stack link` support is missing.
# gh-stack v0.0.8+ uses the public Stacks REST API and preserves normal GitHub
# CLI authentication, including GH_TOKEN/GITHUB_TOKEN. For known older versions,
# stax strips those overrides before `gh stack` and falls back to a keyring OAuth
# account. `stax doctor` always shows the installed version, marks anything below
# v0.0.8 as out of date, can upgrade it with `stax doctor --fix`, and probes
# legacy OAuth only when token overrides exist.
# Native GitHub Stack updates are append-only. If relinking would remove or insert
# a PR, run `stax stack unlink <stack-number>` and then `stax stack link` again.
# stax prints the repository-scoped Stack number when gh-stack returns it.
# Once linked, GitHub owns base-branch transitions for those PRs and rejects
# any PATCH touching `base` ("...part of a stack"). stax treats this as
# non-fatal in submit/merge cascade retargets (prints a note, continues);
# `stax merge --stack`/`--queue` fail with an actionable message instead,
# since merging out of stack order needs a real base change (run
# `stax stack unlink` first if that's what you want).
# GitHub's native Stack feature only supports one linear chain — if a branch
# in the local stack has two+ children (a fork), stax detects this itself
# and skips native `gh stack link` for that submit (prints a note) rather
# than handing gh-stack a branch set it might silently mis-linearize.
# stax's own body/comment stack links have no such limit and render forked
# siblings at equal depth.

stax branch submit                 # Submit current branch only
stax bs                            # Hidden shortcut alias for branch submit
stax upstack submit                # Submit current + descendants
stax downstack submit              # Submit ancestors + current

# submit can publish temporary rebased heads for branches that need restack;
# local branch tips and metadata are not moved. Scoped submit still requires an
# excluded parent to be remote-synced; otherwise use downstack/full submit or
# restack first.

stax merge --all                   # Merge whole stack
stax merge --downstack-only        # Merge ancestors below current, then rebase current
stax merge --ds                    # Alias for --downstack-only
stax merge --dry-run               # Preview merge plan only
stax merge --method squash         # squash|merge|rebase
stax merge --stack                 # GitHub only: validate selected tip once, merge it, and let lower PRs become merged when GitHub detects it
stax merge --stack --downstack-only # Stack-merge ancestors below current; keep current open
stax merge --stack --full          # Stack-merge full stack even from the middle
stax merge --stack --when-ready    # Wait only for selected tip PR readiness before stack fast-forward merge
stax merge --when-ready            # Wait for CI + approval before each merge
stax merge --remote                # Merge via GitHub API only — no local checkout/rebase/push
stax merge --remote --all          # Include full stack (GitHub only)
stax merge --interval 30           # Poll interval in seconds for --when-ready / --remote / --stack --when-ready
stax merge --no-wait               # Fail fast if CI is pending
stax merge --timeout 60            # Max wait minutes per PR
stax merge --no-delete             # Keep branches after merge
stax merge --no-sync               # Skip post-merge sync
stax merge-when-ready              # Backward-compatible alias

stax rs                            # Sync trunk + clean merged branches
stax rs --restack                  # Sync then restack
stax sync --continue               # Continue after resolved sync conflicts
stax sync --safe                   # Avoid hard reset on trunk update
stax sync --force                  # Force sync without prompts; preserve linked worktrees during cleanup
stax sync --prune                  # No-op: kept for CLI compatibility (use --full to fetch --prune all remote-tracking refs)
stax sync --full                   # Fetch all remote branches with --prune (slower; default is trunk-only fetch + ls-remote)
stax sync --no-delete              # Keep merged branches
stax sync --auto-stash-pop         # Stash/pop dirty target worktrees
# sync cleanup switches/detaches linked worktrees before deleting merged/gone branches; interactive removal remains explicit.
# Imported support branches may still be deleted locally after merge/gone, but their remotes are never push-deleted.
# The sync footer reports trunk commits/files/line changes plus non-zero cleanup/imported/restack counts.
# Conditional attention lines name blocked cleanup, trunk failures, and checkout changes, followed by one prioritized next command. For a diverged trunk, inspect and reconcile it with its remote instead of treating `st trunk` as a repair; other trunk failures use `st trunk`. Routine restack health stays in stax ls and the TUI.
# When --restack is requested, a failed fetch or trunk that did not reach the fetched remote commit stops sync before imported refresh, merged cleanup, or feature-branch rebases. Any sync auto-stash is restored first.

stax sweep                         # Classify ALL local branches (merged/gone/stale/active) — read-only
stax sweep --delete                # Delete merged/tracked-merged PRs + upstream-gone branches with no unique work after confirmation
stax sweep --delete --include-stale  # Also delete stale branches
stax sweep --delete --force        # Skip confirmation prompt
stax sweep --stale-days 60         # Override stale threshold in days (default 30, or branch.stale_days config)
stax sweep --json                  # Machine-readable branch classification (conflicts with --delete)

stax update                        # Sync trunk, restack, then submit (no merged cleanup)
stax update --no-pr                # Push only after trunk sync/restack
stax update --no-submit            # Trunk sync/restack only
stax update --force                # Force sync without prompts first
stax update --force --yes --no-prompt # Full update without sync/submit prompts
stax update --verbose              # Show detailed sync/restack/submit timings
# update inherits sync's fetch/trunk guard and exits before its submit phase, so it does not push or update PRs after that failure.

stax restack                       # Restack current branch onto parent
stax restack --all                 # Restack whole stack
stax restack --continue            # Continue after conflicts
stax restack --dry-run             # Predict conflicts only
stax restack --submit-after yes    # ask|yes|no
stax restack --auto-stash-pop      # Stash/pop dirty target worktrees
stax restack --quiet               # Also silences the preflight notice below

stax cascade                       # Restack bottom-up then submit
stax cascade --no-pr               # Push only, skip PR updates
stax cascade --no-submit           # Local restack only
stax cascade --auto-stash-pop      # Stash/pop dirty target worktrees
```

### Navigation and Scopes

```bash
stax co                            # Interactive branch picker
stax co <branch>                   # Checkout specific branch
stax checkout --trunk              # Jump to trunk
stax checkout --parent             # Jump to parent
stax checkout --child 1            # Jump to first child
stax t                             # Trunk alias
stax trunk main                    # Set trunk to 'main'
stax u 3                           # Move up 3 branches
stax d                             # Move down 1 branch
stax top                           # Tip of current stack
stax bottom                        # Base branch above trunk
stax p                             # Previous branch

stax get                           # Sync and restack current stack
stax get teammate-branch           # Fetch/sync remote branch, track under trunk, checkout
stax get 123                       # Fetch/sync the branch for PR #123
stax get teammate-branch --parent base-branch  # Track fetched branch under explicit parent
stax get teammate-branch --downstack  # Do not sync local upstack descendants
stax get teammate-branch --remote-upstack  # Include remote-only upstack PR branches when forge metadata is available
stax get teammate-branch --no-checkout  # Fetch and track without switching branches
# Existing local branches fast-forward or rebase local-only commits onto the fetched remote tip; use --force only to reset.
# New remote-only imports are read-only during submit. Existing Stax-managed branches keep ownership metadata. Branches checked out in another linked worktree are skipped.
# Imported PRs still get stack-link comments with relative intro text. GitHub comments keep compact native PR references and mark the rendered PR with 👈.
# sync --restack refreshes clean imported bases before rebasing descendants; cleanup can remove them locally after merge/gone.
stax branch track --parent main    # Track existing branch under parent
stax branch track --all-prs        # Import your open PRs
stax branch untrack <branch>       # Remove stax metadata only
stax branch reparent --parent new  # Change parent branch
stax branch delete <branch>        # Delete branch + metadata
stax branch squash -m "message"    # Squash all commits into one
stax branch fold --keep            # Fold into parent; optionally keep branch
stax branch up                     # Move to child (branch scope command)
stax branch down                   # Move to parent
stax branch top                    # Move to stack tip
stax branch bottom                 # Move to stack base

stax upstack restack               # Restack descendants
stax downstack get                 # Show branches below current
```

### Diagnostics, CI, Comments, and Reporting

```bash
stax ls                            # Fast stack tree
stax ll                            # Stack + PR URLs
stax log                           # Stack + commit details
stax diff                          # Diff each branch vs parent + aggregate stack diff
stax range-diff                    # Range-diff branches needing restack

stax pr body                       # Print current PR description
stax pr body --edit                # Edit current PR description in $EDITOR
stax ready                         # Interactive PR readiness dashboard for all tracked PRs, newest changed first; arrows move, Enter opens PR
stax ready --current               # Current-stack readiness dashboard
stax ready --plain                 # Fresh static readiness table: ACTION, PR, BRANCH, REVIEWS, CI, TITLE
stax ready --all                   # Readiness for all tracked branch PRs (default)
stax ready --json                  # Machine-readable readiness rows
stax pr list --ready               # Same readiness view under PR list
stax issue list --limit 50 --json  # List open issues with optional limit and JSON output
stax comments                      # Show current PR comments
stax comments --plain              # Raw markdown output
stax next / stax n                  # Next unmerged branch; deterministic on forks
stax freeze [branch]                # Protect branch from restacks and sync history rewrites (including imported refresh/squash cleanup)
stax unfreeze [branch]              # Remove freeze protection
stax run --parallel --jobs 4 <cmd>  # Concurrent checks; command receives STAX_RUN_BRANCH

stax ci                            # Live CI for current PR head, full per-check table (falls back to local revision when needed)
stax ci --stack                    # CI for current stack (defaults to the one-line-per-branch roll-up)
stax ci --all                      # CI for all tracked branches (one-line-per-branch roll-up)
stax ci --oneline                  # One compact line per branch across the stack (alias: -1)
stax ci --watch --interval 30      # Watch until all checks finish, custom poll interval
stax ci --watch --strict           # Watch but exit as soon as any check fails
stax ci --watch --alert            # Watch CI, play built-in success/error sounds
stax ci --watch --alert /path/to/sound.wav  # Use one custom sound for either outcome
stax ci --watch --no-alert         # Suppress configured completion sounds for one run
stax ci --refresh                  # Force refresh (bypass cache)
stax ci --json                     # Machine-readable output
stax ci --verbose                  # Compact summary cards (grouped failed/running/passed per branch)

# Oneline roll-up: status icon · branch · #PR · draft/ready · title · check-count + timing.
# Single branch shows the full per-check table; any multi-branch view defaults to oneline;
# --verbose forces the grouped cards. --oneline conflicts with --verbose.

# ~/.config/stax/config.toml
[ci]
alert = true                       # Play success/error sounds for stax ci --watch
success_alert_sound = "/path/to/ci-success.wav"  # optional, built-in when omitted
error_alert_sound = "/path/to/ci-error.wav"      # optional, built-in when omitted

stax standup --hours 48            # Summarize recent activity window
stax standup --all --json          # All stacks in JSON
stax standup --ai             # AI spoken standup — colored card, word-wrapped
stax standup --ai --style slack  # AI Slack-ready Yesterday/Today bullets
stax standup --ai --agent claude  # Override AI agent for one run
stax standup --ai --plain-text    # Raw text output (pipe-friendly)
stax standup --ai --json          # {"summary": "..."} JSON
stax standup --ai --jit           # Add Jira context via jit (github.com/cesarferreira/jit)

stax changelog v1.2.0 HEAD         # Changelog from ref to ref
stax changelog v1.2.0 --path src/  # Filter by path
stax changelog find                # Interactive fuzzy picker over commits in the changelog range
stax changelog find "auth fix"     # Search commit messages in the changelog range
stax changelog --find "auth fix"   # Flag form for scripts
stax changelog v1.2.0 --json       # JSON output

stax gen                           # Interactive AI picker (PR body / title / commit msg)
stax generate --pr-body            # Refresh PR body with AI (non-interactive)
stax gen --pr-title                # Refresh PR title with AI
stax gen --commit-msg              # Amend HEAD commit message with AI
stax generate --pr-body --edit     # Open editor before update
stax generate --pr-body --agent codex --model gpt-5
```

### AI Worktree Lanes (parallel AI agents)

```bash
stax lane                                         # Interactive lane picker (create or resume)
stax lane add-dark-mode "Add dark mode"           # Start a named lane with a prompt
stax lane add-dark-mode --agent codex             # Start a lane with a specific agent
stax lane add-dark-mode --agent codex --model gpt-5.5-fast  # Override model too
stax lane add-dark-mode                           # Re-enter the lane (reattaches tmux session)
stax lane add-dark-mode "new prompt" --no-tmux    # Force direct terminal (no tmux)

stax wt ll                                        # Rich status of all lanes
stax wt rs                                        # Restack ALL stax-managed worktrees after trunk moves
stax wt rm add-dark-mode --delete-branch          # Remove worktree + delete branch + metadata
stax wt rm add-dark-mode --force                  # Force remove dirty worktree
stax wt promote                                    # Continue current lane branch in main worktree
stax wt cleanup --dry-run                         # Preview bulk prune/remove decisions
stax wt cleanup                                   # Prune stale entries + remove merged/detached lanes

# Lower-level worktree control
stax wt c review-pass --agent codex -- "address the open PR comments"  # Create + launch agent
stax wt go review-pass --agent codex --tmux       # Re-enter + launch agent in existing lane

# Warm-start dependencies: by default, removing a clean, merged-equivalent
# worktree parks it as a reusable warm slot (reset --hard trunk + `git clean -fd`,
# which keeps gitignored deps like node_modules / .venv) instead of deleting it.
# The next create/lane adopts that slot instead of a cold `git worktree add`, so
# built deps survive. A --force dirty removal never parks.
#
# Optional ~/.config/stax/config.toml or repo-root stax.toml overrides:
[worktree]
reuse_slots = false               # disable recycling (cold create + real remove)
max_idle_slots = 4                # cap on parked idle slots
reconcile = "pnpm install"        # non-fatal deps re-sync on adopt
```

### Maintenance, Safety, and Setup

```bash
stax continue                      # Continue after resolving rebase conflicts
stax abort                         # Abort in-progress rebase/conflict flow

stax undo                          # Undo last risky operation
stax undo <op-id>                  # Undo a specific operation
stax undo --no-push                # Undo locally only
stax redo                          # Re-apply last undone operation
stax redo <op-id> --no-push        # Redo locally only

stax validate                      # Validate stack metadata health (read-only; never prunes refs)
stax fix --dry-run                 # Preview metadata repairs
stax fix --yes                     # Apply metadata repairs non-interactively

stax test --all --fail-fast -- make lint
stax test -- cargo test -p my-crate

stax auth --token <token>          # Save GitHub PAT
stax auth --from-gh                # Import from gh auth token
stax auth status                   # Show active auth source
stax config                        # Print config location + values
stax cli upgrade                   # Upgrade using the detected install method, then refresh shell setup
stax doctor                        # Repo/config health checks (also reports stale skill files)
stax doctor --fix                  # Confirm once to set recommended git config and update stale installed skills
stax demo                          # Interactive tutorial

stax skills                        # List installed AI agent skill files + version status
stax skills list                   # Same as above
stax skills update                 # Download latest skills from GitHub and update all installed files
stax skills update --dry-run       # Preview what would be updated without writing
```

## Common Workflows

### Start a New Feature Stack

```bash
stax t
stax rs
stax create api-layer
# ...changes...
stax m
stax create ui-layer
# ...changes...
stax m
stax ss
```

### Update Reviewed Branch and Re-request Review

```bash
stax co <branch>
# ...fixes...
stax m
stax ss --rerequest-review
```

### Merge with Safety Gates (CI + approvals)

```bash
stax ready
stax merge --when-ready --interval 15
stax merge --stack --when-ready    # GitHub stack fast-forward: selected tip CI only, defaults to rebase
```

### After Base PR Merges

```bash
stax update
```

### Resolve Rebase Conflicts

```bash
stax restack
# ...resolve conflicts...
git add -A
stax continue
```

If stax detects that the stored `parentBranchRevision` would replay much more
history than `merge-base(parent, branch)`, it prints a `preflight:` notice and
automatically uses the merge-base boundary for that rebase. This is the common
cause of “conflicts on files I never edited” after `git merge main` into a
branch or late tracking.

Silence the notice with `[restack] preflight_warn = false` or `--quiet`.
Disable the automatic correction with `[restack] preflight_auto_repair = false`
only when debugging old boundary behaviour.

### Repair Broken Metadata

```bash
stax validate
stax fix --dry-run
stax fix --yes
```

### Work on Multiple Stacks in Parallel (Developer Worktrees)

```bash
# One-time shell integration (enables transparent cd)
stax setup
stax setup --yes               # Shell integration + skills + auth import from gh when available
stax setup --install-skills    # Non-interactive onboarding: shell integration + AI agent skills

# Create a worktree for an existing local branch
stax worktree create feature/payments-api

# Create a local tracking branch and worktree from a fetched remote branch
stax worktree create origin/feature/payments-api

# List all worktrees
stax w

# Jump to a worktree
stax worktree go payments-api
# or with the shell alias:
sw payments-api

# All stax commands work normally inside worktrees
stax restack --all
stax ss

# Hand this branch back to the main worktree (both checkouts must be clean)
stax worktree promote

# Clean up
stax worktree remove payments-api
```

### Run Multiple AI Agents in Parallel

Each agent gets its own isolated worktree and branch. They cannot conflict.

```bash
# 1. Start one lane per task — stax creates the worktree, branch, and launches the agent
stax lane add-dark-mode --agent codex "Add dark mode"
stax lane fix-auth-refresh --agent claude "Fix auth refresh edge case"
stax lane write-integration-tests "Write integration tests for checkout flow"

# 2. Check status while agents run
stax wt ll           # rich status of all lanes (tmux state, dirty/clean, branch)
stax status          # all three branches appear in the normal stack tree

# 3. Reattach to a session later
stax lane            # interactive picker — fuzzy, shows tmux + status columns
stax lane fix-auth-refresh  # jump directly back to that lane

# 4. Trunk moved — restack everything at once
stax wt rs

# 5. Review and submit each branch normally
stax checkout add-dark-mode
stax submit

# 6. Clean up
stax wt rm add-dark-mode --delete-branch
stax wt cleanup      # bulk-remove merged/detached lanes
```

## Reading Stack Output

```
◉  feature/validation 1↑         # ◉ = current branch, 1↑ = commits ahead of parent
○  feature/auth 2↑ 1↓ ⟳          # ⟳ = needs restack
○  feature/old-base (missing parent: feature/base)
│ ○    ☁ feature/payments PR #42 # ☁ = has remote, PR #N = open PR
○─┘    ☁ main                    # trunk branch
```

Symbols:

- `◉` = current branch
- `○` = other branch
- `☁` = has remote tracking
- `↑` = commits ahead of parent
- `↓` = commits behind parent
- `⟳` = needs restacking (parent changed)
- `(missing parent: X)` = branch metadata points to a deleted/missing parent; run `stax fix --yes`
- `PR #N` = open PR

## Best Practices

1. Keep branches small and reviewable.
2. Sync often (`stax rs`).
3. Restack after merges (`stax rs --restack`); squash-merged local parents collapse to their updated parent before descendants rebase.
4. Prefer amend flow (`stax m`) to keep one commit per branch.
5. Validate and repair metadata (`stax validate`, `stax fix`) before deep stack surgery. Validation is read-only; only `fix` removes orphaned refs.
6. Check stack shape (`stax ls` / `stax ll`) before submit or merge.
7. Use `stax lane <name> [prompt]` to give each AI agent its own isolated worktree — prevents agents from conflicting on the same files.
8. After trunk moves, run `stax wt rs` once instead of rebasing each agent worktree manually.
9. Use `stax worktree create` when you want a worktree for an existing local branch, fetched remote branch, or human parallel development — `st lane` is the higher-level AI shortcut.
10. Use `stax worktree promote` inside a clean lane to retire it and continue its branch in the main worktree without losing stax or PR metadata.
11. Run `stax setup` once per machine to enable `stax worktree go`, `stax worktree promote`, and the `sw` alias to move the parent shell automatically.

## Tips

- Run `stax` with no args to launch the interactive TUI; selected-branch CI hydrates in the background, unchanged branch diffs can be reused from the repo-local TUI cache on reopen, and `1`/`2`/`3` toggle the Stack/Summary/Patch panes for small terminals. Pane visibility is remembered per repo.
- Run `stax gui [path]` on macOS to launch a fresh installed GUI window for one repository. A pathless app launch restores the last project; use the toolbar project dropdown to switch recent repositories or add another. Use `/` for search, `1`/`2`/`3` for persistent pane toggles, and the guarded controls, menus, or shortcuts for structural operations and local-only undo/redo.
- Use `stax --help` or `stax <command> --help` for exact flags.
- Add global `--trace` to profile instrumented Git subprocesses and total command time; use `make benchmark-status` for reproducible cold status scaling fixtures.
- Hidden convenience shortcuts: `stax bc`, `stax bu`, `stax bd`, `stax bs`, `stax w`, `stax wtc`, `stax wtgo`, `stax wtrm`.
- Use `--yes` for non-interactive scripting.
- Use `--json` on supported commands for machine-readable output.
- Use `stax lane` with no arguments for an interactive picker over all stax-managed lanes — useful when you forget where a session lives.
- Use `stax worktree go` (or `sw`) + shell integration to switch between stacks without `cd` gymnastics.
- Use `stax worktree promote` when a lane should become the main-worktree checkout; it refuses dirty or conflicted checkouts instead of stashing automatically. If Git reports a removal failure after already retiring the lane, Stax keeps the completed promotion and warns you to inspect leftover files.
- `stax worktree list` shows ALL worktrees including those created externally via `git worktree add`.
