# Full command reference

The complete command surface. For day-to-day commands only, see [Core commands](core.md). For navigation specifically, see [Navigation](navigation.md).

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `st status` | `ls` | Show stack |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack |
| `st merge` | | Cascade-merge from bottom to current (see flags below) |
| `st merge-when-ready` | `mwr` | Backward-compatible alias for `st merge --when-ready` |
| `st sync` | `rs` | Pull trunk, delete merged branches (incl. squash merges), reparent children |
| `st sync --restack` | `rs --restack` | `sync` **plus** rebase current stack onto updated parents |
| `st sync --delete-upstream-gone` | | Also delete local branches whose upstream tracking ref is gone |
| `st sweep` | | Classify all local branches as merged / upstream-gone / stale / active (read-only by default) |
| `st sweep --delete` | | Delete merged branches (including tracked merged PRs) and upstream-gone branches with no unique work after confirmation |
| `st sweep --delete --include-stale` | | Also delete stale branches (older than `--stale-days` / `branch.stale_days` config key) |
| `st sweep --delete --force` | | Skip confirmation prompt |
| `st sweep --stale-days <N>` | | Override stale threshold in days (default: 30) |
| `st sweep --json` | | Machine-readable branch classification (conflicts with `--delete`) |
| `st update` | | Sync trunk without merged-branch cleanup, restack, then push and create/update PRs for the current stack |
| `st update --force --yes --no-prompt` | | Run the full update flow without sync or submit prompts |
| `st update --verbose` | | Same as `st update`, with detailed sync/restack/submit timing |
| `st restack` | | Rebase current stack locally — auto-normalizes missing/merged parents; `--stop-here` limits scope |
| `st cascade` | | Restack from bottom and submit updates |
| `st diff` | | Show per-branch diffs vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |
| `st stack` | `s` | Stack command namespace for `submit` and `restack` (`st stack submit`, `st stack restack`) |

### `st merge` variants

- `st merge` — local cascade merge with provenance-aware descendant rebases, then `st rs --force` unless `--no-sync`
- `st merge --when-ready` — wait for CI + approvals + mergeability; incompatible with `--dry-run`, `--no-wait`, `--remote`, and `--queue`
- `st merge --downstack-only` / `--ds` — merge ancestors below the current branch, then rebase the current branch onto trunk; composes with `--stack`, and is incompatible with `--all`, `--full`, `--remote`, and `--queue`
- `st merge --stack` — GitHub-only fast-forward stack merge: validate the selected tip PR once, retarget it to trunk, merge only that PR, wait briefly for selected downstack PRs to become merged in GitHub, and rebase/retarget remaining descendants; defaults to `--method rebase`
- `st merge --stack --full` — include descendants above the current branch and land the full stack through the actual stack tip
- `st merge --remote` — merge entirely via GitHub API, no local git operations (GitHub only)
- `st merge --queue` — enqueue PRs into GitHub merge queue / GitLab merge trains

See also: [Merge and cascade](../workflows/merge-and-cascade.md)

## Navigation

| Command | Alias | Description |
|---|---|---|
| `st checkout` | `co`, `bco` | Interactive branch picker |
| `st trunk` | `t` | Switch to trunk (or set trunk with `st trunk <branch>`) |
| `st up [n]` | `u` | Move up to child |
| `st down [n]` | `d` | Move down to parent |
| `st top` | | Stack tip |
| `st bottom` | | Stack base |
| `st prev` | `p` | Toggle to previous branch |

## Branch management

| Command | Alias | Description |
|---|---|---|
| `st create <name>` | `c`, `add`, `bc` | Create stacked branch (TTY menu when nothing staged and `-m`) |
| `st create --ai` | | Generate a branch name from local changes (`-a` also generates a first commit message) |
| `st create <name> --below` | | Insert a new branch below current |
| `st get [branch|PR]` | | Sync current stack, or fetch, sync/create, checkout, and track a remote branch/PR |
| `st modify` | `m` | Amend staged changes into current commit (`-a` stages all, `-r` restacks after) |
| `st rename` | | Rename current branch |
| `st move [target]` | `mv` | Move the current branch and descendants onto a new parent (`st upstack onto` parity alias; picker when omitted) |
| `st branch track` | | Track an existing branch |
| `st branch track --all-prs` | | Track all open PRs (GitHub, GitLab, Gitea) |
| `st branch untrack` | `ut` | Remove stax metadata |
| `st branch reparent` | | Change parent |
| `st branch submit` | `bs` | Submit current branch only; can temporarily restack the publish head when the excluded parent is remote-synced |
| `st branch delete` | | Delete branch |
| `st fold` / `st branch fold` | `b f` | Fold current branch into its parent (preserves commits, reparents descendants, rebases siblings; `--keep` keeps current name) |
| `st branch squash` | | Squash commits |
| `st detach` | | Remove branch from stack, reparent children |
| `st reorder` | | Interactively reorder branches in stack |
| `st absorb` | | Distribute staged changes to the correct stack branches (file-level) |

### Up/down scopes

| Command | Description |
|---|---|
| `st upstack restack` | Restack current + descendants |
| `st upstack onto [branch]` | Reparent current + descendants onto a new parent |
| `st upstack submit` | Submit current + descendants; temporary publish heads are chained for stale descendants |
| `st downstack get` | Show branches below current |
| `st downstack submit` | Submit ancestors + current |

## Interactive modes

| Command | Description |
|---|---|
| `st` | Launch the TUI |
| `st split` | Split branch into stacked branches (commit-based; needs 2+ commits) |
| `st split --hunk` | Split a single commit by selecting individual diff hunks |
| `st split --file <pathspec>` | Split by extracting matching files into a new parent branch |
| `st edit` · `e` | Interactively edit commits (pick, reword, squash, fixup, drop) |

## Recovery

| Command | Description |
|---|---|
| `st resolve` | AI-resolve an in-progress rebase conflict |
| `st abort` | Abort the in-progress rebase / conflict resolution |
| `st undo` | Undo the last operation |
| `st undo <op-id>` | Undo a specific operation |
| `st redo` | Re-apply the last undone operation |

## Health and testing

| Command | Description |
|---|---|
| `st validate` | Check stack metadata for orphans, cycles, and staleness |
| `st fix` | Auto-repair broken metadata (`--dry-run` previews) |
| `st run <cmd>` | Run a command on each branch (alias: `st test`); `--stack[=<branch>]`, `--all`, `--fail-fast` |

## CI, PRs, and reporting

| Command | Description |
|---|---|
| `st ci` | CI status for current branch — full per-check table (with elapsed/ETA learned from recent runs) |
| `st ci --stack` / `--all` | Scope to stack / all tracked branches; multi-branch views default to the one-line roll-up |
| `st ci --oneline` / `-1` | One compact line per branch (icon · branch · #PR · draft/ready · title · checks + timing) |
| `st ci --watch` | Watch modes (`--watch --strict` fail-fasts on failure) |
| `st ci -w --alert` / `--alert <file>` / `--no-alert` | Success/error completion sounds for watch mode |
| `st ci --verbose` / `--json` | Grouped summary cards · JSON output |
| `st pr` · `st pr open` | Open current branch PR |
| `st pr body` · `st pr body --edit` | Print or edit the current branch PR description |
| `st pr list` | List open PRs (GitHub, GitLab, Gitea) |
| `st pr list --ready` | Open live PR readiness for all tracked branch PRs, newest changed PR first (`--current`/`--stack` limits to the current stack, `--plain` prints a table) |
| `st ready` | Short alias for `st pr list --ready` (`--current`, `--stack`, `--all`, `--plain`, `--json`) |
| `st draft [branch]` | Mark the current or named branch's PR as a draft |
| `st undraft [branch]` | Mark the current or named branch's PR as ready for review |
| `st issue list` | List open issues |
| `st comments` | Show PR comments |
| `st copy` · `st copy --pr` | Copy branch name · PR URL |
| `st standup` | Recent activity (`--ai` for AI spoken version; `--jit` for Jira context) |
| `st changelog [from] [to]` | Generate changelog (auto-resolves last tag when `from` omitted) |
| `st changelog find [query]` | Fuzzy-find commits in the selected changelog range |
| `st changelog --find [query]` | Flag form of commit fuzzy-find |
| `st generate` · `st gen` | AI generation: interactive picker, or `--pr-body` / `--pr-title` / `--commit-msg` |
| `st ss --ai` | Submit with AI-generated PR title/body suggestions |
| `st watch` | Live auto-refreshing stack status with CI and PR state (`--current`, `--interval <seconds>`) |

## Utilities

| Command | Description |
|---|---|
| `st auth` | Configure GitHub token (`--from-gh`, `--token <token>`, `status`) |
| `st config` | Show current configuration |
| `st config --set-ai` | Interactively set AI agent/model (global or per-feature) |
| `st config --reset-ai` | Clear saved AI defaults and re-prompt (`--no-prompt` to clear only) |
| `st init` | Initialize stax or reconfigure trunk (`--trunk <branch>`) |
| `st cli upgrade` | Detect install method and run the matching upgrade |
| `st doctor` | Check repo health |
| `st doctor --fix` | Apply safe local repairs after one confirmation (recommended Git config and stale AI skills) |
| `st skills` | Manage installed AI agent skill files (`list`, `update`, `update --dry-run`) |
| `st continue` | Continue after conflicts |
| `st open` | Open repository in browser |
| `st demo` | Interactive tutorial — no auth or repo required |

### `st tmux`

| Command | Description |
|---|---|
| `st tmux status` | Print a compact tmux-formatted status string for `status-right` |
| `st tmux popup` | Open `stax watch --current` in a tmux display-popup |

## Worktrees

Full guide: [Worktrees](../worktrees/index.md) · [AI lanes](../workflows/agent-worktrees.md)

| Command | Aliases | Description |
|---|---|---|
| `st worktree` | `wt` | Open the interactive dashboard (TTY only) |
| `st worktree create [name]` | `wt c`, `wtc` | Create or reuse a lane (random name if omitted) |
| `st lane [name] [prompt]` | | AI-lane entrypoint; bare `st lane` opens a picker |
| `st worktree list` | `wt ls`, `w`, `wtls` | List all worktrees |
| `st worktree ll` | `wt ll` | Rich status view |
| `st worktree go [name]` | `wt go`, `wtgo` | Navigate to a worktree (shell integration required for `cd`) |
| `st worktree path <name>` | | Print absolute path (scripting) |
| `st worktree remove [name]` | `wt rm`, `wtrm` | Remove a worktree (`wt rm` removes the current lane) |
| `st worktree prune` | `wt prune`, `wtprune` | Clean stale git worktree bookkeeping |
| `st worktree cleanup` | `wt cleanup`, `wt clean` | Prune + remove safe detached/merged lanes (`--dry-run` previews) |
| `st worktree restack` | `wt rs`, `wtrs` | Restack all stax-managed worktrees |

### `st setup`

| Command | Description |
|---|---|
| `st setup` | One-shot onboarding: shell integration + optional skills + auth |
| `st setup --yes` | Accept defaults, install skills, import auth from `gh` when available |
| `st setup --install-skills` / `--skip-skills` | Control AI agent skills prompt |
| `st setup --auth-from-gh` / `--skip-auth` | Control auth onboarding |
| `st setup --print` | Print shell integration snippet for manual install |

### Lane launch examples

```bash
st lane
st lane review-pass "address PR comments"
st lane fix-flaky --agent claude --yolo "stabilize the flaky tests"
st lane big-refactor --agent claude --agent-arg=--verbose "split the auth module"
st wt go ui-polish --run "cursor ." --tmux
```

## Flags by command

### `st modify`

- `-a` stage all and amend
- `-am "msg"` stage all and amend with a new message
- `-r` restack after amending
- `-ar` stage all, amend, restack
- With nothing staged in a TTY: menu to stage all, `--patch`, amend message only, or abort

### `st create`

- `st add <name>` is an alias for `st create <name>`
- `-m "msg"` set commit message (with nothing staged in a TTY: menu for stage all, `--patch`, empty branch, or abort)
- `-am "msg"` stage all and commit
- `--ai` generate missing branch name and/or first commit message from local changes
- `--ai -a --yes` stage all changes, generate branch name + commit message, and skip AI value review prompts
- `st create <name> --ai -a` keeps `<name>` and generates the first commit message
- `st create --ai -m "msg"` keeps the commit message and generates the branch name
- `-n`, `--no-verify` skip pre-commit and commit-msg hooks when creating a commit
- `-m` / `-am` create the commit before creating the destination branch, including with `--from` and `--below`, so hook failures or interrupts do not leave orphan branches
- `-m` / `--ai` derived branch names refuse collisions instead of creating `-2` duplicates; pass an explicit different name or checkout/reparent the existing branch
- `--insert` reparent children of the current branch onto the new branch
- `--below` create from the current branch's parent and reparent the current branch onto the new branch; prepared tracked and untracked changes are auto-stashed and reapplied onto the new lower branch, and `-m`/`-am` commits staged changes there
- `st branch create --message "msg" --prefix feature/`

Prepared-work `--below` example:

```bash
# On an upstack branch, after editing a CVE hotfix that belongs lower down:
st create cve-hotfix --below

# Or commit it immediately on the inserted lower branch:
st create --below -am "fix: patch CVE-2026-0001"
```

If the stash cannot apply cleanly while committing below, Stax restores the original branch and prepared changes so the same command can be retried after resolving the conflict. For name-only `--below`, the inserted branch is left in place and the auto-stash remains available for a manual `git stash apply`.

### `st status` / `st ll` / `st log`

- `--stack <branch>` · `--current` · `--compact` · `--json` · `--quiet`

### `st submit`

- `--draft` / `--publish` / `--no-pr` / `--no-fetch` / `--no-verify` / `--open` / `--quiet` / `--verbose`
- `--no-verify` (`-n`) skips pre-push hooks while pushing branches
- `--reviewers alice,bob --labels bug,urgent --assignees alice`
- `--squash` squash commits on each branch before pushing
- `--ai` generate PR title and body with AI; narrow with `--title` or `--body`
- `--template <name>` / `--no-template` / `--edit`
- `--rerequest-review` / `--update-title`
- `--yes` / `--no-prompt`

Config: `[submit] stack_links = "comment" | "body" | "both" | "off"` in `~/.config/stax/config.toml`.

### `st merge`

- `--dry-run` / `--yes`
- `--all` / `--downstack-only` (`--ds`) / `--stack` / `--stack --full` / `--method squash|merge|rebase`
- `--when-ready` · `--when-ready --interval 10`
- `--remote` · `--remote --all` · `--remote --timeout 60 --interval 10`
- `--queue` · `--queue --all --yes`
- `--no-wait` / `--no-sync` / `--no-delete` / `--timeout 60` / `--quiet`

### `st sync` / `st rs`

- `--restack` · `--restack --auto-stash-pop`
- `--delete-upstream-gone`
- `--force` / `--safe` / `--continue` / `--quiet` / `--verbose`
- Imported branches from `st get` are remote-delete exempt: once they are detected as merged or upstream-gone, sync may delete the local support branch and metadata, but it will not push-delete the imported remote branch.

### `st restack`

- `--all` / `--continue` / `--quiet`
- `--stop-here`
- `--submit-after ask|yes|no`

### Temporary publish restack

`st submit`, `st downstack submit`, `st branch submit`, and `st upstack submit` can publish a temporary rebased head without moving local branch tips. When a submitted branch needs restack, Stax creates an internal temporary ref, replays the branch's current commits onto the submitted parent for the push, and keeps local metadata unchanged. Descendants chain onto those temporary publish heads so the remote stack stays linear.

If the excluded parent has local-only commits, scoped submit still refuses and asks you to include ancestors with `st downstack submit` / `st submit` or restack first. `--squash` also requires a local restack first because squashing rewrites local branch history.

### `st resolve`

- `--agent codex --model gpt-5.3-codex --max-rounds 5`

### `st cascade`

- `--no-pr` / `--no-submit` / `--auto-stash-pop`

### `st checkout`

- `--trunk` / `--parent` / `--child 1`

### `st get`

- With no argument, `st get` syncs and restacks the current stack, equivalent to the Graphite `gt get` current-stack flow.
- The argument may be a remote branch name, `origin/<branch>`, or a PR number when forge auth is configured.
- `--parent <branch>` records a non-trunk parent in stax metadata
- `--no-checkout` fetches and tracks without switching branches
- `--downstack` skips local upstack branches when the target already exists locally.
- `--remote-upstack` includes remote-only upstack PR branches discovered from open PR base/head metadata. This is best-effort without Graphite's central backend.
- `--no-restack` skips the default restack after checkout.
- `--unfrozen` is accepted for Graphite CLI compatibility; Stax does not currently freeze branches.
- Existing local branches fast-forward when possible, or rebase local-only commits onto the fetched remote tip when branch histories diverge.
- `--force` resets an existing local branch to the remote tip instead of preserving local commits.
- Branches checked out in another linked worktree are skipped instead of being moved from the current worktree.
- New remote-only branches imported by `st get` are read-only support branches: submit uses them as stack bases but does not push them or update their PRs. Existing Stax-managed branches keep their ownership metadata when synced with `st get`.
- `st sync --restack` refreshes imported branches from their remote tips before restacking descendants; if an imported branch is checked out in a dirty worktree, sync skips it unless `--force` is used.

### `st ci`

- `--stack` / `--all` / `--oneline` (`-1`) / `--verbose` / `--watch` / `--watch --strict` / `--interval 30` / `--json`
- Three render modes: the **full per-check table** (single branch, default), grouped **summary cards** (`--verbose`/`-v`), and the **one-line roll-up** (`--oneline`/`-1`). Any multi-branch view (`--stack`/`--all`) defaults to the roll-up; `--verbose` overrides it back to cards. `--oneline` and `--verbose` cannot be combined.
- The roll-up renders one line per branch, base→tip: CI status icon · branch · `#PR` · `draft`/`ready` · PR title · trailing check-count and timing. A bare `--oneline` defaults its scope to the current stack.
- By default, `--watch` waits until every check is terminal, even if one check has already failed. Add `--strict` to exit as soon as any check fails.
- `--watch --alert` plays built-in success/error sounds; `--watch --alert <file>` uses one custom sound for either outcome; `--watch --no-alert` suppresses `[ci] alert = true` for one run.
- Config can enable alerts by default with `[ci] alert = true`; set `success_alert_sound` and/or `error_alert_sound` to override the per-outcome built-in sounds.

### `st standup`

- `--all` / `--hours 48` / `--json`
- `--ai` · `--ai --agent claude` · `--ai --hours 48`
- `--ai --style slack`
- `--ai --plain-text` / `--ai --json` / `--ai --jit`

### `st pr` / `st issue`

- `st pr list --limit 50 --json`
- `st issue list --limit 50 --json`

### `st generate` · `st gen`

- Bare `st gen` opens an interactive picker (PR body, PR title, commit message).
- `--pr-body` — refresh the open PR body from the branch diff (PR templates: `--template` / `--no-template`).
- `--pr-title` — refresh the open PR title from the branch diff.
- `--commit-msg` — amend `HEAD` with an AI-generated message from the last commit’s patch.
- Shared: `--no-prompt` / `--edit` / `--agent <name>` / `--model <name>` (`--model` requires `--agent`).

### `st changelog`

- `--tag-prefix release/ios`
- `--path src/`
- `find [query]` / `search [query]` — fuzzy-find commits in the selected range; omit `query` for an interactive picker.
- `--find [query]` / `--search [query]` — flag form of the same fuzzy finder.
- `--json`

### `st auth`

- `--from-gh` / `--token <token>` / `status`

### `st init`

- `--trunk main`

### `st undo` / `st redo`

- `--yes` / `--no-push` / `--quiet`

### `st absorb`

- `--dry-run` (preview) · `-a` (stage all first)

### `st edit`

- `--yes` (skip final confirmation) · `--no-verify` (skip pre-commit hooks)

### `st split`

- `--file <pathspec>` (or `-f "src/api/*"` with glob support)
- `--hunk` (single-commit hunk-based split)
