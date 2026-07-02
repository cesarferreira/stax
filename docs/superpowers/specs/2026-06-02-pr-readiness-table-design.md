# PR readiness table

**Date:** 2026-06-02
**Status:** Design approved, self-reviewed, pending user review

## Problem

`st watch` gives a pleasant live stack view, and `st ci --oneline` gives compact CI status. Neither is optimized for the daily PR triage question:

- Which stack PRs can I merge?
- Which PRs need a reviewer ping?
- Which PRs need me to fix CI, conflicts, or requested changes?
- Which PRs are simply waiting?

The user wants fresh forge data at a glance, but not inside `st watch`.

## Goal

Add a read-only PR readiness table with a compact action signal on the left and titled columns across the row.

The command is available as:

```bash
st pr list --ready
st ready
```

`st pr list` without `--ready` keeps its existing repo-wide PR-list behavior.

## Decisions

| Decision | Choice |
|---|---|
| Default scope | Current stack, excluding trunk. |
| Wider scope | `--all` shows all tracked branch PRs. |
| Top-level shortcut | `st ready` is an alias for the readiness view. |
| Canonical home | `st pr list --ready`, because the data is PR-list data. |
| Mode | Read-only in v1. No merge, ping, rerequest, or fix actions. |
| Freshness | Fetch live forge data on every run; do not render stale cache as fresh. |
| Output shape | Titled table with `ACTION` first. |

## Output

Default table:

```text
stax/stax  current stack · fresh 11:21:08 · 4 PRs

ACTION   PR       BRANCH                                            REVIEWS         CI        TITLE
────────────────────────────────────────────────────────────────────────────────────────────────────────
✓ merge  #115665  cesar/OBX-2758-internal-tbt-poc-design            2 approvals     passed    Internal TBT POC design
● ping   #114961  cesar/enforce-ruby-version-for-android-releases   missing review  passed    Enforce Ruby version for Android releases
✕ fix    #112732  codex/date-based-android-versioning               1 approval      2 failed  Date based Android versioning
○ wait   #107328  codex/robot-android-bazel-docker                  0 approvals     running   Robot Android Bazel Docker
◌ draft  #115400  cesar/prep-release-notes                          draft           not run   Prepare release notes
```

Columns:

1. **ACTION**: the recommended next human action.
2. **PR**: `#<number>`.
3. **BRANCH**: local tracked branch name.
4. **REVIEWS**: review summary, such as `2 approvals`, `1 approval`, `missing review`, `changes requested`, `draft`, or `unknown`.
5. **CI**: compact CI summary, such as `passed`, `running`, `2 failed`, `no CI`, `not run`, or `unknown`.
6. **TITLE**: PR title from the forge.

The header includes the repository label, scope, freshness timestamp, and row count.

## Action Classification

Actions are derived from live PR, review, CI, and mergeability state.

Classification order is important:

1. **`◌ draft`**: PR is draft.
2. **`✕ fix`**: CI failed, changes were requested, the PR is non-mergeable, or the PR has conflicts.
3. **`○ wait`**: CI is pending/running, or GitHub mergeability is still being computed.
4. **`✓ merge`**: PR is open, non-draft, mergeable, CI is passing or no CI is configured, and review state is approved or no review is required.
5. **`● ping`**: CI is passing and the PR is otherwise unblocked, but review is required or no approvals are present when review state is unavailable.

Each row also carries a short machine-readable reason for JSON output and future actions, for example `ci_failed`, `changes_requested`, `review_required`, `mergeable_pending`, `ready`, or `draft`.

## Sorting

Rows sort by action priority, then stack order:

1. `fix`
2. `merge`
3. `ping`
4. `wait`
5. `draft`

Within each bucket, keep the branch order from the stack so dependencies remain understandable.

## Data Flow

1. Load `GitRepo`, `Config`, and `Stack`.
2. Determine branch scope:
   - default: `stack.current_stack(current)` excluding trunk
   - `--all`: all tracked branches excluding trunk
3. Resolve PR numbers from stack metadata first; if missing, fall back to forge lookup by branch name, matching existing merge/readiness patterns.
4. Fetch live data for each PR:
   - PR title, draft state, head SHA, mergeability, and state
   - review decision, approval count, and changes-requested state
   - detailed check runs for the branch head so the `CI` column can show counts like `2 failed`
5. Build `PrReadinessRow` values.
6. Render a responsive table using the existing table helpers from `commands::github_list`.

The implementation reuses existing primitives where practical:

- `ForgeClient`
- `GitHubClient::get_pr_merge_status` / forge equivalents for readiness fields
- `commands::ci::fetch_ci_statuses` or lower-level check fetching for detailed CI counts
- `commands::github_list` table width/truncation helpers

## CLI

Extend `PrCommands::List`:

```rust
List {
    #[arg(long, default_value_t = DEFAULT_GITHUB_LIST_LIMIT, value_parser = clap::value_parser!(u8).range(1..=100))]
    limit: u8,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    ready: bool,
    #[arg(long, requires = "ready")]
    all: bool,
}
```

Add top-level `Commands::Ready` as a visible command. It calls the same readiness runner as `st pr list --ready`.

`st ready --all` is accepted and behaves like `st pr list --ready --all`.

## JSON Output

`--json` returns the computed readiness rows:

```json
[
  {
    "branch": "cesar/example",
    "pr_number": 115665,
    "title": "Example PR",
    "action": "merge",
    "reason": "ready",
    "review_decision": "APPROVED",
    "approvals": 2,
    "changes_requested": false,
    "ci_status": "success",
    "ci_summary": "passed",
    "is_draft": false,
    "mergeable": true,
    "mergeable_state": "clean"
  }
]
```

## Edge Cases

- **No tracked branches in scope**: print a dimmed message and exit successfully.
- **Tracked branch without PR**: include a row only if a PR can be found by branch lookup; otherwise omit it from readiness and mention the skipped count in the header or footer.
- **Forge/auth failure**: return an error that states live readiness could not be fetched.
- **Partial row fetch failure**: mark the row `wait` with `unknown` fields only when the PR exists but one non-critical subrequest failed; avoid presenting it as mergeable.
- **Draft PR with failed CI**: action is `draft`, because the first action is to publish it before triaging merge readiness.
- **No CI configured**: treat as merge-compatible, matching existing `CiStatus::NoCi`, and render `no CI`.
- **Narrow terminals**: keep `ACTION`, `PR`, `REVIEWS`, and `CI`; truncate `BRANCH` and `TITLE` with existing width helpers.

## Tests

Add focused tests for pure classification and formatting:

- approved + passing + mergeable => `merge`
- review required + passing => `ping`
- zero approvals with unknown review decision + passing => `ping`
- failed CI => `fix`
- changes requested => `fix`
- non-mergeable/conflicts => `fix`
- pending CI => `wait`
- draft => `draft`
- action priority sorting preserves stack order within buckets
- table rendering includes the column titles and action labels
- JSON includes action, reason, review, CI, draft, and mergeability fields

Add at least one CLI parser/integration test showing:

- `st ready --help` is available
- `st pr list --ready --help` exposes `--all`
- `st pr list` without `--ready` retains existing behavior

Network-backed forge behavior is covered with existing mock clients or lower-level unit tests rather than live GitHub calls.

## Documentation

Because this changes user-visible behavior, update:

- `README.md` command table with `st ready`.
- `docs/commands/core.md` or `docs/commands/reference.md` with the readiness command.
- `skills.md` command map and PR workflow tips.

## Out of Scope

- Sending pings or re-requesting review.
- Merging from the readiness table.
- Showing arbitrary repo PRs authored by the user that are not tracked by stax.
- Continuous watch mode.
- Cross-repository dashboards.
