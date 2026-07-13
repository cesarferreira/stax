# Connected Stack Rails Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the disconnected text-glyph topology gutter with continuous full-row stack rails that preserve the relationships shown by `st ls`.

**Architecture:** Keep topology computation pure in `stack_topology.rs`, but return per-lane connection cells instead of strings. `stack_pane.rs` paints those cells as fixed-width GPUI lane elements while continuing to derive all visible rows from the full unfiltered snapshot.

**Tech Stack:** Rust, GPUI 0.2.2, cargo-nextest, native macOS app bundle, existing Stax theme and application models.

## Global Constraints

- Preserve the 48 px branch rows, branch name/status block, selection, hover, focus, search, virtualization, shortcuts, and pane resizing.
- Keep topology supplemental to textual Current/Trunk, PR, CI, and restack status.
- Derive topology from the full repository snapshot before filtering.
- Do not add a dependency or a second sidebar mode.
- Full-suite validation must run through `make test`.

---

### Task 1: Model continuous lane connections

**Files:**
- Modify: `crates/stax-gui/src/views/stack_topology.rs`

**Interfaces:**
- Consumes: `stax::application::BranchSummary` in the existing display order.
- Produces: `layout(branches: &[BranchSummary]) -> Vec<TopologyRow>` where each `TopologyRow` contains `cells: Vec<TopologyCell>` and each `TopologyCell` exposes `lane`, `top`, `bottom`, `left`, `right`, and `node: Option<TopologyNode>`.

- [ ] **Step 1: Replace glyph assertions with failing connection assertions**

Add these public-to-module types and test helpers to the test expectations before implementing them:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TopologyNode {
    Branch,
    Current,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TopologyCell {
    pub lane: usize,
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
    pub node: Option<TopologyNode>,
}
```

Assert the following exact cases:

```rust
assert_eq!(rows[0].cells[0], cell(0, false, true, false, false, Branch));
assert_eq!(rows[1].cells[0], cell(0, true, true, false, false, Current));
assert_eq!(rows[2].cells[0], cell(0, true, false, false, false, Branch));

assert_eq!(return_row.cells[1], cell(1, true, true, false, true, Branch));
assert_eq!(return_row.cells[2], cell(2, true, false, true, false, None));

assert_eq!(trunk.cells[0], cell(0, true, false, false, true, Branch));
assert_eq!(trunk.cells[1], cell(1, true, false, true, true, None));
assert_eq!(trunk.cells[2], cell(2, true, false, true, false, None));
```

Retain empty-input and branch-name assertions.

- [ ] **Step 2: Run the topology tests and verify they fail**

Run:

```bash
cargo nextest run -p stax-gui stack_topology::tests::
```

Expected: compilation or assertion failure because `TopologyCell` connection data is not implemented.

- [ ] **Step 3: Implement the minimal connection layout**

For each row, allocate one default cell per lane through the global maximum column, then apply these rules:

```rust
for lane in 0..branch.column {
    cells[lane].top = true;
    cells[lane].bottom = true;
}

let node = &mut cells[branch.column];
node.node = Some(if branch.is_current {
    TopologyNode::Current
} else {
    TopologyNode::Branch
});
node.bottom = !branch.is_trunk;
```

For a non-trunk branch, connect the node upward when the preceding row has the same column. When the preceding row has a larger column, set the node's `right`, set `left` and `right` on intermediate cells, and set `top` plus `left` on the returning cell. For the trunk, set lane zero's `top`, join only through the maximum direct-child column, and give each joined child lane a `top` connection.

Remove string padding and glyph generation; every row has exactly `max_column + 1` fixed cells.

- [ ] **Step 4: Run topology and filter-invariance tests**

Run the two filters separately:

```bash
cargo nextest run -p stax-gui stack_topology::tests::
cargo nextest run -p stax-gui filtering_reuses_rows_from_the_full_topology
```

Expected: topology tests pass; the existing filter test may fail until Task 2 updates its cell assertions.

- [ ] **Step 5: Commit the model**

```bash
git add crates/stax-gui/src/views/stack_topology.rs
git commit -m "refactor(gui): model connected stack rails"
```

### Task 2: Paint full-height lane cells in GPUI

**Files:**
- Modify: `crates/stax-gui/src/views/stack_pane.rs`
- Modify: `crates/stax-gui/src/views/tests.rs`

**Interfaces:**
- Consumes: `TopologyRow.cells` and `TopologyNode` from Task 1.
- Produces: `render_topology_cell(cell: TopologyCell, theme: Theme, selected: bool) -> Div` plus the existing `render_branch_row` behavior.

- [ ] **Step 1: Write failing render and filter tests**

Update `filtering_reuses_rows_from_the_full_topology` to assert the `side` row retains the ancestor rail, parent node, and returning child lane from the full snapshot:

```rust
assert!(side.cells[0].top && side.cells[0].bottom);
assert_eq!(side.cells[1].node, Some(TopologyNode::Branch));
assert!(side.cells[1].right);
assert!(side.cells[2].top && side.cells[2].left);
```

Extend the GPUI presentation test to require selectors for the lane and rail primitives:

```rust
for selector in [
    "stack-topology-gutter",
    "stack-topology-cell",
    "stack-topology-vertical-rail",
    "stack-topology-node",
] {
    assert!(cx.debug_bounds(selector).is_some());
}
```

- [ ] **Step 2: Run the render tests and verify they fail**

Run:

```bash
cargo nextest run -p stax-gui stack_pane::tests::
cargo nextest run -p stax-gui workspace_renders_codex_presentation_landmarks
```

Expected: failure because lane-cell selectors and rendering do not exist.

- [ ] **Step 3: Implement fixed-width full-height lane rendering**

Use these dimensions in `stack_pane.rs`:

```rust
const TOPOLOGY_LANE_WIDTH: f32 = 16.0;
const TOPOLOGY_RAIL_WIDTH: f32 = 1.0;
const TOPOLOGY_NODE_SIZE: f32 = 7.0;
const BRANCH_ROW_HEIGHT: f32 = 48.0;
```

Render each cell as a `relative`, full-height, fixed-width div. Add absolute rail children only for enabled edges:

```rust
div()
    .debug_selector(|| "stack-topology-cell".into())
    .relative()
    .flex_none()
    .w(px(TOPOLOGY_LANE_WIDTH))
    .h(px(BRANCH_ROW_HEIGHT))
```

Vertical top/bottom rails use `TOPOLOGY_RAIL_WIDTH`, centered at `TOPOLOGY_LANE_WIDTH / 2`, and cover exactly the top or bottom half. Horizontal left/right rails use one-half lane width and sit at `BRANCH_ROW_HEIGHT / 2`. Give every rail the cell's `theme.topology_lane(cell.lane)` color.

Render `TopologyNode::Branch` as a 7 px outlined circle and `TopologyNode::Current` as a 7 px filled circle, centered over the rails. Keep node and rail selectors stable for GPUI tests. Set the gutter width to `row.cells.len() * TOPOLOGY_LANE_WIDTH`; do not measure text characters.

- [ ] **Step 4: Run the full GUI suite**

Run:

```bash
cargo fmt --all
cargo nextest run -p stax-gui
```

Expected: all GUI tests pass, including connection, filtering, selection, search, and interaction tests.

- [ ] **Step 5: Commit the renderer**

```bash
git add crates/stax-gui/src/views/stack_pane.rs crates/stax-gui/src/views/tests.rs
git commit -m "fix(gui): connect stack topology rails"
```

### Task 3: Native visual QA and publication

**Files:**
- Modify: `design-qa.md`

**Interfaces:**
- Consumes: the native `target/gui/Stax.app`, the supplied disconnected-sidebar screenshot, and the connected implementation.
- Produces: a passing visual QA record, a pushed update to PR #618, and green CI.

- [ ] **Step 1: Build and inspect the native app**

Run:

```bash
make gui-app
```

Open the native app on this repository and capture the same sidebar state shown in the supplied screenshot. Verify rails touch row boundaries, nodes sit on the rails, returns connect horizontally, branch text remains aligned, and selected/hover rows do not obscure topology.

- [ ] **Step 2: Compare reference and implementation**

Create a same-state side-by-side comparison containing the supplied screenshot and the new native capture. Record source paths, viewport/state, visible differences, interaction checks, and iteration history in `design-qa.md`. Fix every P0/P1/P2 issue and repeat until the report says:

```text
final result: passed
```

- [ ] **Step 3: Run repository verification**

Run:

```bash
cargo fmt --all -- --check
make lint
cargo nextest run -p stax-gui
make test
make gui-app
git diff --check
```

Expected: formatting/lint pass, all GUI tests pass, all full-suite tests pass, the native app assembles, and the diff has no whitespace errors.

- [ ] **Step 4: Commit QA evidence and publish**

```bash
git add design-qa.md
git commit -m "test(gui): verify connected stack rails"
stax branch submit --yes --no-prompt --no-template --no-fetch --publish
```

Update PR #618's body with the connected-rail summary and new test totals without removing its existing redesign context.

- [ ] **Step 5: Watch CI to completion**

Run:

```bash
stax ci --watch --strict --interval 10
```

Expected: every PR #618 check passes. If a check fails, inspect it, fix the root cause with regression coverage, submit again, and continue watching until green.
