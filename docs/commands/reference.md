# Full Command Reference

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `stax status` | `s`, `ls` | Show stack |
| `stax ll` | | Show stack with PR URLs and full details |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax submit` | `ss` | Submit full current stack |
| `stax merge` | | Merge PRs bottom -> current, then sync local repo (`--no-sync` to skip) |
| `stax merge-when-ready` | `mwr` | Backward-compatible alias for `stax merge --when-ready` |
| `stax sync` | `rs` | Pull trunk, delete merged branches, preserve child provenance for restack |
| `stax restack` | | Rebase current branch onto parent (provenance-aware `--onto` when possible) |
| `stax cascade` | | Restack from bottom and submit updates |
| `stax diff` | | Show per-branch diffs vs parent |
| `stax range-diff` | | Show range-diff for branches needing restack |

## Navigation

| Command | Alias | Description |
|---|---|---|
| `stax checkout` | `co`, `bco` | Interactive branch picker |
| `stax trunk` | `t` | Switch to trunk |
| `stax up [n]` | `u` | Move up to child branch |
| `stax down [n]` | `d` | Move down to parent branch |
| `stax top` | | Move to stack tip |
| `stax bottom` | | Move to stack base |
| `stax prev` | `p` | Switch to previous branch |

## Branch management and scopes

| Command | Alias | Description |
|---|---|---|
| `stax create <name>` | `c`, `bc` | Create stacked branch |
| `stax modify` | `m` | Stage all and amend current commit |
| `stax rename` | | Rename current branch |
| `stax branch track` | | Track existing branch |
| `stax branch track --all-prs` | | Track all open PRs |
| `stax branch untrack` | `ut` | Remove stax metadata |
| `stax branch reparent` | | Change parent |
| `stax branch submit` | `bs` | Submit current branch only |
| `stax branch delete` | | Delete branch |
| `stax branch fold` | | Fold branch into parent |
| `stax branch squash` | | Squash commits |
| `stax detach` | | Remove branch from stack, reparent children |
| `stax reorder` | | Interactively reorder branches in stack |
| `stax upstack restack` | | Restack current + descendants |
| `stax upstack submit` | | Submit current + descendants |
| `stax downstack get` | | Show branches below current |
| `stax downstack submit` | | Submit ancestors + current |

## Interactive

| Command | Description |
|---|---|
| `stax` | Launch TUI |
| `stax split` | Split branch into stacked branches |

## Recovery

| Command | Description |
|---|---|
| `stax abort` | Abort in-progress rebase/conflict resolution |
| `stax undo` | Undo last operation |
| `stax undo <op-id>` | Undo specific operation |
| `stax redo` | Re-apply last undone operation |

## Health & Testing

| Command | Description |
|---|---|
| `stax validate` | Validate stack metadata (orphans, cycles, staleness) |
| `stax fix` | Auto-repair broken metadata |
| `stax stack fix --dry-run` | Preview fixes without applying |
| `stax test <cmd>` | Run a command on each branch in the stack |
| `stax test <cmd> --fail-fast` | Stop after first failure |
| `stax test <cmd> --all` | Run on all tracked branches |

## Utilities

| Command | Description |
|---|---|
| `stax auth` | Configure GitHub token |
| `stax auth status` | Show active auth source |
| `stax config` | Show current configuration |
| `stax doctor` | Check repo health |
| `stax continue` | Continue after conflicts |
| `stax pr` | Open current branch PR |
| `stax open` | Open repository in browser |
| `stax ci` | Show CI status for current branch (full per-check table with ETA) |
| `stax ci --stack` | Show CI status for all branches in current stack |
| `stax ci --all` | Show CI status for all tracked branches |
| `stax ci --watch` | Watch CI until completion, polls every 15s |
| `stax ci --watch --interval 30` | Watch with custom polling interval (seconds) |
| `stax ci --verbose` | Compact summary cards instead of full table |
| `stax ci --json` | Output CI status as JSON |
| `stax comments` | Show PR comments |
| `stax copy` | Copy branch name |
| `stax copy --pr` | Copy PR URL |
| `stax standup` | Show recent activity |
| `stax changelog <from> [to]` | Generate changelog |
| `stax generate --pr-body` | Generate PR body with AI |
| `stax demo` | Interactive tutorial (no auth/repo needed) |

## Common flags

- `stax create -am "msg"`
- `stax branch create --message "msg" --prefix feature/`
- `stax branch reparent --branch feature-a --parent main`
- `stax branch rename --push`
- `stax branch squash --message "Squashed commit"`
- `stax branch fold --keep`
- `stax status --stack <branch> --current --compact --json --quiet`
- `stax ll --stack <branch> --current --compact --json --quiet`
- `stax log --stack <branch> --current --compact --json --quiet`
- `stax submit --draft --yes --no-prompt`
- `stax submit --no-pr`
- `stax submit --no-fetch`
- `stax submit --open`
- `stax submit --force`
- `stax submit --reviewers alice,bob --labels bug,urgent --assignees alice`
- `stax submit --quiet`
- `stax submit --verbose`
- `stax submit --ai-body`
- `stax submit --template <name>`
- `stax submit --no-template`
- `stax submit --edit`
- `stax submit --rerequest-review`
- `stax merge --all --method squash --yes`
- `stax merge --dry-run`
- `stax merge --when-ready`
- `stax merge --when-ready --interval 10`
- `stax merge --no-wait`
- `stax merge --no-sync`
- `stax merge --timeout 60 --no-delete --quiet`
- `stax rs --restack --auto-stash-pop`
- `stax sync --force --safe --continue`
- `stax sync --quiet`
- `stax sync --verbose`
- `stax restack --all --continue --quiet`
- `stax restack --submit-after ask|yes|no`
- `stax cascade --no-pr`
- `stax cascade --no-submit`
- `stax checkout --trunk`
- `stax checkout --parent`
- `stax checkout --child 1`
- `stax ci --stack --watch --interval 30 --json`
- `stax standup --all --hours 48 --json`
- `stax auth --from-gh`
- `stax auth --token <token>`
- `stax undo --yes --no-push`
- `stax undo --quiet`
- `stax redo --yes --no-push --quiet`
