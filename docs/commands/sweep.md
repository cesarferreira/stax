# stax sweep

Classify all local branches and optionally delete selected categories.

## Why sweep?

Over time a repo accumulates branches that were merged, whose PR was closed without merging, or that were abandoned as work-in-progress. `stax sync` already cleans up **stax-tracked** merged branches during `rs` and reports closed PR branches without deleting them, but it ignores untracked branches and has no read-only listing mode.

`stax sweep` fills both gaps:

- Operates on **every** local branch (tracked and untracked).
- Lists branches grouped by status so you can see what's accumulating.
- Deletes only what you explicitly ask for, never touching trunk or the current branch.

## Branch statuses

| Status | Meaning |
|---|---|
| `merged` | Ancestor of trunk, patch-equivalent to trunk, PR metadata says merged, or a stax-tracked PR branch with no known closed state has a deleted upstream |
| `closed-pr` | Tracked PR recorded as closed without merging; retained unless deletion explicitly includes closed PRs |
| `upstream-gone` | Remote tracking ref is `[gone]` and the branch has no commits unique to local or remote trunk |
| `stale` | Last commit older than the configured threshold (default 30 days) |
| `active` | Everything else |

Precedence when a branch matches multiple: **integrated > closed-pr > safe upstream-gone > stale > active**. A closed PR whose commits reached trunk is integrated and classified `merged`. A deleted remote head alone does not make a known closed PR merged. Ordinary upstream-gone branches with unique commits are treated as active.

## Usage

```bash
# Read-only: classify all local branches and print a grouped summary
stax sweep

# Delete merged branches and upstream-gone branches with no unique work, with confirmation
stax sweep --delete

# Explicitly discard branches with closed, unmerged PRs
stax sweep --delete --include-closed

# Also include stale branches in the deletion set
stax sweep --delete --include-stale

# Skip the confirmation prompt
stax sweep --delete --force

# Override the stale threshold
stax sweep --stale-days 60

# Machine-readable output (conflicts with --delete)
stax sweep --json
```

## Flags

| Flag | Description |
|---|---|
| `--delete` | Delete merged branches, tracked merged PR branches, and upstream-gone branches with no unique work after confirmation; retain closed PR branches |
| `--include-stale` | Extend deletion to stale branches (requires `--delete`) |
| `--include-closed` | Extend deletion to closed, unmerged PR branches (requires `--delete`) |
| `--force` | Skip confirmation prompt (requires `--delete`) |
| `--stale-days <N>` | Override stale threshold in days (default: 30) |
| `--json` | Output classification as JSON; conflicts with `--delete` |

## Configuration

Set the stale threshold globally in `~/.config/stax/config.toml`:

```toml
[branch]
stale_days = 60
```

`--stale-days` overrides this per-run. See the [configuration reference](../configuration/index.md#stale-branch-threshold) for details.

## Safety

- Trunk and the current branch are always excluded.
- `--delete` without `--include-closed` retains closed-but-unmerged PR branches even when their remote head is gone. `--force` only skips confirmation; it does not include them.
- `--delete` without `--include-stale` never touches stale branches.
- Ordinary upstream-gone branches with commits not reachable from local or remote trunk are classified as active and are not deleted by `--delete`.
- Stax-tracked children of deleted branches are reparented to trunk before deletion so `stax status` stays clean.
- `--json` is always read-only (conflicts with `--delete`).

## JSON output

`stax sweep --json` emits a JSON object with a `branches` array:

```json
{
  "branches": [
    { "name": "feature/old-stuff", "status": "merged", "tracked": true },
    { "name": "feature/abandoned", "status": "closed-pr", "tracked": true },
    { "name": "experiment-2024", "status": "stale", "tracked": false, "days_old": 47 },
    { "name": "feature/active", "status": "active", "tracked": true }
  ]
}
```

Fields:

| Field | Type | Description |
|---|---|---|
| `name` | string | Branch name |
| `status` | string | `merged` / `closed-pr` / `upstream-gone` / `stale` / `active` |
| `tracked` | bool | Whether stax has metadata for this branch |
| `days_old` | number | Age of most recent commit in days (only present for `stale`) |
