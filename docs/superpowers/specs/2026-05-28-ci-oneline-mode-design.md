# `st ci --oneline` — compact whole-stack CI view

**Date:** 2026-05-28
**Status:** Design approved, pending spec review

## Problem

`st ci` today has two rendering modes:

- **Default** (`display_ci_verbose`): full per-check table, one line per individual check.
- **`-v/--verbose`** (`display_ci_compact`): "compact cards" — failed/running/passed grouped per branch, several lines per branch.

Both are detailed. There is no way to see the **whole stack at a glance** — one line per branch — the way a GitHub "Pull requests" list shows one row per PR (status, title, number, age). For a stack of 4–6 branches, the user wants a single-screen roll-up.

## Goal

Add a third rendering mode to `st ci`: a one-line-per-branch roll-up, rendered in stack order, with a leading CI status icon, branch name, PR number, PR title, and a trailing summary of check counts + timing.

## Decisions (locked with user)

| Decision | Choice |
|---|---|
| Invocation | New `--oneline` flag, short `-1`. Rendering-only; composes with scope flags. |
| Default scope when `--oneline` alone | **Current stack** (base→tip). `--all` still overrides to all tracked branches. |
| Row primary label | **PR title + branch name** (branch padded, current bold; title from API). |
| Trailing column | **Mix of check counts + timing**: e.g. `12 checks · 4m`, `2 failing · 3m`, `6/12 running · 2m`, `no CI`. |
| `--oneline` + `-v/--verbose` | **Conflict** — clap rejects with "cannot be used together". |

## Output format

Multi-branch summary header (reuse existing `print_multi_branch_header`), a blank line, then one row per branch in stack order:

```
CI  3 branches  ✓ 1 passing  ✗ 1 failing  ● 1 running

✓  feat/api-base        #115392  Enable KSP2 for faster annotation     12 checks · 4m
✗  feat/reduce-memory   #115377  Reduce Gradle local build memory       2 failing · 3m
●  feat/parallel-cache  #115389  Enable parallel config cache           6/12 running · 2m
○  feat/no-ci           #115401  Restructure agent docs                 no CI
```

### Columns (left → right)

1. **CI status icon** — same mapping as existing headers: `✓` green (success), `✗` red (failure), `●` yellow (pending/running), `○` dimmed (no CI / unknown).
2. **Branch name** — left-padded to the max branch width across rows; current branch rendered bold.
3. **`#<PR>`** — PR number when present; omitted (blank-padded) when the branch has no PR.
4. **PR title** — from the API; truncated with `…` to fit the remaining terminal width.
5. **Trailing summary** — `<counts> · <timing>`:
   - failure → `<N> failing · <elapsed>`
   - running/pending → `<done>/<total> running · <elapsed>`
   - all success → `<N> checks · <elapsed>`
   - no checks → `no CI` (no timing)
   - timing segment omitted when `calculate_branch_timing` returns `None`.

### Width handling

- Branch column width = max visible branch length across the rendered statuses.
- PR column width = max `#<PR>` width across rows (blank-padded for PR-less rows).
- Title is truncated to whatever horizontal space remains after icon + branch + PR + trailing summary, using terminal width (fallback to a sane default, e.g. 100, when width is unavailable). Visible-length math uses the existing `strip_ansi_len` helper so ANSI color codes don't skew alignment.

## Data changes

`BranchCiStatus` (src/commands/ci.rs:22) gains:

```rust
#[serde(skip_serializing_if = "Option::is_none")]
pub pr_title: Option<String>,
```

`#[serde(skip_serializing_if = "Option::is_none")]` keeps `--json` output unchanged for branches without a PR.

In `fetch_ci_statuses_async`, the current `client.get_pr(n)` call (which only reads `is_draft`) is replaced by `client.get_pr_with_head(n)`, which returns `PrInfoWithHead { title, info: PrInfo { is_draft, .. }, .. }`. Both `pr_is_draft` and `pr_title` are populated from that single call — no extra API round-trip. On error, both fall back to `None` (current behavior preserved).

## Rendering

New functions in src/commands/ci.rs:

- `fn display_ci_oneline(repo: &GitRepo, statuses: &[BranchCiStatus], current: &str, stack: &Stack)` — prints the multi-branch header (when >1 branch), reorders statuses into stack order (base→tip) using the stack, computes column widths, and prints each row.
- `fn oneline_row(status: &BranchCiStatus, is_current: bool, branch_w: usize, pr_w: usize, title_w: usize) -> String` — pure formatting for a single row; unit-testable without git/network.
- A small helper to build the trailing `counts · timing` segment from the existing partition logic + `calculate_branch_timing`.

### Ordering

`fetch_ci_statuses` currently sorts statuses alphabetically. For the oneline view, `display_ci_oneline` reorders by stack position (base→tip) before rendering — matching how `st log`/`st ls` present a stack. Other modes are unaffected.

## Dispatch

In `Commands::Ci`, add `oneline: bool` (clap `#[arg(long, short = '1', conflicts_with = "verbose")]`).

In `commands::ci::run(...)`:

- Add an `oneline: bool` parameter.
- **Scope**: when `oneline && !all && !stack`, set the scope to the current stack (same branch set as the `stack` path) so a bare `st ci --oneline` shows the whole stack.
- **Render**: branch the dispatch — `if oneline { display_ci_oneline(...) } else if verbose { display_ci_compact(...) } else { display_ci_verbose(...) }`.
- **Watch**: thread `oneline` through `run_watch_mode` so `st ci --oneline --watch` renders the compact view on each poll.

The CLI dispatch site in src/cli (where `Commands::Ci { .. }` is destructured and `commands::ci::run` is called) passes the new `oneline` argument.

## Edge cases

- **No tracked branches** — unchanged: prints "No tracked branches found.".
- **Single branch in scope** — still renders one row (header omitted when only one branch, matching existing `multi` gating).
- **Branch with no PR** — no `#` shown; title column blank; trailing summary still reflects checks.
- **Branch with no CI** — `○` icon, `no CI` trailing, no timing.
- **Narrow terminal** — title truncated; if space is extremely tight, title may be dropped entirely (branch + PR + status always shown).
- **`--json`** — orthogonal to `--oneline`; JSON path returns early before any display function, so `--oneline --json` yields the same JSON (now optionally including `pr_title`).

## Testing

Unit tests (no network/git) for `oneline_row` and the trailing-summary helper:

- passing branch with PR title → `N checks · <t>`
- failing branch → `N failing · <t>`
- running branch → `done/total running · <t>`
- no-CI branch → `no CI`, no timing
- branch without PR → no `#`, title blank
- long title truncated to width with `…`
- column alignment: branches of differing lengths pad to a common width

## Out of scope (YAGNI)

- PR review-state label ("Ready for review" / "Draft") as a column — the CI status icon is the leading signal; draft state already lives in `pr_is_draft` and can be added later if wanted.
- PR age / "15m"-style timestamps — trailing column is CI timing, not PR age.
- Cross-repo "repo" column from the screenshot — stax operates on a single repo's stack.
- Making `--oneline` the default for multi-branch views — explicit opt-in only.
