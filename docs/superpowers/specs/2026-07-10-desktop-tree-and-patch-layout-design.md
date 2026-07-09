# Desktop stack tree and patch layout design

## Summary

The macOS desktop app will replace its two-line branch cards with a compact,
single-line stack tree based on `st ls`. It will also make every patch row an
honest single line so long diff content cannot paint over adjacent virtual
rows.

This is a presentation-only change. The existing desktop protocol already
contains the topology and status fields needed by the new view, so the schema
and Rust snapshot payload remain unchanged.

## Problem

The left pane currently flattens each branch into a two-line row. Although the
snapshot includes each branch's stack column, the view does not render the
connector graph that makes forks and ancestry immediately legible in `st ls`.

The patch pane renders each line as a styled span paragraph inside a fixed
22-point virtual row. Native SDK span paragraphs always word-wrap and reserve
their wrapped height. The virtual list still advances by 22 points, so a long
path or source line paints through the following rows.

## Goals

- Render a compact, one-line branch row with visible stack topology.
- Preserve mouse selection, tree keyboard navigation, filtering, and native
  accessibility semantics.
- Show the most useful `st ls` status at a glance without making the branch
  name unreadable.
- Guarantee that every patch row paints within its fixed virtual extent.
- Keep the desktop engine protocol at schema version 1.

## Non-goals

- Pixel-for-pixel terminal emulation or use of a terminal renderer.
- Collapsible subtrees or a new expansion model.
- Horizontal scrolling in the patch pane.
- New Git, GitHub, CI, or repository queries.
- Changes to CLI/TUI `st ls` output.

## Stack row design

Each visible branch occupies one 36-point row. From left to right the row
contains:

1. Fixed-width connector cells for every visible lane.
2. A remote marker when the branch has a remote.
3. The branch name, which grows into remaining space and ellipsizes when
   necessary.
4. Compact status: ahead/behind counts, pull request, CI, and
   `needs restack` when present.

The selected row keeps the existing native selected background. The current
branch uses `◉`; other branches and trunk use `○`. The current lane and branch
name receive the lane's accent color, making a separate `HEAD` badge
unnecessary.

Lane colors repeat through four semantic theme tokens in the same visual order
as `st ls`: info/cyan, success/green, warning/yellow, and destructive/red. This
keeps the graph theme-aware rather than hard-coding RGB values.

Status order is stable:

```text
☁  branch/name  ↑2 ↓3  #42  ●  needs restack
```

Zero counts, missing PRs, and absent CI are omitted where doing so saves space.
The branch name owns the flexible width; the topology and status cells keep
their widths. If the pane becomes narrow, the name ellipsizes before the
status disappears.

## Connector geometry

The desktop model derives connector cells from the filtered branch rows using
the same rules as `src/tui/widgets/stack_tree.rs`:

- The maximum visible column determines a fixed connector width for every row.
- A non-trunk row paints `│` in columns before its own, then `◉` or `○` in its
  own column.
- When the previous visible row is deeper, the current node closes that lane
  with a corner.
- The trunk row joins the visible direct-child lanes with horizontal junctions.
- Empty cells after the active node preserve branch-name alignment.

Each connector cell has a fixed pixel width and its own lane color. The model
returns semantic cells rather than a preformatted terminal string, allowing
the Native view to retain correct layout, theme, and accessibility behavior.
Filtering follows the existing TUI behavior: topology is recomputed over the
visible rows while each branch retains its original column.

## Model and view boundary

`BranchRow` will expose the existing branch data plus:

- `is_trunk` and `needs_restack`;
- remote, pull-request, and CI display state;
- a slice of derived connector cells;
- lane-tone booleans used by declarative markup to select theme tokens.

Each connector cell contains a stable index, a two-character glyph, and the
same four lane-tone booleans. No protocol fields are added.

The declarative view will use a nested loop to paint fixed-width connector
cells, then separate single-line text leaves for the branch name and status.
Separate leaves are required because Native SDK styled-span paragraphs always
wrap.

## Patch rendering

Patch rows remain virtualized at 22 points. Each row becomes a plain `text`
leaf with explicit `wrap="false"` and ellipsis overflow. The row's semantic
foreground color continues to distinguish additions, deletions, hunks, files,
and context.

Removing the nested `span` is intentional. In the pinned Native SDK, `mono`,
inline weight, and mixed inline colors are span features, and span paragraphs
always wrap. Correct row isolation takes priority over the monospace face.
The full unmodified line remains in the model and accessibility text; only its
painted tail is ellipsized at the current pane width.

## Interaction and accessibility

- The outer row remains a `treeitem` and keeps the existing typed
  `select_branch` message.
- Up/Down, Home/End, Enter, and pointer selection continue to be owned by the
  Native tree widget.
- Connector glyphs and status fragments are visual support; the row's
  accessible label remains the full branch name.
- Filtering and selected-index behavior do not change.
- Patch text remains selectable one line at a time.

## Testing

Model tests will cover:

- nested lanes and fixed-width alignment;
- closing corners when a deeper branch precedes its parent;
- trunk junctions across multiple direct-child columns;
- current/trunk markers and four-color lane cycling;
- status derivation for remote, PR, CI, counts, and restack state;
- filtering with topology recomputed over visible rows.

View tests will assert:

- branch rows remain native tree items and dispatch their original selection;
- a long branch name is a single-line ellipsizing leaf;
- patch lines use plain text with no spans and explicit no-wrap behavior;
- the markup and accessibility audits remain clean.

Verification will run `native check`, the full Native test suite, the packaged
automation smoke, and the production Launch Services smoke. The final macOS
window will be inspected at the narrow patch-pane width that reproduced the
overlap.

## Documentation

`docs/interface/desktop.md` will describe the compact `st ls`-style topology
and single-line patch behavior. The README and `skills.md` command guidance do
not change because commands, flags, installation, and workflow behavior are
unchanged.
