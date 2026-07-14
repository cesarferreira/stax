# Connected Stack Rails Design

## Context

The Codex-inspired sidebar currently renders the same topology glyph sequence as
`st ls`, but it places that sequence in 48 px branch rows. The large row height
creates visible gaps between `│` glyphs, so the graph reads as disconnected
symbols rather than a continuous stack. The branch order and topology data are
correct; the presentation is not.

## Goal

Render the existing stack topology as continuous, fixed-column rails while
preserving the current sidebar density, branch metadata, selection behavior,
search behavior, and keyboard/mouse interactions.

## Visual Design

- Reserve a fixed-width gutter made of equal lane columns.
- Draw every active lane through the full height of its branch row, split into
  top and bottom halves around the node position.
- Center the current/non-current node on its lane and join it to adjacent rails.
- Draw horizontal spans and return corners for fork parents and the trunk using
  the same relationships represented by `st ls`.
- Keep lane colors stable by column. Selected and hover backgrounds remain
  behind the topology.
- Keep the branch name and status block unchanged. The graph becomes connected
  without compressing the 48 px row or sacrificing status text.

## Topology Model

Replace presentation-oriented text segments with per-lane drawing data. Each
lane cell records whether it has a top rail, bottom rail, left connection, right
connection, and a node. The pure topology layout remains responsible for all
relationships; the GPUI renderer only paints the returned cells.

The layout derives connections from the full repository snapshot:

- ancestor lanes continue vertically through a row;
- a branch node connects upward/downward when an adjacent row continues that
  lane;
- a lane drop produces a top rail on the returning lane plus a horizontal span
  to the parent node;
- the trunk joins only direct-child lanes, matching the current `st ls` rule;
- filtering selects already-computed rows and never recalculates the graph from
  the filtered subset.

## Rendering

`stack_pane` renders one full-height lane cell per topology column. Thin rails
fill the appropriate half or width of the cell, and the node is centered above
them. The gutter width is derived from the full topology and remains stable
while searching.

The implementation does not alter branch loading, diff hydration/cache reuse,
uniform-list virtualization, selection, shortcuts, or pane resizing.

## Accessibility

Topology remains supplemental: branch names, Current/Trunk labels, PR state,
CI state, and restack state stay textual. Lane color is never the only source of
meaning. Focus and selected-row treatments remain unchanged.

## Testing

- Pure topology tests assert lane-cell connections for linear stacks, nested
  forks, lane returns, multiple direct trunk children, current nodes, and empty
  input.
- A filter-invariance test proves visible rows retain connections from the full
  snapshot.
- A GPUI render test proves the gutter contains full-height lane cells without
  disturbing row interaction selectors.
- Run `cargo nextest run -p stax-gui`, `make lint`, `make test`, and
  `make gui-app`.
- Capture the native app at the same repository state as the supplied screenshot
  and compare the connected rails visually before updating `design-qa.md`.

## Non-goals

- Changing branch order or stack semantics.
- Replacing the three-pane layout.
- Removing status metadata or reducing branch-row height.
- Adding a second indentation-only sidebar mode.
