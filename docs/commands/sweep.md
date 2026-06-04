# stax sweep

Classify all local branches and optionally delete the safe-to-remove ones.

## Why sweep?

Over time a repo accumulates branches that were merged, whose PR was closed, or that were abandoned as work-in-progress. `stax sync` already cleans up **stax-tracked** merged branches during `rs`, but it ignores untracked branches and has no read-only listing mode.

`stax sweep` fills both gaps:

- Operates on **every** local branch (tracked and untracked).
- Lists branches grouped by status so you can see what's accumulating.
- Deletes only what you explicitly ask for, never touching trunk or the current branch.

## Branch statuses

| Status | Meaning |
|---|---|
| `merged` | Ancestor of trunk, or confirmed merged via remote state |
| `upstream-gone` | Remote tracking ref is `[gone]` (upstream branch deleted) |
| `stale` | Last commit older than the configured threshold (default 30 days) |
| `active` | Everything else |

Precedence when a branch matches multiple: **merged > upstream-gone > stale > active**.

## Usage

```bash
# Read-only: classify all local branches and print a grouped summary
stax sweep

# Delete merged and upstream-gone branches (safe to remove), with confirmation
stax sweep --delete

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
| `--delete` | Delete merged and upstream-gone branches after confirmation |
| `--include-stale` | Extend deletion to stale branches (requires `--delete`) |
| `--force` | Skip confirmation prompt (requires `--delete`) |
| `--stale-days <N>` | Override stale threshold in days (default: 30) |
| `--json` | Output classification as JSON; conflicts with `--delete` |

## Configuration

Set the stale threshold globally in `~/.config/stax/config.toml`:

```toml
[branch]
stale_days = 60
```

`--stale-days` overrides this per-run.

## Safety

- Trunk and the current branch are always excluded.
- `--delete` without `--include-stale` never touches stale branches; unmerged work is safe.
- Stax-tracked children of deleted branches are reparented to trunk before deletion so `stax status` stays clean.
- `--json` is always read-only (conflicts with `--delete`).

## JSON output

`stax sweep --json` emits a JSON object with a `branches` array:

```json
{
  "branches": [
    { "name": "feature/old-stuff", "status": "merged", "tracked": true },
    { "name": "experiment-2024", "status": "stale", "tracked": false, "days_old": 47 },
    { "name": "feature/active", "status": "active", "tracked": true }
  ]
}
```

Fields:

| Field | Type | Description |
|---|---|---|
| `name` | string | Branch name |
| `status` | string | `merged` / `upstream-gone` / `stale` / `active` |
| `tracked` | bool | Whether stax has metadata for this branch |
| `days_old` | number | Age of most recent commit in days (only present for `stale`) |
