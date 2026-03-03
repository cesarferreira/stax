# Full Command Reference

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `st status` | `s`, `ls` | Show stack |
| `st ll` | | Show stack with PR URLs and full details |
| `st log` | `l` | Show stack with commits and PR info |
| `st submit` | `ss` | Submit full current stack |
| `st merge` | | Merge PRs bottom -> current with provenance-aware descendant rebases, then sync local repo (`--no-sync` to skip) |
| `st merge-when-ready` | `mwr` | Backward-compatible alias for `st merge --when-ready` |
| `st sync` | `rs` | Pull trunk, delete merged branches, preserve child provenance for restack |
| `st sync --delete-upstream-gone` | | Also delete local branches whose upstream tracking ref is gone |
| `st restack` | | Rebase current branch onto parent; auto-normalize missing/merged-equivalent parents and use provenance-aware `--onto` when possible |
| `st cascade` | | Restack from bottom and submit updates |
| `st diff` | | Show per-branch diffs vs parent |
| `st range-diff` | | Show range-diff for branches needing restack |

## Navigation

| Command | Alias | Description |
|---|---|---|
| `st checkout` | `co`, `bco` | Interactive branch picker |
| `st trunk` | `t` | Switch to trunk |
| `st up [n]` | `u` | Move up to child branch |
| `st down [n]` | `d` | Move down to parent branch |
| `st top` | | Move to stack tip |
| `st bottom` | | Move to stack base |
| `st prev` | `p` | Switch to previous branch |

## Branch management and scopes

| Command | Alias | Description |
|---|---|---|
| `st create <name>` | `c`, `bc` | Create stacked branch |
| `st modify` | `m` | Stage all and amend current commit |
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
| `st test <cmd>` | Run a command on each branch in the stack |
| `st test <cmd> --fail-fast` | Stop after first failure |
| `st test <cmd> --all` | Run on all tracked branches |

## Utilities

| Command | Description |
|---|---|
| `st auth` | Configure GitHub token |
| `st auth status` | Show active auth source |
| `st config` | Show current configuration |
| `st doctor` | Check repo health |
| `st continue` | Continue after conflicts |
| `st pr` | Open current branch PR |
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
| `st standup --summary --jit` | Include Jira `jit` context for in-flight and next-up work |
| `st changelog <from> [to]` | Generate changelog |
| `st generate --pr-body` | Generate PR body with AI |
| `st demo` | Interactive tutorial (no auth/repo needed) |

## Agent worktrees

| Command | Alias | Description |
|---------|-------|-------------|
| `st agent create <title>` | `ag create` | Create worktree + stacked branch |
| `st agent open [name]` | `ag attach` | Reopen in editor (fuzzy picker if no name) |
| `st agent list` | `ag ls` | Show all registered worktrees |
| `st agent register` | | Register current dir as an agent worktree |
| `st agent remove [name]` | | Remove worktree (+ `--delete-branch` to delete branch) |
| `st agent prune` | | Remove dead registry entries + `git worktree prune` |
| `st agent sync` | | Restack all registered worktrees |

## Common flags

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
- `st merge --all --method squash --yes`
- `st merge --dry-run`
- `st merge --when-ready`
- `st merge --when-ready --interval 10`
- `st merge --no-wait`
- `st merge --no-sync`
- `st merge --timeout 60 --no-delete --quiet`
- `st rs --restack --auto-stash-pop`
- `st sync --delete-upstream-gone`
- `st sync --force --safe --continue`
- `st sync --quiet`
- `st sync --verbose`
- `st restack --all --continue --quiet`
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
- `st auth --from-gh`
- `st auth --token <token>`
- `st undo --yes --no-push`
- `st undo --quiet`
- `st redo --yes --no-push --quiet`
