# Full command reference

The complete command surface. For day-to-day commands only, see [Core commands](core.md). For navigation specifically, see [Navigation](navigation.md).

## Global diagnostics

Add `--trace` before or after any subcommand to print instrumented Git
subprocess timings, the summed Git time, and command wall time. URL- and
token-shaped arguments are redacted.

```bash
st --trace status --json >/dev/null
```

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `st status` | `ls` | Show stack |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack |
| `st stack link` | | Register the current PR stack as a native GitHub Stack via `gh stack link` |
| `st stack unlink [<stack-number>]` | | Unstack a remote native Stack by number, or the active locally tracked stack when omitted |
| `st merge` | | Cascade-merge from bottom to current (see flags below) |
| `st merge-when-ready` | `mwr` | Backward-compatible alias for `st merge --when-ready` |
| `st sync` | `rs` | Pull trunk, delete merged branches (incl. squash merges), reparent children |
| `st sync --restack` | `rs --restack` | `sync` **plus** rebase current stack onto updated parents |
| `st sync --delete-upstream-gone` | | Also delete local branches whose upstream tracking ref is gone |
| `st sync --stash` | `rs --stash` | Stash the current working tree before sync starts without prompting; works with `--quiet` and `--json`; does NOT auto-confirm branch deletions; conflicts with `--no-stash` at parse time |
| `st sync --no-stash` | `rs --no-stash` | Fail if the working tree is dirty; overrides `--force`; conflicts with `--stash` at parse time |
| `st sync --dry-run` / `st sync --plan` | | Preview what sync would do — ls-remote only, no fetch/stash/ref-writes/push/metadata writes; always exits 0; composes with `--restack`, `--delete-upstream-gone`, `--safe`; `--force`, `--auto-stash-pop`, `--full`, `--stash`, `--no-stash`, and `--verbose` emit a warning and are otherwise ignored; `--continue` is rejected |
| `st sync --dry-run --json` | | Same as `--dry-run` but emits a single JSON document (`kind: "sync_plan"`, `schema_version: 1`, `dry_run: true`) instead of human text |
| `st sync --json` | | Emit the sync result as a single JSON document (`kind: "sync"`, `schema_version: 1`); implies non-interactive; failures emit JSON + non-zero exit; conflicts with `--continue` |
| `st sweep` | | Classify all local branches as merged / upstream-gone / stale / active (read-only by default) |
| `st sweep --delete` | | Delete merged branches (including tracked merged PRs) and upstream-gone branches with no unique work after confirmation |
| `st sweep --delete --include-stale` | | Also delete stale branches (older than `--stale-days` / `branch.stale_days` config key) |
| `st sweep --delete --force` | | Skip confirmation prompt |
| `st sweep --stale-days <N>` | | Override stale threshold in days (default: 30) |
| `st sweep --json` | | Machine-readable branch classification (conflicts with `--delete`) |
| `st refresh` | `r` | Sync trunk without merged-branch cleanup, restack, then push and create/update PRs for the current stack |
| `st refresh --force --yes --no-prompt` | | Run the full refresh flow without sync or submit prompts |
| `st refresh --verbose` | | Same as `st refresh`, with detailed sync/restack/submit timing |
| `st refresh --all-stacks` | | Refresh every stack in the repo (fetch/trunk sync once, then restack + submit each stack); stops at the first conflict |
| `st restack` | | Rebase current stack locally — auto-normalizes missing/merged parents; `--stop-here` limits scope |
| `st cascade` | | Restack the stack and submit updates, without fetching trunk (offline-friendly) |
| `st diff` | | Show per-branch diffs vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |
| `st stack` | `s` | Stack command namespace for `submit` and `restack` (`st stack submit`, `st stack restack`) |

### `st merge` variants

- `st merge` — local cascade merge with provenance-aware descendant rebases, then `st rs --force` unless `--no-sync`
- `st merge --when-ready` — wait for CI + approvals + mergeability; incompatible with `--dry-run`, `--no-wait`, `--remote`, and `--queue`
- `st merge --downstack-only` / `--ds` — merge ancestors below the current branch, then rebase the current branch onto trunk; composes with `--stack`, and is incompatible with `--all`, `--full`, `--remote`, and `--queue`
- `st merge --stack` — GitHub/GitLab stack merge: target every selected PR/MR to trunk, merge only the selected tip with SHA-preserving `merge`, and poll lower items for authoritative merged state; timed-out items stay open. Multi-PR GitHub `rebase`/`squash` fails before mutation because rewritten SHAs prevent genuine lower-PR merged state; a single selected GitHub PR may still use them. GitLab first verifies project merge settings, sends `squash: false`, and rejects explicit `rebase`/`squash`. Gitea/Forgejo is unsupported
- `st merge --stack --full` — include descendants above the current branch and land the full stack through the actual stack tip
- `st merge --remote` — merge entirely via GitHub API, no local git operations (GitHub only)
- `st merge --queue` — enqueue PRs into GitHub merge queue / GitLab merge trains, polling only within the configured timeout deadline

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
| `st next` | `n` | Move to the first unmerged branch upstack (deterministic on forks) |

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
| `st branch track --all` | | Track every untracked non-trunk local branch under its nearest cycle-safe strict local ancestor |
| `st branch track --all-prs` | | Track all open PRs (GitHub, GitLab, Gitea); sets upstream on branches it fetches |
| `st branch untrack` | `ut` | Remove stax metadata |
| `st branch reparent` | | Change parent |
| `st branch submit` | `bs` | Submit current branch only; can temporarily restack the publish head when the excluded parent is remote-synced |
| `st branch delete` | | Delete branch |
| `st fold` / `st branch fold` | `b f` | Fold current branch into its parent (preserves commits, reparents descendants, rebases siblings; `--keep` keeps current name) |
| `st branch squash` | | Squash commits |
| `st detach` | | Remove branch from stack, reparent children |
| `st reorder` | | Interactively reorder branches in stack |
| `st absorb` | | Distribute staged changes to the correct stack branches (file-level) |

`st branch track --all` is local and metadata-only: it does not contact a forge,
fetch or change remotes/upstreams, load remote configuration, or switch branches.
It snapshots the untracked non-trunk local branches before writing metadata and
leaves existing tracked metadata and the trunk untouched. For each target, it
chooses the cycle-safe strict local ancestor with the fewest commits between
that ancestor and the target, breaking equal-distance ties lexically by branch
name. Non-trunk branches at the same commit are never selected as one another's
parent. When no cycle-safe strict local ancestor exists, including for unrelated
histories, the branch is parented to trunk; trunk remains the fallback root even
when it points at the same commit. `--all` conflicts with `--parent` and
`--all-prs`.

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
| `st web [path] [--port <n>] [--no-open]` | Start the localhost HTMX workspace; a busy requested port falls back to a free OS-selected port |
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
| `st run <cmd>` | Run a command on each branch (alias: `st test`); `--stack[=<branch>]`, `--all`, `--fail-fast`, or `--parallel --jobs <N>` |
| `st freeze [branch]` / `st unfreeze [branch]` | Protect/unprotect a tracked branch from direct/upstack/get restacks and sync history rewrites, including imported refreshes and squash-merge cleanup rebases |

## CI, PRs, and reporting

| Command | Description |
|---|---|
| `st ci` | Live CI status for the current PR head — full per-check table (with elapsed/ETA learned from recent runs) |
| `st ci --stack` / `--all` | Scope to stack / all tracked branches; multi-branch views default to the one-line roll-up |
| `st ci --oneline` / `-1` | One compact line per branch (icon · branch · #PR · draft/ready · title · checks + timing) |
| `st ci --watch` | Watch modes (`--watch --strict` fail-fasts on failure) |
| `st ci -w --alert` / `--alert <file>` / `--no-alert` | Success/error completion sounds for watch mode |
| `st ci --verbose` / `--json` | Grouped summary cards · JSON output |
| `st pr` · `st pr open` | Open current branch PR |
| `st pr body` · `st pr body --edit` | Print or edit the current branch PR description |
| `st pr list` | List open PRs (GitHub, GitLab, Gitea) |
| `st pr list --ready` | Interactive PR readiness TUI for unmerged tracked PRs; remotely merged PRs disappear on live refresh (`--current`/`--stack` limits to the current stack, `--plain` for a static table) |
| `st ready` | Same as `st pr list --ready` — interactive TUI with CI, review approval, and merge state; filtering merged PRs does not clean up local branches (`--current`, `--stack`, `--all`, `--plain`, `--json`, `--interval`) |
| `st board` / `st home` | Interactive repository dashboard — PULL REQUESTS / ISSUES tabs, a detail pane (branch, files/+/−, CI checks, labels, body, comment count), inline diff and comment viewers, label add/remove, draft toggle, and confirm-then-merge (squash, API-only). GitHub only — errors on other forges. `--limit`, `--tab prs\|issues`, `--interval` (default 60s), `--plain` for static PR/issue tables |
| `st draft [branch]` | Mark the current or named branch's PR as a draft |
| `st draft --stack` | Mark every PR in the current stack as a draft |
| `st undraft [branch]` | Mark the current or named branch's PR as ready for review |
| `st undraft --stack` | Mark every PR in the current stack as ready for review |
| `st issue list` | List open issues |
| `st comments` / `st reviews` | Show current PR comments; `--stack` or `--all` creates a review inbox, GitHub review comments include inline file/line locations, and `--json` emits a versioned machine-readable view |
| `st copy` · `st copy --pr` | Copy branch name · PR URL |
| `st standup` | Recent activity (`--ci` for opt-in live CI; `--ai` for AI spoken version; `--jit` for Jira context) |
| `st changelog [from] [to]` | Generate changelog (auto-resolves last tag when `from` omitted) |
| `st changelog find [query]` | Fuzzy-find commits in the selected changelog range |
| `st changelog --find [query]` | Flag form of commit fuzzy-find |
| `st generate` · `st gen` | AI generation: interactive picker, or `--pr-body` / `--pr-title` / `--commit-msg` |
| `st ss --ai` | Submit with AI-generated PR title/body suggestions |
| `st watch` | Live auto-refreshing stack status with CI and PR state (`--current`, `--interval <seconds>`); `--iterations <N>` performs exactly `N` total refreshes (`1` renders exactly once; `0` is rejected), then returns without a final sleep. For `N > 1`, use `--interval <seconds>` to control the delay between refreshes. |

## Utilities

| Command | Description |
|---|---|
| `st auth` | Configure GitHub token (`--from-gh`, `--token <token>`, `status`) |
| `st config` | Show current configuration |
| `st user` | View or set personal preferences (`branch-prefix`, `branch-date`, `branch-replacement`, `editor`, `tips`, `submit-body`) |
| `st config --set-ai` | Interactively set AI agent/model (global or per-feature) |
| `st config --reset-ai` | Clear saved AI defaults and re-prompt (`--no-prompt` to clear only) |
| `st --default-config` | Print annotated config template (all options and allowed values) |
| `st --skill` | Print bundled AI agent skill document (SKILL.md format with frontmatter) |
| `st init` | Initialize stax or reconfigure trunk (`--trunk <branch>`) |
| `st update` | Detect install method and run the matching upgrade, then check installed AI agent skill files and offer to refresh them. Skips the CLI upgrade if already on the latest version — pass `--force` to upgrade anyway |
| `st doctor` | Check repo health |
| `st doctor --fix` | Apply safe local repairs after one confirmation (recommended Git config, stale AI skills for selected harnesses, and optional `gh-stack` install) |
| `st skills` | Manage installed AI agent skill files (`list`, `update`, `update --all`, `update --skills <list>`, `update --dry-run`) |
| `st continue` | Continue after conflicts |
| `st open` | Open repository in browser |
| `st demo` | Interactive tutorial — no auth or repo required |

`st skills update` fetches the remote body and compares each fully rendered harness file byte-for-byte, rewriting instructions that differ even when the installed package-version marker matches. It leaves byte-identical files untouched; `--dry-run` reports the same decision without writing. `st skills list` is intentionally local-only: its current/stale indicator compares the installed package-version marker with this stax binary and does not verify fetched content.

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
| `st worktree promote` | `wt promote` | Retire the current lane and check its branch out in the main worktree |
| `st worktree prune` | `wt prune`, `wtprune` | Clean stale git worktree bookkeeping |
| `st worktree cleanup` | `wt cleanup`, `wt clean` | Prune + remove safe detached/merged lanes (`--dry-run` previews) |
| `st worktree restack` | `wt rs`, `wtrs` | Restack all stax-managed worktrees |

### `st setup`

| Command | Description |
|---|---|
| `st setup` | One-shot onboarding: shell integration + optional skills + auth |
| `st setup --yes` | Accept defaults, install skills, import auth from `gh` when available |
| `st setup --install-skills` / `--skip-skills` | Control AI agent skills prompt |
| `st setup --skills <list>` | Choose harnesses (`all`, `detected`, `auto`, `none`, or comma-separated ids) |
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

- `--dry-run` / `--plan` prints a read-only plan without fetching, pushing, editing metadata, or calling forge mutation APIs; add `--json` for the versioned machine-readable schema (currently version 2; action strings are extensible)
- Plans query live remote head IDs with `git ls-remote` without updating local tracking refs; `--no-fetch` deliberately plans from cached tracking refs instead
- A stale branch, and each submitted descendant that must follow its temporary publish head, reports `evaluate_after_temporary_restack` because the final push decision depends on the rewritten commit ID
- Stack-link plans report `update_unless_native_link_succeeds` when native-stack success would suppress Stax-managed links
- Stack-link and native-stack plans report `evaluate_after_pr_discovery` when PRs missing from local metadata could change link or fork eligibility at runtime
- Native-stack plans otherwise report `skip` for known exclusions or `attempt` when the prerequisites are known
- `--draft` / `--publish` / `--no-pr` / `--no-fetch` / `--no-verify` / `--open` / `--quiet` / `--verbose`
- `--no-verify` (`-n`) skips pre-push hooks while pushing branches
- `--reviewers alice,bob --labels bug,urgent --assignees alice`
- `--squash` squash commits on each branch before pushing
- `--ai` generate PR title and body with AI; narrow with `--title` or `--body`
- `--template <name>` / `--no-template` / `--edit`
- `--rerequest-review` / `--update-title`
- `--native-stack` force-attempt native GitHub Stack registration for this submit; `--no-native-stack` skips it
- `--fork` if the upstream push is denied for lack of write access, fall back to submitting the branch from a fork (single branch, GitHub only)
- `--yes` / `--no-prompt`

Config: `[submit] stack_links = "comment" | "body" | "both" | "off"` and `native_stack = "auto" | "off" | "link"`; `[remote] auto_fork` and `[remote] fork_remote` control the fork fallback — see [Configuration → Fork fallback for submit](../configuration/index.md#fork-fallback-for-submit).

### `st completions`

Generate a completion script without requiring an initialized repository:

```bash
st completions bash
st completions zsh
st completions fish
st completions powershell
st completions elvish
```

### `st merge`

- `--dry-run` / `--yes`
- `--all` / `--downstack-only` (`--ds`) / `--stack` / `--stack --full` / `--method squash|merge|rebase`
- `--when-ready` · `--when-ready --interval 10`
- `--remote` · `--remote --all` · `--remote --timeout 60 --interval 10`
- `--queue` · `--queue --all --yes`
- `--timeout <minutes>` — positive whole minutes (default: 30); zero is rejected
- `--interval <seconds>` — positive whole seconds (default: 15) for `--when-ready`, `--remote`, `--queue`, and `--stack --when-ready`; zero is rejected
- `--no-wait` / `--no-sync` / `--no-delete` / `--quiet`
- `--ignore-failed-ci` — merge even when CI checks have failed; still blocks on draft, changes-requested, conflicts, and closed. Incompatible with `--queue`. Use when only optional checks failed and branch protection does not require them.

For `--queue`, the timeout is a real deadline: stax caps the final sleep to the remaining budget and does not start another forge status poll once the deadline is reached.

### `st sync` / `st rs`

- `--restack` · `--restack --auto-stash-pop`
- `--delete-upstream-gone`
- `--force` / `--safe` / `--continue` / `--quiet` / `--verbose`
- `--stash` / `--no-stash` — dirty-tree handling before sync starts: `--stash` auto-stashes without prompting (works with `--quiet`/`--json`; does NOT auto-confirm branch deletions); `--no-stash` fails on a dirty tree and overrides `--force`; the two conflict at parse time; the dirty-tree error message names `--stash`. In dry-run mode both flags are accepted but ignored (stderr warning emitted).
- **Interactive Sync plan** — applies to **`st sync` / `st rs`** only (not **`st refresh`**, which skips this prompt). After fetch, stax refreshes PR metadata, then prints one **Sync plan** (trunk moves, merged/upstream-gone branches with PR numbers when known, optional `--restack` preview) and asks how to proceed. When deletions are listed: continue and delete all (non-blocking), per-branch prompts, or cancel (`Aborted.`, no undo receipt). When only trunk/restack changes are pending: **Proceed with restack** / **Cancel**, or **Continue sync** / **Cancel sync** when no restack is requested. Skipped when `--quiet`, `--json`, `--force`, or when there is nothing to confirm.
- `--prune` — **deprecated**; accepted for compatibility and emits a stderr warning; use `--full` to fetch `--prune` all remote-tracking refs
- `--full` — classic `git fetch --prune <remote>` for all remote-tracking refs; unlike the default trunk-only fetch it does not pass `--no-tags`, so tags reachable from fetched refs are downloaded too. Slower on large repos.
- `--dry-run` (alias `--plan`) — read-only preview: probes the remote with ls-remote (no fetch, no FETCH_HEAD write), patches PR states in-memory, classifies the trunk transition, reports merged/upstream-gone candidates and per-branch disposition, previews the restack scope with conflict predictions; always exits 0. `--force`, `--auto-stash-pop`, `--full`, `--stash`, `--no-stash`, and `--verbose` are accepted but ignored (stderr warning emitted for each). `--continue` conflicts and is rejected by clap.
- `--json` — emit the sync result as a single JSON document on stdout (schema version 1). Implies non-interactive (`quiet=true`); does **not** imply `--force` (branches that need confirmation are recorded in `skipped_branches` and left intact). Conflicts with `--continue` (rejected by clap). Failures still emit the JSON envelope with `success: false` and exit non-zero. `--verbose` is ignored (stderr warning emitted). `--dry-run --json` emits `kind: "sync_plan"` with `dry_run: true` and always exits 0.
- JSON schema (`kind: "sync"`, `schema_version: 1`; action/kind strings are extensible — consumers should treat unknown values as forwards-compatible additions):

| Field | Type | Notes |
|---|---|---|
| `schema_version` | `number` | Always `1` |
| `kind` | `string` | `"sync"` (run) · `"sync_plan"` (dry-run) |
| `success` | `bool` | `false` on any error |
| `dry_run` | `bool` | `true` for `--dry-run --json` |
| `duration_ms` | `number` | Wall-clock time in milliseconds |
| `trunk.branch` | `string` | Local trunk name |
| `trunk.remote_ref` | `string` | Remote-tracking ref (e.g. `origin/main`) |
| `trunk.action` | `string` | `up_to_date` · `fast_forwarded` · `reset` · `diverged` · `failed` · `unknown` |
| `trunk.commits?` | `number` | Present on `fast_forwarded` |
| `trunk.files?` | `number` | Present on `fast_forwarded` |
| `trunk.additions?` | `number` | Present on `fast_forwarded` |
| `trunk.deletions?` | `number` | Present on `fast_forwarded` |
| `deleted_branches[]` | `array` | Absent when empty |
| `deleted_branches[].name` | `string` | Branch name |
| `deleted_branches[].category` | `string` | `merged` · `upstream_gone` |
| `deleted_branches[].scope` | `string` | `both` · `local` · `remote` |
| `deleted_branches[].tip?` | `string` | Tip SHA at deletion time |
| `deleted_branches[].metadata_deleted` | `bool` | Whether the metadata ref was also deleted |
| `skipped_branches[]` | `array` | Absent when empty; branches that needed confirmation but `--force` was not given |
| `skipped_branches[].name` | `string` | Branch name |
| `skipped_branches[].reason` | `string` | Why deletion was skipped (e.g. `"not confirmed"`) |
| `protected_branches[]` | `array` | Absent when empty; upstream-gone branches skipped due to unique local commits |
| `partially_merged[]` | `array` | Absent when empty; branches with a signal of merging but with uncommitted local commits |
| `partially_merged[].name` | `string` | Branch name |
| `partially_merged[].reason` | `string` | `pr_merged` · `pr_closed` · `history_merged` |
| `partially_merged[].pr_number?` | `number` | Present when a PR was detected |
| `partially_merged[].extra_commits` | `number` | Number of local commits beyond what was merged |
| `restacked_branches[]` | `array` | Absent when empty |
| `imported_branches_updated[]` | `array` | Absent when empty |
| `checkout_change?` | `object` | Present when the current branch changed during sync; `{from, to}` |
| `stash.stashed` | `bool` | Whether working tree was auto-stashed |
| `stash.restored` | `bool` | Whether the stash was successfully restored |
| `stash.left_stashed` | `bool` | `stashed && !restored` |
| `merged_candidates[]` | `array` | **`sync_plan` only.** Absent when empty; per-branch disposition for merged-branch candidates |
| `merged_candidates[].name` | `string` | Branch name |
| `merged_candidates[].disposition` | `string` | `would_delete` · `would_prompt_then_delete` · `would_keep_worktree` · `would_skip` · `would_rebase_children` |
| `merged_candidates[].scope?` | `string` | `both` · `local`; present when disposition is `would_delete` or `would_prompt_then_delete` |
| `merged_candidates[].keep_reason?` | `string` | Human-readable reason; present when disposition is `would_keep_worktree` or `would_skip` |
| `merged_candidates[].children?` | `array` | Child branch names; present when disposition is `would_rebase_children` |
| `upstream_gone_protected[]` | `array` | **`sync_plan` only.** Absent when empty; upstream-gone branches protected because they have unique local commits |
| `upstream_gone_deletable[]` | `array` | **`sync_plan` only.** Absent when empty; upstream-gone branches that would be deleted |
| `upstream_gone_deletable[].name` | `string` | Branch name |
| `upstream_gone_deletable[].disposition` | `string` | `would_delete` (with `--force`) or `would_prompt_then_delete` |
| `frozen_branches[]` | `array` | **`sync_plan` only.** Absent when empty; branches skipped because they are frozen |
| `branches_to_restack[]` | `array` | **`sync_plan` only.** Absent when empty; branches that would be restacked |
| `predicted_conflicts[]` | `array` | **`sync_plan` only.** Absent when empty; branches predicted to have merge conflicts during restack |
| `predicted_conflicts[].branch` | `string` | Branch that would conflict |
| `predicted_conflicts[].onto` | `string` | Parent branch it would rebase onto |
| `predicted_conflicts[].files` | `array` | Files predicted to conflict |
| `would_stash` | `bool` | **`sync_plan` only.** Whether the working tree is dirty (real sync would auto-stash) |
| `error?` | `object` | Present on failure; `{kind, message}` |
| `error.kind` | `string` | `dirty_working_tree` · `restack_conflict` · `error` |

- On early-bail paths (e.g. dirty working tree in non-interactive mode) and on restack-conflict paths where finalize never runs, `trunk.action` is `"unknown"` — this is intentional.
- Sync is transactional and undoable via `st undo`. A single receipt covers trunk fast-forwards, deleted branch heads, deleted metadata refs, reparented children's metadata, and the optional restack phase. A no-op sync (nothing changed) writes no receipt so the previous undoable operation remains on the undo stack.
- Imported branches from `st get` are remote-delete exempt: once they are detected as merged or upstream-gone, sync may delete the local support branch and metadata, but it will not push-delete the imported remote branch.
- The completion footer summarizes the trunk commit, file, and line delta together with non-zero merged-cleanup, imported-update, and restack counts. It reuses sync's existing results and does not perform extra network or Git work.
- When sync itself leaves exceptional work behind, it reports skipped cleanup with its reason, trunk update failures, and cleanup-driven checkout changes. It prints one prioritized next command: a diverged trunk gets non-destructive guidance to inspect and reconcile it with its remote; other trunk failures suggest `st trunk`; blocked cleanup suggests `st sweep`. Routine restack health remains visible in `st ls` and the TUI instead of appearing after every sync.
- When `--restack` is requested, sync fails closed if its fetch did not succeed or the local trunk did not reach the fetched remote-trunk commit. It restores any sync auto-stash and exits non-zero before imported-branch refresh, merged-branch cleanup, or restacking can rewrite feature refs. `st refresh` inherits this guard and exits before its submit phase, so it does not push or update PRs after either failure.
- Deletion output lines for locally deleted branches (merged or upstream-gone) append the branch's tip SHA (first 7 characters, dimmed) so you can reference the exact commit that was removed.
- If sync auto-stashed your working tree and then fails on an error path that cannot restore the stash, it prints a warning to stderr naming the stash "stax auto-stash" with instructions to run `git stash pop` to restore your changes.

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

Restack the stack and submit updates, without fetching trunk (offline-friendly).

- `--no-pr` / `--no-submit` / `--auto-stash-pop`

### `st checkout`

- `--trunk` / `--parent` / `--child 1`

### `st web`

- `st web` discovers the enclosing Git worktree from the current directory, starts its localhost HTMX workspace, and opens it in the browser.
- `st web <path>` accepts a repository or any path inside one and opens the enclosing worktree.
- `st web --port <n>` binds on a specific port (default 8787). `--port 0` picks an ephemeral port. If the requested port is busy, stax warns and uses a free OS-selected port.
- `st web --no-open` starts the server and prints the URL without opening the browser.
- Binds **127.0.0.1 only**; no `--host` flag is available.
- Each session URL embeds an unguessable 48-hex session token: `/s/<token>/`. Requests with a wrong or missing token return 404.
- Every mutating POST requires a matching CSRF token field; mismatches return 403.
- `Host` must be local. Requests without `Origin` are allowed, but a present `Origin` must exactly equal `http://127.0.0.1:<actual-bound-port>` (including an ephemeral or busy-port fallback port); cross-site, malformed, duplicate, and wrong-port values return 403. This check is independent of the session token and CSRF protections.
- Only one mutation runs at a time; mutating controls are disabled while an operation is in flight.
- All git operations run in `spawn_blocking` to avoid blocking the async Tokio runtime.
- Session state is in-memory; restarting the server generates a new token and URL.
- See [Web workspace guide](../interface/web.md) for the full routes reference, keyboard shortcuts, and layout details.



- With no argument, `st get` syncs and restacks the current stack, equivalent to the Graphite `gt get` current-stack flow.
- The argument may be a remote branch name, `origin/<branch>`, or a PR number when forge auth is configured.
- `--parent <branch>` records a non-trunk parent in stax metadata
- `--no-checkout` fetches and tracks without switching branches
- `--downstack` skips local upstack branches when the target already exists locally.
- `--remote-upstack` includes remote-only upstack PR branches discovered from open PR base/head metadata. This is best-effort without Graphite's central backend.
- `--no-restack` skips the default restack after checkout.
- `--unfrozen` unfreezes the requested branch before syncing it; frozen targets are otherwise skipped.
- Existing local branches fast-forward when possible, or rebase local-only commits onto the fetched remote tip when branch histories diverge.
- `--force` resets an existing local branch to the remote tip instead of preserving local commits.
- Branches checked out in another linked worktree are skipped instead of being moved from the current worktree.
- New remote-only branches imported by `st get` are read-only support branches: submit uses them as stack bases but does not push them or update their PRs. Existing Stax-managed branches keep their ownership metadata when synced with `st get`.
- `st sync --restack` refreshes imported branches from their remote tips before restacking descendants; if an imported branch is checked out in a dirty worktree, sync skips it unless `--force` is used.

### `st run`

- `--parallel` uses detached temporary worktrees, so the main worktree never changes branch.
- `--jobs <N>` sets the positive concurrency cap (default 8) and requires `--parallel`.
- Each parallel command receives `STAX_RUN_BRANCH` with the original logical branch name; Git itself remains on a detached HEAD inside the temporary worktree.
- Output is captured concurrently and printed in deterministic branch order.
- Clean temporary worktrees are removed after success or failure. If a command leaves uncommitted tracked or untracked changes, that worktree is preserved and its recovery path is printed; the branch is counted as failed. Ignored artifacts do not prevent cleanup.
- `--parallel` conflicts with `--fail-fast`, because commands may already be running concurrently.

### `st ci`

- `--stack` / `--all` / `--oneline` (`-1`) / `--verbose` / `--watch` / `--watch --strict` / `--interval 30` / `--json`
- For tracked PRs, CI is fetched for the forge's live PR head, so commits added remotely by formatters or other automation are reflected without moving the local branch. If the live PR revision cannot be resolved, stax falls back to the local branch revision.
- CI status is always fetched live. `--refresh` remains accepted for compatibility but does not change the fetch behavior.
- On GitHub, all commit-status and check-run pages are collected before the latest result for each check is selected and the overall state is calculated.
- Three render modes: the **full per-check table** (single branch, default), grouped **summary cards** (`--verbose`/`-v`), and the **one-line roll-up** (`--oneline`/`-1`). Any multi-branch view (`--stack`/`--all`) defaults to the roll-up; `--verbose` overrides it back to cards. `--oneline` and `--verbose` cannot be combined.
- The roll-up renders one line per branch, base→tip: CI status icon · branch · `#PR` · `draft`/`ready` · PR title · trailing check-count and timing. A bare `--oneline` defaults its scope to the current stack.
- By default, `--watch` waits until every check is terminal, even if one check has already failed. Add `--strict` to exit as soon as any check fails.
- `--watch --alert` plays built-in success/error sounds; `--watch --alert <file>` uses one custom sound for either outcome; `--watch --no-alert` suppresses `[ci] alert = true` for one run.
- Config can enable alerts by default with `[ci] alert = true`; set `success_alert_sound` and/or `error_alert_sound` to override the per-outcome built-in sounds.

### `st standup`

- `--all` / `--hours 48` / `--json` / `--ci`
- `--ai` · `--ai --agent claude` · `--ai --hours 48`
- `--ai --style slack`
- `--ai --plain-text` / `--ai --json` / `--ai --jit`
- `--ci` checks only the selected branches and may add network latency; combine it with `--all` to check all tracked branches.
- GitHub authored reviews come from one time-bounded, maximum-100 GraphQL query. GitLab and Gitea mark that signal unsupported rather than scanning every MR/PR.
- JSON preserves `reviews_given` and `needs_attention.ci_failing`; use `signals.<name>.status` (`available`, `unsupported`, `unavailable`, or `not_requested`) to interpret empty arrays.

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
