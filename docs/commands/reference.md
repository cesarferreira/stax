# Full command reference

The complete command surface. For day-to-day commands only, see [Core commands](core.md). For navigation specifically, see [Navigation](navigation.md).

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `st status` | `s`, `ls` | Show stack |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack |
| `st merge` | | Cascade-merge from bottom to current (see flags below) |
| `st merge-when-ready` | `mwr` | Backward-compatible alias for `st merge --when-ready` |
| `st sync` | `rs` | Pull trunk, delete merged branches (incl. squash merges), reparent children |
| `st sync --restack` | `rs --restack` | `sync` **plus** rebase current stack onto updated parents |
| `st sync --delete-upstream-gone` | | Also delete local branches whose upstream tracking ref is gone |
| `st refresh` | | `sync --restack`, then push and create/update PRs for the current stack |
| `st refresh --force --yes --no-prompt` | | Run the full refresh flow without sync or submit prompts |
| `st refresh --verbose` | | Same as `st refresh`, with detailed sync/restack/submit timing |
| `st restack` | | Rebase current stack locally — auto-normalizes missing/merged parents; `--stop-here` limits scope |
| `st cascade` | | Restack from bottom and submit updates |
| `st diff` | | Show per-branch diffs vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |

### `st merge` variants

- `st merge` — local cascade merge with provenance-aware descendant rebases, then `st rs --force` unless `--no-sync`
- `st merge --when-ready` — wait for CI + approvals + mergeability; incompatible with `--dry-run`, `--no-wait`, `--remote`
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
| `st create <name>` | `c`, `bc` | Create stacked branch (TTY menu when nothing staged and `-m`) |
| `st create --ai` | | Generate a branch name from local changes (`-a` also generates a first commit message) |
| `st create <name> --below` | | Insert a new branch below current |
| `st modify` | `m` | Amend staged changes into current commit (`-a` stages all, `-r` restacks after) |
| `st rename` | | Rename current branch |
| `st branch track` | | Track an existing branch |
| `st branch track --all-prs` | | Track all open PRs (GitHub, GitLab, Gitea) |
| `st branch untrack` | `ut` | Remove stax metadata |
| `st branch reparent` | | Change parent |
| `st branch submit` | `bs` | Submit current branch only |
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
| `st upstack submit` | Submit current + descendants |
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
| `st ci` | CI status for current branch (with elapsed/ETA learned from recent runs) |
| `st ci --stack` / `--all` / `--watch` | Scope and watch modes (`--watch --strict` fail-fasts on failure) |
| `st ci -w --alert` / `--alert <file>` / `--no-alert` | Success/error completion sounds for watch mode |
| `st ci --verbose` / `--json` | Summary cards · JSON output |
| `st pr` · `st pr open` | Open current branch PR |
| `st pr list` | List open PRs (GitHub, GitLab, Gitea) |
| `st issue list` | List open issues |
| `st comments` | Show PR comments |
| `st copy` · `st copy --pr` | Copy branch name · PR URL |
| `st standup` | Recent activity (`--ai` for AI spoken version; `--jit` for Jira context) |
| `st changelog [from] [to]` | Generate changelog (auto-resolves last tag when `from` omitted) |
| `st generate` · `st gen` | AI generation: interactive picker, or `--pr-body` / `--pr-title` / `--commit-msg` |
| `st ss --ai` | Submit with AI-generated PR title/body suggestions |

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
| `st continue` | Continue after conflicts |
| `st open` | Open repository in browser |
| `st demo` | Interactive tutorial — no auth or repo required |

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

- `-m "msg"` set commit message (with nothing staged in a TTY: menu for stage all, `--patch`, empty branch, or abort)
- `-am "msg"` stage all and commit
- `--ai` generate missing branch name and/or first commit message from local changes
- `--ai -a --yes` stage all changes, generate branch name + commit message, and skip AI value review prompts
- `st create <name> --ai -a` keeps `<name>` and generates the first commit message
- `st create --ai -m "msg"` keeps the commit message and generates the branch name
- `-n`, `--no-verify` skip pre-commit and commit-msg hooks when creating a commit
- `-m` / `-am` create the commit before creating the destination branch, including with `--from` and `--below`, so hook failures or interrupts do not leave orphan branches
- `--insert` reparent children of the current branch onto the new branch
- `--below` create from the current branch's parent and reparent the current branch onto the new branch; combines with `-m`/`-am` to commit on the new lower branch
- `st branch create --message "msg" --prefix feature/`

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
- `--all` / `--method squash|merge|rebase`
- `--when-ready` · `--when-ready --interval 10`
- `--remote` · `--remote --all` · `--remote --timeout 60 --interval 10`
- `--queue` · `--queue --all --yes`
- `--no-wait` / `--no-sync` / `--no-delete` / `--timeout 60` / `--quiet`

### `st sync` / `st rs`

- `--restack` · `--restack --auto-stash-pop`
- `--delete-upstream-gone`
- `--force` / `--safe` / `--continue` / `--quiet` / `--verbose`

### `st restack`

- `--all` / `--continue` / `--quiet`
- `--stop-here`
- `--submit-after ask|yes|no`

### `st resolve`

- `--agent codex --model gpt-5.3-codex --max-rounds 5`

### `st cascade`

- `--no-pr` / `--no-submit` / `--auto-stash-pop`

### `st checkout`

- `--trunk` / `--parent` / `--child 1`

### `st ci`

- `--stack` / `--all` / `--watch` / `--watch --strict` / `--interval 30` / `--json`
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
