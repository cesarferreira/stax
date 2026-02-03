# stax co minimal checkout redesign

## Goal
Redesign `stax co` to be a fast, minimalist command-palette style picker that is easy to scan and search. The focus is on reducing visual clutter and making it easy to find the right branch quickly.

## UX summary
- Single dense list, no preview pane.
- Rows are aligned into compact columns.
- Minimal “stacked” depth cues via indentation and light guide characters.
- Current stack is shown first to optimize the common workflow.

## Layout
The list is a single table-like view with the following columns:

- `branch`: branch name with stacked depth indicator (indent + glyph)
- `stack`: root name of the stack (trunk child) or `trunk` for the trunk branch
- `Δ`: ahead/behind relative to parent (e.g., `+2/-0`), `—` when not applicable
- `PR/flags`: PR number if present (e.g., `#103`) and `⟳` if needs restack

Example:

```
Checkout branch (type to filter)

depth  branch               stack     Δ      PR/flags
•      auth                 auth      +0/-1  #101
│ ○    auth-api             auth      +1/-0  #102
│ │ ▪  auth-ui              auth      +2/-0  #103  ⟳
•      hotfix-payment       hotfix    +0/-0  —
•      main                 trunk     +0/-0  —
```

Rules:
- Indentation is based on depth from trunk (0 for trunk child).
- The bullet is `•` for depth 0, `○` for depth > 0, and `●` for current branch.
- Vertical guides (`│`) show depth while staying minimal.

## Ordering
- Current stack first (root -> descendants in pre-order).
- Remaining stacks after, sorted alphabetically by root.
- Trunk is listed last.

This ordering optimizes common switching behavior without adding extra UI controls.

## Data model
- Stack structure from `Stack::load`.
- Depth computed by walking parents until trunk.
- Root stack name computed by walking parents until trunk or a missing parent.
- Ahead/behind uses existing `commits_ahead_behind(parent, branch)`.
- `PR/flags` uses stored PR metadata in `StackBranch` and `needs_restack`.

## Interaction
- Keep `dialoguer::FuzzySelect` with the same prompt.
- Filtering should match across the full row (branch + stack + PR number).
- No highlight-match coloring to avoid ANSI conflicts.

## Risks / edge cases
- Orphaned branches (parent missing) are treated as trunk children for display.
- If `commits_ahead_behind` fails for a row, display `—` for `Δ`.

## Testing
- Manual check in a repo with:
  - 1 stack
  - multiple stacks
  - a deep stack (3+ levels)
  - a branch with no PR metadata
- Ensure list is stable and aligned while filtering.
