# Full Command Reference

## Stack operations

| Command | Alias | Description |
|---|---|---|
| `stax status` | `s`, `ls` | Show stack |
| `stax log` | `l` | Show stack with commits and PR info |
| `stax submit` | `ss` | Submit full current stack |
| `stax merge` | | Merge PRs bottom -> current |
| `stax sync` | `rs` | Pull trunk, delete merged branches |
| `stax restack` | | Rebase current branch onto parent |
| `stax diff` | | Show per-branch diffs vs parent |
| `stax range-diff` | | Show range-diff for branches needing restack |

## Branch management

| Command | Alias | Description |
|---|---|---|
| `stax create <name>` | `c`, `bc` | Create stacked branch |
| `stax checkout` | `co`, `bco` | Interactive branch picker |
| `stax modify` | `m` | Stage all and amend current commit |
| `stax rename` | `b r` | Rename branch |
| `stax branch track` | | Track existing branch |
| `stax branch track --all-prs` | | Track all open PRs |
| `stax branch untrack` | `ut` | Remove stax metadata |
| `stax branch reparent` | | Change parent |
| `stax branch submit` | `bs` | Submit current branch only |
| `stax branch delete` | | Delete branch |
| `stax branch fold` | | Fold branch into parent |
| `stax branch squash` | | Squash commits |
| `stax upstack restack` | | Restack current + descendants |
| `stax upstack submit` | | Submit current + descendants |
| `stax downstack submit` | | Submit ancestors + current |

## Interactive

| Command | Description |
|---|---|
| `stax` | Launch TUI |
| `stax split` | Split branch into stacked branches |

## Recovery

| Command | Description |
|---|---|
| `stax undo` | Undo last operation |
| `stax undo <op-id>` | Undo specific operation |
| `stax redo` | Re-apply last undone operation |

## Utilities

| Command | Description |
|---|---|
| `stax auth` | Configure GitHub token |
| `stax auth status` | Show active auth source |
| `stax config` | Show current configuration |
| `stax doctor` | Check repo health |
| `stax continue` | Continue after conflicts |
| `stax pr` | Open current branch PR |
| `stax ci` | Show CI status |
| `stax comments` | Show PR comments |
| `stax copy` | Copy branch name |
| `stax copy --pr` | Copy PR URL |
| `stax standup` | Show recent activity |
| `stax changelog <from> [to]` | Generate changelog |
| `stax generate --pr-body` | Generate PR body with AI |

## Common flags

- `stax create -am "msg"`
- `stax submit --draft --yes --no-prompt`
- `stax submit --template <name>`
- `stax submit --no-template`
- `stax submit --edit`
- `stax merge --all --method squash --yes`
- `stax merge --dry-run`
- `stax merge --no-wait`
- `stax rs --restack --auto-stash-pop`
- `stax cascade --no-pr`
- `stax cascade --no-submit`
- `stax status --json`
- `stax undo --yes --no-push`
