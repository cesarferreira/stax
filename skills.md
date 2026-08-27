<!-- stax-skills-version: 0.107.0 -->
# Stax Skills for AI Coding Agents

This document teaches AI coding agents (Claude Code, Codex, Cursor, Gemini CLI, OpenCode, pi) how to use `stax` to manage stacked Git branches and PRs.

> Installing this skill: run `stax skills update` (or `st setup --install-skills`). To print the bundled skill (SKILL.md format): `st --skill`. Per-agent setup details live in `docs/integrations/`.

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
stax web [path] [--port <n>] [--no-open]  # Start localhost workspace; busy ports fall back automatically

stax submit|ss                 # Submit full stack
stax stack link                # Register current PR stack as native GitHub Stack (GitHub + gh-stack)
stax stack unlink <stack-number> # Unstack a native GitHub Stack remotely; omit number for active local tracking
stax merge                     # Merge PRs from stack bottom upward
stax sync|rs                   # Sync trunk + clean merged branches
stax sweep                     # Classify + optionally delete merged/gone/stale branches
stax restack                   # Rebase branch/stack onto parents
stax cascade                   # Restack bottom-up and submit updates (no trunk fetch; offline-friendly)

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
stax undo [op-id]              # Undo last/specific operation — covers restack, submit, sync (trunk ff + deletions + reparents + restack phase)
stax redo [op-id]              # Redo last/specific undone operation

stax pr                        # Open current branch PR
stax pr body                   # Print current PR description
stax pr body --edit            # Edit current PR description in $EDITOR
stax ready                     # Interactive unmerged-PR readiness TUI; refresh drops remotely merged PRs without local cleanup
stax ready --current           # PR readiness TUI scoped to current stack only
stax ready --stack             # Same as --current
stax ready --plain             # Static readiness table for captured/non-interactive output
stax pr list --ready           # Same interactive TUI under PR list
stax draft [branch]            # Mark current or named branch PR as draft
stax draft --stack             # Mark every PR in the current stack as draft
stax undraft [branch]          # Mark current or named branch PR ready for review
stax undraft --stack           # Mark every PR in the current stack ready for review
stax ready --all               # Explicit all tracked branches (default)
stax issue list                # List open issues
stax open                      # Open repo in browser
stax comments                  # Show current PR comments
stax reviews --stack           # Review/comment inbox; GitHub review comments include inline file/line locations
stax reviews --all --json      # Machine-readable inbox for every tracked PR
stax copy [--pr]               # Copy branch name or PR URL
stax ci [--oneline|-1]         # CI status (all GitHub status/check-run pages; --oneline / multi-branch = one line per branch)
stax standup                   # Recent activity summary
stax standup --ci              # Opt in to live CI for selected branches (adds network latency)
stax standup --ci --json       # Include CI plus signal availability metadata
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
stax update                    # Upgrade the CLI, then offer to refresh installed AI agent skill files
stax doctor                    # Health checks (also reports stale skill files)
stax doctor --fix              # Show one repair plan, then apply safe local fixes after confirmation
stax validate                  # Validate stack metadata
stax fix                       # Auto-repair metadata
stax test <cmd...>             # Run command on each branch
stax demo                      # Interactive tutorial

stax skills                    # List installed AI agent skill files + local package-version-marker status
stax skills list               # Same as above; does not verify fetched content
stax skills update             # Fetch, compare fully rendered files, and refresh changed instructions
stax skills update --dry-run   # Preview content-based updates without writing
st --skill                     # Print bundled agent skill (SKILL.md format) to stdout

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
stax setup --install-skills    # Install shell integration and skills for all harnesses (non-interactive)
stax setup --install-skills --skills claude,cursor   # Only selected harnesses
stax setup --yes               # Skills for detected harnesses only
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

### Web Workspace (`st web`)

`st web` starts a localhost HTMX workspace in the browser.

```bash
stax web                          # Start on 127.0.0.1:8787 and open browser
stax web --port 9000              # Custom port (falls back to a free OS port if busy)
stax web --port 0                 # Ephemeral port
stax web --no-open                # Print URL only; don't open browser
stax web /path/to/repo            # Open a specific repository
```

Key properties:
- GitKraken-inspired layout: grouped toolbar, stack graph table (topology + ahead/behind + PR chips), file-list + patch Changes panel, Details inspector, status bar
- Binds **127.0.0.1 only** — never reachable from the network; no `--host` flag
- Unguessable 48-hex session token in every URL: `/s/<token>/…`
- CSRF token required on all mutating POSTs; wrong token → 403
- `Host` must be local. Originless requests are allowed; if sending `Origin`, use exactly `http://127.0.0.1:<actual-bound-port>` from the printed URL. Cross-site, malformed, duplicate, and wrong-port Origins → 403; this does not replace the session token or CSRF requirement.
- One mutation at a time; mutating controls disabled while op is in flight
- Session state is in-memory; server restart generates a new URL

Supports checkout, create, rename, delete, restack, submit (draft), undo/redo, and move. Use `/` to search, `1`/`2`/`3` to toggle panes, `Esc` to dismiss overlays.



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
stax submit --fork                 # Fall back to fork on permission-denied push (single branch, GitHub only)
stax completions zsh               # Generate completions: bash|zsh|fish|powershell|elvish

# ~/.config/stax/config.toml; repo-root stax.toml overlays shared values
# stax --default-config  # print full annotated template (all sections + allowed values)
# st --skill             # print bundled AI agent skill (SKILL.md format)
[submit]
stack_links = "body"               # "comment" | "body" | "both" | "off"
single_stack = "on"                # "on" | "off" — when "off", skip stack-link sync while only one PR exists; populates on all PRs as soon as the stack reaches 2
native_stack = "auto"              # "auto" | "off" | "link" — gh-stack on submit; use "off" to disable
stack_links_when_native = "keep"   # "keep" | "off" — keep stax body/comment links when native registration succeeds

[ai.generate]
title = "Prefix PR titles with the issue key"
body = "Include testing and rollout sections"

[remote]
auto_fork = false                  # always fall back to `--fork` when the upstream push is denied for lack of write access
fork_remote = "fork"               # name of a pre-configured git remote for the fork (skip auto-detect)

# Native GitHub Stacked PRs are additive. Disable with native_stack = "off" or st submit --no-native-stack.
# Repos/users without access or without `github/gh-stack` installed behave exactly as normal stax. `stax doctor --fix`
# can offer `gh extension install github/gh-stack` when `gh` is installed.
# `stax submit --native-stack` still keeps submit non-blocking, but prints an
# actionable note when `gh`, `github/gh-stack`, or `gh stack link` support is missing.
# gh-stack v0.0.8+ uses the public Stacks REST API and preserves normal GitHub
# CLI authentication, including GH_TOKEN/GITHUB_TOKEN. For known older versions,
# stax strips those overrides before `gh stack` and falls back to a keyring OAuth
# account. `stax doctor` always shows the installed version, marks anything below
# v0.1.0 as out of date (v0.1.0 adds `gh stack merge` for atomic `merge --stack`),
# can upgrade it with `stax doctor --fix`, and probes legacy OAuth (versions
# below v0.0.8 only) only when token overrides exist.
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
stax merge --stack                 # GitHub/GitLab only: target selected items to trunk, merge the tip, preserve merged state
                                    # (GitHub: delegates to atomic `gh stack merge` when the repo has a confirmed native
                                    # Stack and gh-stack v0.1.0+; falls back to the flow above otherwise, with a note)
stax merge --stack --method rebase # Single-PR GitHub only; multi-PR GitHub and all GitLab ranges reject rewriting methods
stax merge --stack --downstack-only # Stack-merge ancestors below current; keep current open
stax merge --stack --full          # Stack-merge full stack even from the middle
stax merge --stack --when-ready    # Wait only for selected tip PR/MR readiness before the one-item stack merge
stax merge --when-ready            # Wait for CI + approval before each merge
stax merge --remote                # Merge via GitHub API only — no local checkout/rebase/push
stax merge --remote --all          # Include full stack (GitHub only)
stax merge --interval 30           # Positive poll seconds for --when-ready / --remote / --queue / --stack --when-ready
stax merge --no-wait               # Fail fast if CI is pending
stax merge --timeout 60            # Positive max-wait minutes (default 30; zero is rejected)
stax merge --no-delete             # Keep branches after merge
stax merge --no-sync               # Skip post-merge sync
stax merge-when-ready              # Backward-compatible alias

# For merge --queue, timeout is a hard polling deadline: cap the final sleep to
# the remaining budget and never poll the forge at or after the deadline.

stax rs                            # Sync trunk + clean merged branches
stax rs --restack                  # Sync then restack
stax sync --dry-run                # Preview sync plan (read-only — no fetch, no stash, no ref writes); alias: --plan
stax sync --dry-run --json         # Same as --dry-run but emits a single JSON doc (kind:"sync_plan", schema_version:1, dry_run:true); always exits 0; no receipt written
stax sync --json                   # Run sync and emit result as JSON (kind:"sync", schema_version:1); implies non-interactive (quiet); does NOT imply --force; failures emit JSON + non-zero exit
stax sync --json --force           # Same as --json but also auto-confirms branch deletions (scripting entry point)
stax sync --continue               # Continue after resolved sync conflicts
stax sync --safe                   # Avoid hard reset on trunk update
stax sync --force                  # Force sync without prompts; preserve linked worktrees during cleanup
# Interactive Sync plan: stax sync/rs only (not refresh/update). After fetch + PR metadata refresh; lists trunk, deletions, restack cascade; skipped with --force, --quiet, or --json.
stax sync --prune                  # Deprecated: accepted for compatibility, emits a stderr warning; use --full instead
stax sync --full                   # Fetch all remote branches with --prune (slower; default is trunk-only fetch + ls-remote)
stax sync --no-delete              # Keep merged branches
stax sync --auto-stash-pop         # Stash/pop dirty target worktrees during the restack phase
stax sync --stash                  # Stash the current working tree before sync starts without prompting; works with --quiet/--json; does NOT auto-confirm branch deletions; conflicts with --no-stash at parse time
stax sync --no-stash               # Fail on a dirty working tree; overrides --force; conflicts with --stash at parse time
# sync cleanup switches/detaches linked worktrees before deleting merged/gone branches; interactive removal remains explicit.
# The sync footer reports trunk commits/files/line changes plus non-zero cleanup/imported/restack counts.
# Conditional attention lines name blocked cleanup, trunk failures, and checkout changes, followed by one prioritized next command. For a diverged trunk, inspect and reconcile it with its remote instead of treating `st trunk` as a repair; other trunk failures use `st trunk`. Routine restack health stays in stax ls and the TUI.
# When --restack is requested, a failed fetch or trunk that did not reach the fetched remote commit stops sync before imported refresh, merged cleanup, or feature-branch rebases. Any sync auto-stash is restored first.
# Deletion lines for locally deleted branches (merged or upstream-gone) show the branch tip SHA (7 chars, dimmed) for traceability.
# If sync auto-stashed your working tree and fails on an error path that cannot restore it, stderr names the stash ("stax auto-stash") with instructions to run `git stash pop`.
# sync is transactional: trunk fast-forwards, merged/gone branch deletions, reparented-child metadata, and the optional restack phase are all covered by one receipt. A no-op sync writes no receipt so the previous undoable operation remains on the undo stack. Recover any sync run with `stax undo`.
# --json scripting entry points: stax sync --json --force (delete all merged); stax sync --json (skip deletions needing confirmation); stax sync --dry-run --json (read-only plan); dirty tree → success:false, error.kind:dirty_working_tree, non-zero exit; error message names --stash; stax sync --json --stash succeeds on a dirty tree (stashes before sync, restores after); --json conflicts with --continue.
# trunk.action values: up_to_date · fast_forwarded · reset · diverged · failed · unknown. On early-bail paths (dirty tree, non-interactive) trunk.action is "unknown" because finalize never runs — intended.

stax sweep                         # Classify ALL local branches (merged/gone/stale/active) — read-only
stax sweep --delete                # Delete merged/tracked-merged PRs + upstream-gone branches with no unique work after confirmation
stax sweep --delete --include-stale  # Also delete stale branches
stax sweep --delete --force        # Skip confirmation prompt
stax sweep --stale-days 60         # Override stale threshold in days (default 30, or branch.stale_days config)
stax sweep --json                  # Machine-readable branch classification (conflicts with --delete)

stax refresh                        # Sync trunk, restack, then submit (no merged cleanup; no Sync plan prompt)
stax refresh --no-pr                # Push only after trunk sync/restack
stax refresh --no-submit            # Trunk sync/restack only
stax refresh --all-stacks           # Sync trunk once, then restack/submit every independent stack; needs a clean tree unless --auto-stash-pop; stops at first conflict
stax refresh --all-stacks --auto-stash-pop # Stash/pop dirty worktrees while refreshing every stack
stax refresh --force                # Force sync without prompts first
stax refresh --force --yes --no-prompt # Full refresh without sync/submit prompts
stax refresh --verbose              # Show detailed sync/restack/submit timings
# refresh inherits sync's fetch/trunk guard and exits before its submit phase, so it does not push or update PRs after that failure.
# `stax update` is a separate top-level command: it upgrades the CLI (see `stax cli upgrade`), then offers to refresh installed AI agent skill files.

stax restack                       # Restack current branch onto parent
stax restack --all                 # Restack whole stack
stax restack --continue            # Continue after conflicts
stax restack --dry-run             # Predict conflicts only
# Preview commands (read-only, always exit 0): stax sync --dry-run · stax restack --dry-run · stax submit --dry-run · stax merge --dry-run
stax restack --submit-after yes    # ask|yes|no
stax restack --auto-stash-pop      # Stash/pop dirty target worktrees
stax restack --quiet               # Also silences the preflight notice below

stax cascade                       # Restack bottom-up then submit (no trunk fetch; offline-friendly)
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
stax ready                         # Interactive unmerged-PR readiness TUI; refresh drops remotely merged PRs without local cleanup
stax ready --current               # PR readiness TUI scoped to current stack only
stax ready --plain                 # Static readiness table: action · PR · branch · reviews · CI · title
stax ready --all                   # Explicit all tracked branches (default)
stax ready --json                  # Machine-readable readiness rows (existing schema: action/reason/branch/…)
stax ready --interval 30           # Override auto-refresh interval (default 15s)
stax pr list --ready               # Same interactive TUI under PR list
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

stax watch --iterations 1          # Render exactly one refresh, then exit
stax watch --iterations N --interval <seconds>  # Run N total refreshes with a delay between them

# Oneline roll-up: status icon · branch · #PR · draft/ready · title · check-count + timing.
# Single branch shows the full per-check table; any multi-branch view defaults to oneline;
# --verbose forces the grouped cards. --oneline conflicts with --verbose.
# GitHub commit statuses and check runs are aggregated across all pages before latest-result roll-up.

`--iterations` counts total refreshes: `1` renders exactly once, `0` is invalid, and a bounded run never sleeps after its final refresh. For `N > 1`, use `--interval <seconds>` to set the delay between refreshes.

# ~/.config/stax/config.toml
[ci]
alert = true                       # Play success/error sounds for stax ci --watch
success_alert_sound = "/path/to/ci-success.wav"  # optional, built-in when omitted
error_alert_sound = "/path/to/ci-error.wav"      # optional, built-in when omitted

stax standup --hours 48            # Summarize recent activity window
stax standup --all --json          # All stacks in JSON
stax standup --ci --json           # Check selected branches' CI and report signal states
stax standup --ai             # AI spoken standup — colored card, word-wrapped
stax standup --ai --style slack  # AI Slack-ready Yesterday/Today bullets
stax standup --ai --agent claude  # Override AI agent for one run
stax standup --ai --plain-text    # Raw text output (pipe-friendly)
stax standup --ai --json          # {"summary": "..."} JSON
stax standup --ai --jit           # Add Jira context via jit (github.com/cesarferreira/jit)

# CI is not checked unless --ci is present. In JSON, do not treat an empty
# reviews_given or needs_attention.ci_failing array as authoritative until its
# signals entry is "available". GitLab/Gitea authored reviews are unsupported;
# GitHub uses one time-bounded, maximum-100 GraphQL query.

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

`[ai.generate].title` applies to `stax gen --pr-title` and title generation in `stax submit --ai`; `.body` applies to `--pr-body` and submit body generation. A repo-root `stax.toml` can override either global field independently. Empty/whitespace-only values do nothing, commit-message generation ignores both, and stax's final JSON/markdown-only output rule remains authoritative.

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
stax update                        # Upgrade the CLI, then offer to refresh installed AI agent skill files
stax doctor                        # Repo/config health checks (also reports stale skill files)
stax doctor --fix                  # Confirm once to set recommended git config and update stale installed skills
stax demo                          # Interactive tutorial

stax skills                        # List installed AI agent skill files + local package-version-marker status
stax skills list                   # Same as above; does not verify fetched content
stax skills update                 # Fetch, compare fully rendered files, and refresh changed instructions
stax skills update --dry-run       # Preview content-based updates without writing
```

GitHub `401`/`404` errors on repository searches or already-resolved
review/comment reads can mean the token is expired or lacks private-repository
access. Follow stax's auth hint: run `stax auth --from-gh`, then verify token
scopes and repository access. Do not treat every 404 as an auth failure: direct
missing-PR lookups and mutations deliberately retain their resource-level
errors.

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
stax merge --stack --when-ready    # GitHub/GitLab stack merge: selected tip CI only, preserving merge
```

### After Base PR Merges

```bash
stax refresh
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
stax setup --yes               # Shell integration + skills for detected agents + auth import from gh when available
stax setup --install-skills    # Non-interactive: shell integration + skills for all harnesses
stax skills update --all         # Update every harness, ignoring configured selection

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
│ ○    ☁ wt feature/payments PR #42 # ☁ = has remote, wt/󰙅 = linked worktree, PR #N = open PR
○─┘    ☁ main                    # trunk branch
```

Symbols:

- `◉` = current branch
- `○` = other branch
- `☁` = has remote tracking
- `wt` / Nerd Font tree icon = checked out in a linked worktree (`display.worktree_glyph` in config)
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
- Use `stax --help` or `stax <command> --help` for exact flags.
- Run `stax skills update` to refresh changed skill instructions even when `stax skills list` shows the installed package version as current; byte-identical files are not rewritten.
- Add global `--trace` to profile instrumented Git subprocesses and total command time; use `make benchmark-status` for reproducible cold status scaling fixtures.
- Hidden convenience shortcuts: `stax bc`, `stax bu`, `stax bd`, `stax bs`, `stax w`, `stax wtc`, `stax wtgo`, `stax wtrm`.
- Use `--yes` for non-interactive scripting.
- Use `--json` on supported commands for machine-readable output.
- Use `stax lane` with no arguments for an interactive picker over all stax-managed lanes — useful when you forget where a session lives.
- Use `stax worktree go` (or `sw`) + shell integration to switch between stacks without `cd` gymnastics.
- Use `stax worktree promote` when a lane should become the main-worktree checkout; it refuses dirty or conflicted checkouts instead of stashing automatically. If Git reports a removal failure after already retiring the lane, Stax keeps the completed promotion and warns you to inspect leftover files.
- `stax worktree list` shows ALL worktrees including those created externally via `git worktree add`.
