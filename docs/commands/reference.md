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
| `st refresh --yes --no-prompt` | | Run the full refresh flow without submit title/body/draft prompts |
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
| `st modify` | `m` | Amend staged changes into current commit (`-a` stages all, `-r` restacks after) |
| `st rename` | | Rename current branch |
| `st branch track` | | Track an existing branch |
| `st branch track --all-prs` | | Track all open PRs (GitHub, GitLab, Gitea) |
| `st branch untrack` | `ut` | Remove stax metadata |
| `st branch reparent` | | Change parent |
| `st branch submit` | `bs` | Submit current branch only |
| `st branch delete` | | Delete branch |
| `st branch fold` | | Fold branch into parent |
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
| `st ci --stack` / `--all` / `--watch` | Scope and watch modes |
| `st ci --verbose` / `--json` | Summary cards · JSON output |
| `st pr` · `st pr open` | Open current branch PR |
| `st pr list` | List open PRs (GitHub, GitLab, Gitea) |
| `st issue list` | List open issues |
| `st comments` | Show PR comments |
| `st copy` · `st copy --pr` | Copy branch name · PR URL |
| `st standup` | Recent activity (`--summary` for AI spoken version; `--jit` for Jira context) |
| `st changelog [from] [to]` | Generate changelog (auto-resolves last tag when `from` omitted) |
| `st generate --pr-body` | Generate PR body with AI |

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
- `--insert` reparent children of the current branch onto the new branch
- `st branch create --message "msg" --prefix feature/`

### `st status` / `st ll` / `st log`

- `--stack <branch>` · `--current` · `--compact` · `--json` · `--quiet`

### `st submit`

- `--draft` / `--publish` / `--no-pr` / `--no-fetch` / `--open` / `--quiet` / `--verbose`
- `--reviewers alice,bob --labels bug,urgent --assignees alice`
- `--squash` squash commits on each branch before pushing
- `--ai-body` generate PR body with AI
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

- `--stack` / `--all` / `--watch` / `--interval 30` / `--json`

### `st standup`

- `--all` / `--hours 48` / `--json`
- `--summary` · `--summary --agent claude` · `--summary --hours 48`
- `--summary --plain-text` / `--summary --json` / `--summary --jit`

### `st pr` / `st issue`

- `st pr list --limit 50 --json`
- `st issue list --limit 50 --json`

### `st generate --pr-body`

- `--template <name>` / `--no-template` / `--no-prompt` / `--edit`
- `--agent <name>` / `--model <name>`

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
