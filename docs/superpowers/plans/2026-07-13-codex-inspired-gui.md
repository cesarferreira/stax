# Codex-Inspired Stax GUI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restyle the dark Stax desktop workspace after the supplied Codex reference and render the branch stack with the same lane topology semantics as `st ls`.

**Architecture:** Add a pure GUI topology-layout module that converts the full ordered `BranchSummary` snapshot into lane-colored glyph segments, then let the stack pane select those immutable rows after search filtering. Keep all repository behavior in the existing application/state layers and confine the remaining work to semantic theme tokens and presentation changes in the three existing panes.

**Tech Stack:** Rust, GPUI, existing `stax::application` view models, cargo-nextest, shell-based macOS bundle tests.

## Global Constraints

- Restyle only the dark appearance; retain existing system appearance selection and leave the light palette functional.
- Preserve stack ordering, repository discovery, selection, search, keyboard access, pane resizing/visibility, operations, and diff caching.
- Add no new third-party dependency and no copied Codex assets.
- Keep color from being the only current-branch or status signal.
- Full-suite validation must run through `make test`, never an unfiltered native `cargo test`.
- The change is presentation-only and does not require README, command documentation, or `skills.md` updates.

---

## File Structure

- Create `crates/stax-gui/src/views/stack_topology.rs`: pure topology layout and unit tests.
- Modify `crates/stax-gui/src/views/mod.rs`: register the new private view helper.
- Modify `crates/stax-gui/src/views/stack_pane.rs`: render topology segments and compact Codex-style rows.
- Modify `crates/stax-gui/src/theme.rs`: Codex-inspired dark semantic tokens and lane palette.
- Modify `crates/stax-gui/src/views/workspace.rs`: compact shell, toolbar, and quiet dividers.
- Modify `crates/stax-gui/src/views/changes_pane.rs`: central canvas/file summary presentation.
- Modify `crates/stax-gui/src/views/inspector_pane.rs`: inset rounded inspector card and sections.
- Modify `crates/stax-gui/src/views/tests.rs` or focused pane tests only where selectors/layout contracts need coverage.
- Create `design-qa.md`: reference-versus-build visual QA record required before handoff.

### Task 1: Exact Stack Topology Layout

**Files:**
- Create: `crates/stax-gui/src/views/stack_topology.rs`
- Modify: `crates/stax-gui/src/views/mod.rs`
- Modify: `crates/stax-gui/src/views/stack_pane.rs`

**Interfaces:**
- Consumes: ordered `&[stax::application::BranchSummary]` with `name`, `parent`, `column`, `is_current`, and `is_trunk`.
- Produces: `pub(super) fn layout(branches: &[BranchSummary]) -> Vec<TopologyRow>`, where each row exposes `branch_name: String` and `segments: Vec<TopologySegment>`; each segment exposes `lane: usize` and `glyph: &'static str`.

- [ ] **Step 1: Write failing topology tests**

```rust
#[test]
fn nested_fork_matches_st_ls_connectors() {
    let rows = layout(&[
        branch("feature/a", Some("main"), 0, false, false),
        branch("feature/b-child", Some("feature/b"), 1, true, false),
        branch("feature/b", Some("main"), 1, false, false),
        branch("main", None, 0, false, true),
    ]);
    assert_eq!(plain(&rows[0]), "○    ");
    assert_eq!(plain(&rows[1]), "│ ○  ");
    assert_eq!(plain(&rows[2]), "│ ○  ");
    assert_eq!(plain(&rows[3]), "○─┘  ");
    assert_eq!(rows[1].segments[1].glyph, "◉");
}

#[test]
fn dropping_a_lane_draws_the_return_corner() {
    let rows = layout(&[
        branch("nested", Some("side"), 2, false, false),
        branch("side", Some("main"), 1, false, false),
        branch("main", None, 0, false, true),
    ]);
    assert_eq!(plain(&rows[1]), "│ ○─┘ ");
}

#[test]
fn trunk_joins_only_direct_child_lanes() {
    let rows = layout(&[
        branch("a", Some("main"), 0, false, false),
        branch("nested", Some("side"), 2, false, false),
        branch("side", Some("main"), 1, false, false),
        branch("main", None, 0, false, true),
    ]);
    assert_eq!(plain(rows.last().unwrap()), "○─┘    ");
}
```

- [ ] **Step 2: Verify the tests fail because the module and layout do not exist**

Run: `cargo nextest run -p stax-gui stack_topology::tests::`

Expected: compilation fails with an unresolved `stack_topology` module or missing `layout` function.

- [ ] **Step 3: Implement the pure topology model**

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopologySegment {
    pub lane: usize,
    pub glyph: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TopologyRow {
    pub branch_name: String,
    pub segments: Vec<TopologySegment>,
}

pub(super) fn layout(branches: &[BranchSummary]) -> Vec<TopologyRow> {
    let max_column = branches.iter().map(|branch| branch.column).max().unwrap_or(0);
    let target_width = (max_column + 1) * 2 + 1;
    branches
        .iter()
        .enumerate()
        .map(|(index, branch)| {
            if branch.is_trunk {
                trunk_row(branches, branch, target_width)
            } else {
                branch_row(branches, index, branch, target_width)
            }
        })
        .collect()
}
```

Implement `branch_row` with one `"│ "` segment for every lane left of `branch.column`, `"○"` or `"◉"` at the branch lane, and `"─┘"` when the previous row has a greater column. Implement `trunk_row` with the trunk node and `"─┴"`/`"─┘"` joins through the maximum column among direct trunk children. Append a lane-neutral space segment until every row reaches `target_width` characters.

- [ ] **Step 4: Run the topology tests and make them pass**

Run: `cargo nextest run -p stax-gui stack_topology::tests::`

Expected: every topology test passes.

- [ ] **Step 5: Replace the approximate stack marker with full-layout rows**

Build `Arc<HashMap<String, TopologyRow>>` from `workspace.state().snapshot().branches` before creating the uniform list. Pass the matching row to `render_branch_row`; render each non-padding segment with `MONOSPACE_FONT` and `theme.topology_lane(segment.lane)`. Give the gutter a fixed width derived from the full layout so filtered results keep their original alignment. Remove `topology_label` and column-based row indentation.

- [ ] **Step 6: Verify stack-pane tests**

Run: `cargo nextest run -p stax-gui stack_pane::tests:: stack_topology::tests::`

Expected: topology and status-label tests pass.

- [ ] **Step 7: Commit the topology deliverable**

```bash
git add crates/stax-gui/src/views/mod.rs crates/stax-gui/src/views/stack_topology.rs crates/stax-gui/src/views/stack_pane.rs
git commit -m "feat(gui): render exact stack topology"
```

### Task 2: Codex-Inspired Dark Theme and Shell

**Files:**
- Modify: `crates/stax-gui/src/theme.rs`
- Modify: `crates/stax-gui/src/views/workspace.rs`
- Test: `crates/stax-gui/src/theme.rs`

**Interfaces:**
- Consumes: existing semantic `Theme` usage and `WindowAppearance` selection.
- Produces: `sidebar`, `surface_hover`, and three topology lane tokens plus `Theme::topology_lane(usize) -> Hsla`.

- [ ] **Step 1: Add failing dark-palette tests**

```rust
#[test]
fn dark_theme_has_distinct_codex_style_depths() {
    let theme = Theme::dark();
    assert_ne!(theme.window, theme.sidebar);
    assert_ne!(theme.sidebar, theme.surface);
    assert_ne!(theme.surface, theme.surface_raised);
    assert_ne!(theme.surface_hover, theme.surface_selected);
}

#[test]
fn topology_lane_palette_cycles_deterministically() {
    let theme = Theme::dark();
    assert_ne!(theme.topology_lane(0), theme.topology_lane(1));
    assert_ne!(theme.topology_lane(1), theme.topology_lane(2));
    assert_eq!(theme.topology_lane(0), theme.topology_lane(3));
}
```

- [ ] **Step 2: Run the theme tests to verify failure**

Run: `cargo nextest run -p stax-gui theme::tests::`

Expected: compilation fails for missing `sidebar`, `surface_hover`, or `topology_lane`.

- [ ] **Step 3: Add semantic tokens and tune only the dark palette**

Use deep navy `window`, graphite `sidebar`, quiet blue-gray surfaces, low-contrast borders, readable foregrounds, and restrained blue focus/selection. Retain the current light palette values and provide equivalent light values for every new semantic field so `Theme::light()` remains complete. Implement lane cycling as:

```rust
pub fn topology_lane(self, lane: usize) -> Hsla {
    self.topology_lanes[lane % self.topology_lanes.len()]
}
```

- [ ] **Step 4: Run contrast and palette tests**

Run: `cargo nextest run -p stax-gui theme::tests::`

Expected: all semantic distinction, WCAG contrast, and lane palette tests pass.

- [ ] **Step 5: Restyle the workspace shell**

Reduce the toolbar to 50 px, use `theme.window` for the central canvas, use `theme.sidebar` for the stack surface, change pane dividers from solid 5 px bars to a 1 px visual rule inside the existing 5 px resize hit target, group secondary controls quietly, and retain the primary Submit action. Keep every existing debug selector and activation handler.

- [ ] **Step 6: Verify workspace rendering and interaction contracts**

Run: `cargo nextest run -p stax-gui views::tests:: workspace::tests::`

Expected: existing pane visibility, resizing, action, and keyboard tests pass.

- [ ] **Step 7: Commit the theme and shell deliverable**

```bash
git add crates/stax-gui/src/theme.rs crates/stax-gui/src/views/workspace.rs
git commit -m "style(gui): adopt Codex-inspired dark shell"
```

### Task 3: Restyle Stack, Changes, and Inspector Panes

**Files:**
- Modify: `crates/stax-gui/src/views/stack_pane.rs`
- Modify: `crates/stax-gui/src/views/changes_pane.rs`
- Modify: `crates/stax-gui/src/views/inspector_pane.rs`
- Modify: `crates/stax-gui/src/views/tests.rs` only for stable presentation selectors.

**Interfaces:**
- Consumes: Task 1 topology rows and Task 2 semantic theme fields.
- Produces: compact stack rows, calm central diff canvas, and inset inspector card without changing state or action APIs.

- [ ] **Step 1: Add failing presentation contract tests**

Add stable debug-selector assertions for `stack-topology-gutter`, `inspector-card`, and `changes-file-summary` to the existing GPUI render tests. Do not assert raw colors or pixel snapshots in unit tests.

- [ ] **Step 2: Run focused render tests to verify failure**

Run: `cargo nextest run -p stax-gui views::tests::`

Expected: selector assertions fail because the new presentation landmarks are absent.

- [ ] **Step 3: Restyle the stack pane**

Use `theme.sidebar`, 46 px compact rows, 8 px horizontal insets, rounded selected and hover fills, a quiet search field, a fixed topology gutter, a single-line branch name, and a concise metadata line. Preserve each branch row's ID, focusability, tab order, status text, and activation closure.

- [ ] **Step 4: Restyle the changes pane**

Use the deepest canvas for patch content, make the 43 px heading quieter, mark the summary container with `changes-file-summary`, show additions/deletions as compact colored counters, and retain the uniform lists and their row heights to avoid diff performance regressions.

- [ ] **Step 5: Restyle the inspector pane**

Give the outer pane the window canvas, wrap selected content in an `inspector-card` with 16 px corner radius and the raised surface, separate sections with low-contrast rules, and keep all existing action controls and scroll behavior.

- [ ] **Step 6: Run all GUI tests**

Run: `cargo nextest run -p stax-gui`

Expected: all GUI tests pass, including selection, loading, pane, operation, cache, and presentation contracts.

- [ ] **Step 7: Commit the pane redesign**

```bash
git add crates/stax-gui/src/views/stack_pane.rs crates/stax-gui/src/views/changes_pane.rs crates/stax-gui/src/views/inspector_pane.rs crates/stax-gui/src/views/tests.rs
git commit -m "style(gui): refine workspace panes"
```

### Task 4: Visual QA, Repository Verification, and Publication

**Files:**
- Create: `design-qa.md`
- Modify: only files required to fix visual or verification findings.

**Interfaces:**
- Consumes: completed dark workspace and supplied Codex/`st ls` screenshots.
- Produces: a locally verified app, passing repository checks, clean commits, and a stacked PR based on `cesar/gpui-gui-diff-cache`.

- [ ] **Step 1: Format and run focused static checks**

Run:

```bash
cargo fmt --all -- --check
make lint
```

Expected: both commands exit successfully.

- [ ] **Step 2: Build and launch the macOS app**

Run: `make gui-app`

Expected: `target/gui/Stax.app` is assembled successfully. Launch it against this repository and ensure the window uses the redesigned dark surfaces.

- [ ] **Step 3: Capture and compare the app**

Capture the full workspace at a desktop window size comparable to the reference. Compare palette, surface hierarchy, toolbar density, sidebar selection, typography, inspector card, diff readability, and `st ls` topology semantics. Record findings in `design-qa.md` with priorities P0-P3 and `final result: passed` only after all P0-P2 findings are fixed.

- [ ] **Step 4: Re-run GUI and full repository verification**

Run:

```bash
cargo nextest run -p stax-gui
make test
```

Expected: the GUI suite and full Docker-backed suite pass.

- [ ] **Step 5: Commit QA fixes and evidence**

```bash
git add design-qa.md crates/stax-gui/src
git commit -m "test(gui): verify Codex-inspired redesign"
```

- [ ] **Step 6: Review the complete branch diff**

Run: `git diff --check cesar/gpui-gui-diff-cache...HEAD` and inspect `git diff --stat cesar/gpui-gui-diff-cache...HEAD` plus the complete diff. Resolve correctness, accessibility, performance, and scope findings before publication.

- [ ] **Step 7: Submit the new stacked branch and create a proper PR body**

Run `stax submit --yes --no-prompt --no-template`, then ensure the PR title and body explain the Codex-inspired dark presentation, exact stack topology, tests, visual QA, and why command documentation is unchanged.

- [ ] **Step 8: Watch CI to completion**

Run: `stax ci --watch --strict --interval 10`

Expected: every required check passes. If a check fails, inspect its logs, fix the issue on the same stacked branch, push, and continue watching until green.
