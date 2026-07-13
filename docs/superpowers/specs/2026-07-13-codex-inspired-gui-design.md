# Codex-Inspired Stax GUI Design

## Goal

Make the dark Stax desktop workspace feel visually consistent with the Codex
desktop app while preserving Stax's existing three-pane workflow. Replace the
stack pane's approximate branch markers with a topology gutter that matches the
ordering and connector semantics of `st ls`.

## Scope

- Restyle the dark appearance in this iteration. Existing system appearance
  selection remains intact; the light palette is not redesigned here.
- Preserve the existing stack, changes, and inspector panes and their resizing,
  visibility, keyboard navigation, loading, operation, and caching behavior.
- Restyle the existing screens and controls; do not add a Codex-style global
  navigation system or new product concepts.
- Render the stack topology from existing `BranchSummary` data without changing
  repository discovery or stack ordering.

## Visual Direction

The application uses a deep navy workspace canvas and a slightly lighter
graphite stack sidebar. Borders are low contrast and used sparingly. Selected
rows use a soft rounded fill instead of a bright rectangular outline. Text uses
the system UI font for interface copy and the existing monospace font for branch
names and diffs.

The layout remains:

1. A compact top toolbar for repository identity and primary actions.
2. A left stack sidebar containing its title, search field, topology gutter,
   branch names, and concise status metadata.
3. A central changes surface with the file summary and patch.
4. A right inspector rendered as an inset rounded card with grouped branch,
   pull-request, CI, and action sections.

The palette takes its character from the supplied Codex screenshot: deep navy
for the main canvas, graphite for navigation surfaces, muted blue-gray text,
and restrained blue selection/focus. Semantic success, warning, danger, and
diff colors remain distinguishable and accessible. The topology uses a small
cyan/green/lime lane palette inspired by `st ls`, but normal branch text remains
quiet enough for sustained reading.

## Stack Topology

Introduce a pure topology layout helper for the GUI. It consumes the ordered
branch summaries already emitted by the application layer and produces a row
model with lane cells. Each cell identifies its lane and one of the semantic
connector shapes required by the CLI layout:

- vertical ancestor rail;
- inactive branch node (`○`);
- current branch node (`◉`);
- side-branch return corner (`─┘`);
- trunk join (`─┴` or `─┘`);
- empty alignment cell.

The algorithm follows the same rules as `st ls`: branch rows use their emitted
column, a drop from the previous row's column closes the departing side lane,
and the trunk row joins only lanes used by its direct children. Rows pad to the
maximum visible lane so every branch name aligns.

The GUI renders these cells as fixed-width monospace spans. Connector shape and
lane color are independent, which keeps the layout deterministic and testable.
The current branch is indicated by both the `◉` node and its existing `Current`
text status, so color is never the sole signal.

Search continues to filter branch rows. Topology is computed once from the full
ordered snapshot, then matching row models are selected by branch name. Search
therefore cannot change a branch's lane, connector shape, or implied parent
relationship. Clearing search restores the full graph exactly.

## Components and Boundaries

- `theme.rs` owns the Codex-inspired dark semantic tokens. Existing consumers
  continue to request semantic colors rather than embedding palette literals.
- `stack_pane.rs` owns the pure topology row model and its rendering. Repository
  and Git logic remain outside the view.
- `workspace.rs` owns the shell, toolbar, pane backgrounds, and dividers.
- `changes_pane.rs` and `inspector_pane.rs` receive presentation-only changes;
  their loading and interaction behavior remains unchanged.
- Shared controls retain the existing activation and focus helpers so mouse and
  keyboard behavior stays consistent.

## Interaction and States

- Branch selection remains single-click and keyboard-operable.
- Search, pane resizing, pane visibility, refresh, repository opening, branch
  operations, submitting, and inspector actions keep their current contracts.
- Loading, empty, failure, operation progress, and completion states adopt the
  new surfaces but retain their current copy and recovery actions.
- Focus rings remain visible against normal, selected, and raised dark surfaces.
- The layout remains usable at the application's supported minimum window size;
  long repository paths, branch names, and statuses continue to truncate.

## Testing and Verification

Unit tests cover topology output for linear stacks, nested stacks, sibling
forks, closing side lanes, trunk joins, the current branch, and filtered rows.
Theme tests continue to enforce semantic distinctions and focus/text contrast.
Existing GPUI tests protect selection, keyboard navigation, loading, pane
resizing, operations, and diff caching.

Verification consists of:

1. Targeted topology and GUI tests during implementation.
2. `cargo fmt --all -- --check` and the repository lint target.
3. `cargo nextest run -p stax-gui`.
4. The full suite through `make test`.
5. A local dark-mode app capture compared with the supplied Codex screenshot,
   with blocking visual issues fixed before handoff.

## Documentation Impact

This changes presentation but not commands, flags, workflows, or defaults.
README, command documentation, and `skills.md` do not require changes.
