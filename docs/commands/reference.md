# Full Command Reference

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `st status` | `s`, `ls` | Show stack |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack |
| `st merge` | | Merge PRs bottom -> current with provenance-aware descendant rebases, then sync local repo (`--no-sync` to skip); `st merge --remote` merges via GitHub API only (no local git; GitHub-only) |
| `st merge-when-ready` | `mwr` | Backward-compatible alias for `st merge --when-ready` |
| `st sync` | `rs` | Pull trunk from remote, detect and delete merged branches (incl. squash merges), reparent their children — **no rebasing** |
| `st sync --restack` | `rs --restack` | Everything `sync` does, **then** rebase the current stack onto updated parents |
| `st sync --delete-upstream-gone` | | Also delete local branches whose upstream tracking ref is gone |
| `st restack` | | Rebase current stack onto parents (local only, no fetch/delete) — auto-normalizes missing or merged parents before rebasing; `--stop-here` limits scope to ancestors + current |
| `st cascade` | | Restack from bottom and submit updates |
| `st diff` | | Show per-branch diffs vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |

## Navigation

| Command | Alias | Description |
|---|---|---|
| `st checkout` | `co`, `bco` | Interactive branch picker |
| `st trunk` | `t` | Switch to trunk (or `st trunk <branch>` to set trunk) |
| `st up [n]` | `u` | Move up to child branch |
| `st down [n]` | `d` | Move down to parent branch |
| `st top` | | Move to stack tip |
| `st bottom` | | Move to stack base |
| `st prev` | `p` | Switch to previous branch |

## Branch management and scopes

| Command | Alias | Description |
|---|---|---|
| `st create <name>` | `c`, `bc` | Create stacked branch |
| `st modify` | `m` | Amend staged changes into current commit; prompts when nothing is staged (`-a` to stage all); on a fresh tracked branch, `-m` creates the first commit safely |
| `st rename` | | Rename current branch |
| `st branch track` | | Track existing branch |
| `st branch track --all-prs` | | Track all open PRs |
| `st branch untrack` | `ut` | Remove stax metadata |
| `st branch reparent` | | Change parent |
| `st branch submit` | `bs` | Submit current branch only |
| `st branch delete` | | Delete branch |
| `st branch fold` | | Fold branch into parent |
| `st branch squash` | | Squash commits |
| `st detach` | | Remove branch from stack, reparent children |
| `st reorder` | | Interactively reorder branches in stack |
| `st upstack restack` | | Restack current + descendants |
| `st upstack submit` | | Submit current + descendants |
| `st downstack get` | | Show branches below current |
| `st downstack submit` | | Submit ancestors + current |

## Interactive

| Command | Description |
|---|---|
| `st` | Launch TUI |
| `st split` | Split branch into stacked branches |

## Recovery

| Command | Description |
|---|---|
| `st resolve` | Resolve in-progress rebase conflicts using AI |
| `st abort` | Abort in-progress rebase/conflict resolution |
| `st undo` | Undo last operation |
| `st undo <op-id>` | Undo specific operation |
| `st redo` | Re-apply last undone operation |

## Health & Testing

| Command | Description |
|---|---|
| `st validate` | Validate stack metadata (orphans, cycles, staleness) |
| `st fix` | Auto-repair broken metadata |
| `st fix --dry-run` | Preview fixes without applying |
| `st run <cmd>` | Run a command on each branch in the stack (alias: `st test <cmd>`) |
| `st run <cmd> --stack[=<branch>]` | Run only one stack (current stack by default, or `<branch>` stack when provided) |
| `st run <cmd> --fail-fast` | Stop after first failure |
| `st run <cmd> --all` | Run on all tracked branches |

## Utilities

| Command | Description |
|---|---|
| `st auth` | Configure GitHub token |
| `st auth status` | Show active auth source |
| `st config` | Show current configuration |
| `st config --reset-ai` | Clear saved AI defaults, then re-prompt interactively |
| `st config --reset-ai --no-prompt` | Clear saved AI defaults without reopening the picker |
| `st init` | Initialize stax or reconfigure the repo trunk interactively |
| `st init --trunk <branch>` | Set the repo trunk directly |
| `st doctor` | Check repo health |
| `st continue` | Continue after conflicts |
| `st pr` | Open current branch PR |
| `st pr open` | Explicit form of `st pr` |
| `st pr list` | List open pull requests in the current repo |
| `st issue list` | List open issues in the current repo |
| `st open` | Open repository in browser |
| `st ci` | Show CI status for current branch (full per-check table with ETA) |
| `st ci --stack` | Show CI status for all branches in current stack |
| `st ci --all` | Show CI status for all tracked branches |
| `st ci --watch` | Watch CI until completion, polls every 15s |
| `st ci --watch --interval 30` | Watch with custom polling interval (seconds) |
| `st ci --verbose` | Compact summary cards instead of full table |
| `st ci --json` | Output CI status as JSON |
| `st comments` | Show PR comments |
| `st copy` | Copy branch name |
| `st copy --pr` | Copy PR URL |
| `st standup` | Show recent activity |
| `st standup --summary` | AI-generated spoken standup update |
| `st standup --summary --jit` | Include Jira `jit` context for in-flight and next-up work ([jit repo](https://github.com/cesarferreira/jit)) |
| `st changelog [from] [to]` | Generate changelog (auto-resolves last tag if `from` omitted) |
| `st generate --pr-body [--no-prompt]` | Generate PR body with AI |
| `st demo` | Interactive tutorial (no auth/repo needed) |

## Developer worktrees

| Command | Alias | Description |
|---------|-------|-------------|
| `st worktree` | `wt` | Open the interactive worktree dashboard in a TTY; otherwise print worktree help |
| `st worktree create [name]` | `wt c`, `wtc` | Create or reuse a worktree lane (`wt c` with no args generates a random lane name) |
| `st worktree list` | `wt ls`, `w`, `wtls` | List all worktrees |
| `st worktree ll` | `wt ll` | Show richer worktree status, including managed/prunable/conflict state |
| `st worktree go [name]` | `wt go`, `wtgo` | Navigate to a worktree (picker if no name; requires shell integration for transparent `cd`) |
| `st worktree path <name>` | | Print absolute path of a worktree (for scripting) |
| `st worktree remove [name]` | `wt rm`, `wtrm` | Remove a worktree (`wt rm` with no name removes the current lane) |
| `st worktree prune` | `wt prune`, `wtprune` | Clean stale git worktree bookkeeping only |
| `st worktree restack` | `wt rs`, `wtrs` | Restack all stax-managed worktrees |
| `st shell-setup` | | Print shell integration snippet for manual install |
| `st shell-setup --install` | | Write shell integration under `~/.config/stax/` and source it from your shell config |

Worktree launch examples:
- `st wt c review-pass --agent codex --tmux -- "address PR comments"`
- `st wt go review-pass --agent codex --tmux`
- `st wt go ui-polish --run "cursor ." --tmux`

## Common flags

- `st modify -a` (stage all and amend, old default behavior)
- `st modify -am "msg"` (stage all and amend with new message)
- `st create -am "msg"`
- `st branch create --message "msg" --prefix feature/`
- `st branch reparent --branch feature-a --parent main`
- `st branch rename --push`
- `st branch squash --message "Squashed commit"`
- `st branch fold --keep`
- `st status --stack <branch> --current --compact --json --quiet`
- `st ll --stack <branch> --current --compact --json --quiet`
- `st log --stack <branch> --current --compact --json --quiet`
- `st submit --draft --yes --no-prompt`
- `st submit --no-pr`
- `st submit --no-fetch`
- `st submit --open`
- `st submit --force`
- `st submit --reviewers alice,bob --labels bug,urgent --assignees alice`
- `st submit --quiet`
- `st submit --verbose`
- `st submit --ai-body`
- `st submit --template <name>`
- `st submit --no-template`
- `st submit --edit`
- `st submit --rerequest-review`
- `~/.config/stax/config.toml`: set `[submit] stack_links = "comment"` (or `"body" | "both" | "off"`)
- `st merge --all --method squash --yes`
- `st merge --dry-run`
- `st merge --when-ready`
- `st merge --when-ready --interval 10`
- `st merge --remote`
- `st merge --remote --all --method squash --yes`
- `st merge --no-wait`
- `st merge --no-sync`
- `st merge --timeout 60 --no-delete --quiet`
- `st rs --restack --auto-stash-pop`
- `st sync --delete-upstream-gone`
- `st sync --force --safe --continue`
- `st sync --quiet`
- `st sync --verbose`
- `st restack --all --continue --quiet`
- `st restack --stop-here`
- `st restack --submit-after ask|yes|no`
- `st resolve --agent codex --model gpt-5.3-codex --max-rounds 5`
- `st cascade --no-pr`
- `st cascade --no-submit`
- `st checkout --trunk`
- `st checkout --parent`
- `st checkout --child 1`
- `st ci --stack --watch --interval 30 --json`
- `st standup --all --hours 48 --json`
- `st standup --summary`
- `st standup --summary --agent claude`
- `st standup --summary --hours 48`
- `st standup --summary --plain-text`
- `st standup --summary --json`
- `st standup --summary --jit`
- `st pr list --limit 50 --json`
- `st issue list --limit 50 --json`
- `st changelog --tag-prefix release/ios`
- `st changelog --json`
- `st changelog --path src/`
- `st auth --from-gh`
- `st auth --token <token>`
- `st init --trunk main`
- `st undo --yes --no-push`
- `st undo --quiet`
- `st redo --yes --no-push --quiet`
