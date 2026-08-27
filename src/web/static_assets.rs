//! Static asset bytes embedded at compile time.

pub const HTMX_JS: &str = include_str!("assets/htmx.min.js");

pub const APP_CSS: &str = r#"
/* stax web workspace — reference-faithful three-column layout */

/* ── Light palette (default + forced light) ─────────────────────────── */
:root,
[data-theme="light"] {
  --window:           #f3f3f4;
  --sidebar:          #ebebee;
  --surface:          #f9f9fa;
  --surface-raised:   #ffffff;
  --surface-hover:    #eff2f7;
  --surface-selected: #e6eef9;
  --border:           #d7d7da;
  --border-strong:    #b8b8bd;
  --text:             #202124;
  --text-muted:       #62656a;
  --accent:           #6c5ce7;
  --accent-text:      #ffffff;
  --focus:            #5a4bd1;
  --success:          #287a45;
  --warning:          #915d10;
  --danger:           #b13a36;
  --diff-add:         #1f7a3f;
  --diff-del:         #b23832;
  --diff-hunk:        #6750a4;
  --disabled-surface: #ebebed;
  --disabled-text:    #85878c;
  --lane-0: #3d6fa5; --lane-1: #3f7d58; --lane-2: #8a6d3b;
  --radius:           6px;
  --radius-card:      8px;
  --shadow-sm:        0 1px 3px rgba(0,0,0,.08);
  --shadow-md:        0 2px 8px rgba(0,0,0,.12);
  color-scheme: light;
}

/* ── Forced dark ─────────────────────────────────────────────────────── */
[data-theme="dark"] {
  --window:           #0d0e17;
  --sidebar:          #13141c;
  --surface:          #17182a;
  --surface-raised:   #1c1c28;
  --surface-hover:    #22223a;
  --surface-selected: #1e2545;
  --border:           #1e1e2e;
  --border-strong:    #2a2a40;
  --text:             #e0e0f0;
  --text-muted:       #8888a8;
  --accent:           #6f58e8;
  --accent-text:      #ffffff;
  --focus:            #9b8fff;
  --success:          #4ec97a;
  --warning:          #e8b44e;
  --danger:           #ff6b7a;
  --diff-add:         #4ec97a;
  --diff-del:         #ff6b7a;
  --diff-hunk:        #9b8fff;
  --disabled-surface: #1a1a28;
  --disabled-text:    #5a5a78;
  --lane-0: #6fb3e0; --lane-1: #74c294; --lane-2: #cbb072;
  --radius:           6px;
  --radius-card:      8px;
  --shadow-sm:        0 1px 3px rgba(0,0,0,.5);
  --shadow-md:        0 2px 8px rgba(0,0,0,.6);
  color-scheme: dark;
}

/* ── System follows OS preference ────────────────────────────────────── */
@media (prefers-color-scheme: dark) {
  [data-theme="system"] {
    --window:           #0d0e17;
    --sidebar:          #13141c;
    --surface:          #17182a;
    --surface-raised:   #1c1c28;
    --surface-hover:    #22223a;
    --surface-selected: #1e2545;
    --border:           #1e1e2e;
    --border-strong:    #2a2a40;
    --text:             #e0e0f0;
    --text-muted:       #8888a8;
    --accent:           #6f58e8;
    --accent-text:      #ffffff;
    --focus:            #9b8fff;
    --success:          #4ec97a;
    --warning:          #e8b44e;
    --danger:           #ff6b7a;
    --diff-add:         #4ec97a;
    --diff-del:         #ff6b7a;
    --diff-hunk:        #9b8fff;
    --disabled-surface: #1a1a28;
    --disabled-text:    #5a5a78;
    --lane-0: #6fb3e0; --lane-1: #74c294; --lane-2: #cbb072;
    --radius:           6px;
    --radius-card:      8px;
    --shadow-sm:        0 1px 3px rgba(0,0,0,.5);
    --shadow-md:        0 2px 8px rgba(0,0,0,.6);
    color-scheme: dark;
  }
}

@media (prefers-color-scheme: light) {
  [data-theme="system"] {
    color-scheme: light;
  }
}

/* ── Theme-independent layout metrics + type scale ───────────────────── */
:root {
  --card-pad-y:    7px;
  --card-border-w: 1px;
  --card-margin-y: 2px;
  --rail-bleed:     calc(var(--card-pad-y) + var(--card-border-w) + var(--card-margin-y));
  --topo-lane-w:   20px;
  --stack-rail-w:  240px;
  --stack-pane-w:  240px;

  --sans: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont,
          "Segoe UI Variable Text", "Segoe UI", Inter, Roboto,
          "Helvetica Neue", Arial, sans-serif;
  --mono: ui-monospace, "SF Mono", SFMono-Regular, "JetBrains Mono", Menlo,
          Consolas, "Liberation Mono", monospace;

  --fs-micro:  9.5px;
  --fs-label:  10px;
  --fs-sm:     11px;
  --fs-body:   12px;
  --fs-md:     13px;
  --fs-title:  15px;
  --ls-label:  .11em;
  --ls-title:  -.014em;
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

html, body {
  height: 100%;
  font-family: var(--sans);
  font-size: var(--fs-md);
  line-height: 1.5;
  background: var(--window);
  color: var(--text);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
  font-synthesis-weight: none;
  text-rendering: optimizeLegibility;
}

/* ── Workspace shell ────────────────────────────────────────────────── */
.workspace {
  display: flex;
  flex-direction: column;
  height: 100vh;
  overflow: hidden;
}

/* ── Top bar ─────────────────────────────────────────────────────────── */
.topbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 0 12px;
  height: 42px;
  background: var(--sidebar);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
  min-width: 0;
}

.topbar-logo {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
}
.logo-mark {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  border-radius: 6px;
  background: var(--accent);
  color: var(--accent-text);
  font-weight: 800;
  font-size: 12px;
}
.logo-name {
  font-weight: 700;
  font-size: 13px;
  letter-spacing: -0.02em;
  color: var(--text);
}

.topbar-sep {
  width: 1px;
  height: 20px;
  background: var(--border-strong);
  margin: 0 2px;
  flex-shrink: 0;
}

.topbar-project {
  display: flex;
  align-items: center;
  gap: 5px;
  flex-shrink: 0;
}

.topbar-search {
  flex: 1;
  min-width: 140px;
  max-width: 320px;
  padding: 4px 10px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-family: var(--sans);
  font-size: 12px;
  outline: none;
}
.topbar-search:focus {
  border-color: var(--focus);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--focus) 20%, transparent);
}
.topbar-search::placeholder { color: var(--text-muted); }

.topbar-group {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}

.topbar-actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

/* ── Three-column stage ─────────────────────────────────────────────── */
.stage {
  display: grid;
  grid-template-columns: var(--stack-pane-w) 8px minmax(0, 1fr) 280px;
  grid-template-rows: 1fr;
  flex: 1;
  overflow: hidden;
  min-height: 0;
}

.pane {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-right: 1px solid var(--border);
  background: var(--surface);
  min-width: 0;
}
.pane:last-child { border-right: none; }

.pane-stack  { min-width: 200px; }
.stack-resizer {
  cursor: col-resize;
  background: transparent;
  border: 0;
  padding: 0;
  touch-action: none;
}
.stack-resizer:hover, .stack-resizer:focus-visible { background: var(--accent); outline: none; }
.stage.stack-hidden { grid-template-columns: minmax(0, 1fr) 280px; }
.pane-changes {
  flex-direction: column;
}
.pane-inspector { }

.pane-body {
  flex: 1;
  overflow-y: auto;
  padding: 0;
  min-height: 0;
}

/* ── Status bar ─────────────────────────────────────────────────────── */
.status-bar {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 12px;
  font-size: 11px;
  background: var(--sidebar);
  border-top: 1px solid var(--border);
  flex-shrink: 0;
  min-height: 26px;
  overflow: hidden;
  white-space: nowrap;
}
.status-item { display: inline-flex; align-items: baseline; gap: 4px; }
.status-label {
  font-size: 10px;
  font-weight: 600;
  text-transform: uppercase;
  letter-spacing: .04em;
  color: var(--text-muted);
}
.status-value { font-weight: 500; }
.status-muted { color: var(--text-muted); font-style: italic; }
.status-sep {
  width: 1px;
  height: 12px;
  background: var(--border-strong);
  flex-shrink: 0;
}
.status-chip {
  font-size: 10px;
  font-weight: 600;
  padding: 1px 6px;
  border-radius: 3px;
  flex-shrink: 0;
}
.status-chip.chip-warning { background: rgba(145,93,16,.15); color: var(--warning); }
.status-chip.chip-pr { background: var(--surface-selected); color: var(--accent); }

/* ── Stack rail ─────────────────────────────────────────────────────── */
.stack-rail {
  display: flex;
  flex-direction: column;
  height: 100%;
  overflow: hidden;
  width: 100%;
}

.stack-header {
  display: flex;
  flex-direction: column;
  gap: 9px;
  padding: 14px 14px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.stack-header-labels {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.stack-header-label {
  font-size: var(--fs-label);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: var(--ls-label);
  color: var(--text-muted);
}
.stack-trunk-badge {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  max-width: 60%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: var(--mono);
  font-size: var(--fs-label);
  font-weight: 500;
  color: var(--text-muted);
  background: transparent;
  border: 1px solid var(--border);
  padding: 2px 7px;
  border-radius: 999px;
}
.stack-trunk-badge::before {
  content: "";
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: var(--lane-0);
  flex-shrink: 0;
}
.stack-title-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
  min-width: 0;
}
.stack-title {
  font-size: var(--fs-title);
  font-weight: 640;
  letter-spacing: var(--ls-title);
  line-height: 1.25;
  color: var(--text);
}
.stack-meta {
  font-size: var(--fs-sm);
  font-weight: 450;
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
  white-space: nowrap;
}

.branch-cards {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0 10px;
}

/* ── Branch cards ────────────────────────────────────────────────────── */
.branch-card {
  display: flex;
  align-items: stretch;
  padding: var(--card-pad-y) 8px var(--card-pad-y) 4px;
  margin: var(--card-margin-y) 8px;
  border-radius: var(--radius-card);
  border: var(--card-border-w) solid transparent;
  position: relative;
  transition: background .12s ease, border-color .12s ease;
  gap: 4px;
}
.branch-card:hover { background: var(--surface-hover); }
.branch-card.selected {
  background: var(--surface-selected);
  border-color: color-mix(in srgb, var(--accent) 45%, transparent);
}
.branch-card.selected::before {
  content: "";
  position: absolute;
  left: -1px; top: 6px; bottom: 6px;
  width: 2px;
  border-radius: 2px;
  background: var(--accent);
}
.branch-card.is-current .card-name { font-weight: 620; }
.branch-card.is-trunk .card-name { color: var(--text-muted); }
.branch-card.is-trunk { opacity: 1; }

/* Card selection surface — carries role=button so checkout is a sibling */
.card-select {
  display: flex;
  align-items: stretch;
  flex: 1;
  min-width: 0;
  cursor: pointer;
  user-select: none;
  background: none;
  border: none;
  padding: 0;
  gap: 6px;
}
.card-select:focus-visible {
  outline: 2px solid var(--focus);
  outline-offset: -2px;
  border-radius: calc(var(--radius-card) - 1px);
}

/* ── Card topology connector ─────────────────────────────────────────── */
.card-topo {
  flex-shrink: 0;
  display: flex;
  align-items: stretch;
}

/* Multi-lane topology cells */
.topo-cell {
  width: var(--topo-lane-w);
  flex-shrink: 0;
  position: relative;
  display: flex;
  align-items: center;
  justify-content: center;
  align-self: stretch;
}
.tc-rail {
  position: absolute;
  left: 50%;
  transform: translateX(-50%);
  width: 2px;
  z-index: 2;
}
.tc-rail.tc-top    { top: calc(-1 * var(--rail-bleed)); bottom: 50%; }
.tc-rail.tc-bottom { top: 50%; bottom: calc(-1 * var(--rail-bleed)); }
.tc-rail.lane-0 { background: var(--lane-0); opacity: .55; }
.tc-rail.lane-1 { background: var(--lane-1); opacity: .55; }
.tc-rail.lane-2 { background: var(--lane-2); opacity: .55; }
.tc-h {
  position: absolute;
  top: 50%;
  transform: translateY(-50%);
  height: 2px;
  background: var(--border-strong);
  z-index: 2;
}
.tc-h.tc-left  { left: 0; right: 50%; }
.tc-h.tc-right { left: 50%; right: 0; }
.tc-node {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  position: relative;
  z-index: 3;
  flex-shrink: 0;
  box-shadow: 0 0 0 2px var(--sidebar);
}
.tc-node.lane-0 { background: var(--lane-0); }
.tc-node.lane-1 { background: var(--lane-1); }
.tc-node.lane-2 { background: var(--lane-2); }
.tc-node.current {
  background: var(--accent);
  box-shadow: 0 0 0 2px var(--sidebar), 0 0 0 4px color-mix(in srgb, var(--accent) 30%, transparent);
}

/* ── Card inner ──────────────────────────────────────────────────────── */
.card-inner {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 1px 0;
}
.card-top {
  display: flex;
  align-items: center;
  gap: 6px;
}
.card-name {
  font-size: var(--fs-body);
  font-weight: 500;
  letter-spacing: -.008em;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}
.card-chips {
  display: flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
}
.card-bottom {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: var(--fs-micro);
  color: var(--text-muted);
}
.card-ci {
  font-size: var(--fs-micro);
  font-weight: 600;
  letter-spacing: .01em;
}
.card-ci.ci-pass { color: var(--success); }
.card-ci.ci-fail { color: var(--danger); }
.card-ci.ci-pending { color: var(--warning); }
.card-diverge {
  font-family: var(--mono);
  font-size: var(--fs-micro);
  font-variant-numeric: tabular-nums;
  color: var(--text-muted);
}

/* ── Quick actions ───────────────────────────────────────────────────── */
.quick-actions {
  flex-shrink: 0;
  border-top: 1px solid var(--border);
  padding: 12px 10px 12px;
  display: flex;
  flex-direction: column;
  gap: 1px;
}
.quick-actions-label {
  font-size: var(--fs-label);
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: var(--ls-label);
  color: var(--text-muted);
  padding: 0 4px;
  margin-bottom: 6px;
}
.quick-action {
  display: flex;
  align-items: center;
  gap: 9px;
  width: 100%;
  padding: 6px 8px;
  border: none;
  border-radius: var(--radius);
  background: transparent;
  color: var(--text);
  font-family: var(--sans);
  font-size: var(--fs-body);
  font-weight: 500;
  cursor: pointer;
  transition: background .12s ease;
  text-align: left;
}
.quick-action:hover { background: var(--surface-hover); }
.quick-action:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
.quick-action:disabled { opacity: .38; cursor: default; pointer-events: none; }
.qa-icon {
  width: 16px;
  flex-shrink: 0;
  text-align: center;
  color: var(--text-muted);
  font-size: var(--fs-body);
}
.qa-label { flex: 1; letter-spacing: -.006em; }
.qa-key {
  font-family: var(--mono);
  font-size: var(--fs-label);
  font-weight: 500;
  color: var(--text-muted);
  background: transparent;
  border: 1px solid var(--border);
  padding: 1px 5px;
  border-radius: 4px;
  flex-shrink: 0;
  line-height: 1.4;
}

/* ── Review header ───────────────────────────────────────────────────── */
.review-header {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 14px;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  flex-shrink: 0;
  min-height: 38px;
  flex-wrap: wrap;
}
.review-branch-name {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 260px;
  flex-shrink: 1;
}
.review-stat {
  font-size: 11px;
  color: var(--text-muted);
  flex-shrink: 0;
}
.review-tabs {
  display: flex;
  align-items: center;
  gap: 2px;
  flex-shrink: 0;
}
.review-tab {
  font-size: 11px;
  font-weight: 500;
  padding: 2px 8px;
  border-radius: var(--radius);
  color: var(--text-muted);
}
.review-tab.active {
  font-weight: 600;
  color: var(--text);
  background: var(--surface-selected);
}

/* ── Changes panel: side-by-side file nav + diff pane ───────────────── */
.changes-panel {
  display: flex;
  flex-direction: row;
  height: 100%;
  min-height: 0;
  overflow: hidden;
}

/* File navigator */
.file-nav {
  width: 200px;
  flex-shrink: 0;
  border-right: 1px solid var(--border);
  overflow-y: auto;
  background: var(--surface);
  display: flex;
  flex-direction: column;
}
.file-nav-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 10px;
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .06em;
  color: var(--text-muted);
  border-bottom: 1px solid var(--border);
  position: sticky;
  top: 0;
  background: var(--surface);
  z-index: 1;
  flex-shrink: 0;
}
.file-count {
  font-size: 10px;
  font-weight: 600;
  background: var(--border);
  padding: 1px 5px;
  border-radius: 3px;
}
.file-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 6px;
  width: 100%;
  padding: 5px 10px;
  border: none;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font-size: 11px;
  text-align: left;
  cursor: pointer;
  flex-shrink: 0;
}
.file-row:hover { background: var(--surface-hover); }
.file-row.active { background: var(--surface-selected); }
.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  font-family: var(--mono);
  font-size: 11px;
  min-width: 0;
}
.file-stats { display: flex; gap: 4px; flex-shrink: 0; font-size: 10px; font-weight: 600; }

/* Diff pane */
.diff-pane {
  flex: 1;
  overflow: auto;
  background: var(--surface-raised);
  display: flex;
  flex-direction: column;
  min-width: 0;
}
.diff-file-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--border);
  background: var(--surface);
  flex-shrink: 0;
  font-family: var(--mono);
  font-size: 11px;
  color: var(--text-muted);
  min-height: 32px;
  position: sticky;
  top: 0;
  z-index: 1;
}
.diff-file-path { color: var(--text); font-weight: 500; }
.diff-content {
  font-family: var(--mono);
  font-size: 11px;
  line-height: 1.45;
  flex: 1;
}

/* ── Diff lines with gutter ──────────────────────────────────────────── */
.diff-line {
  display: flex;
  align-items: stretch;
  min-height: 1.45em;
}
.diff-line:hover { filter: brightness(1.05); }
.diff-gutter-old,
.diff-gutter-new {
  width: 34px;
  flex-shrink: 0;
  text-align: right;
  padding: 0 6px;
  color: var(--text);
  user-select: none;
  border-right: 1px solid var(--border);
  font-size: 10px;
  line-height: 1.45;
  font-family: var(--mono);
}
.diff-text {
  flex: 1;
  padding: 0 12px;
  white-space: pre;
  line-height: 1.45;
  overflow: visible;
}
.diff-add .diff-gutter-old,
.diff-add .diff-gutter-new { background: rgba(78,201,122,.07); }
.diff-add .diff-text { color: var(--diff-add); background: rgba(78,201,122,.07); }
.diff-del .diff-gutter-old,
.diff-del .diff-gutter-new { background: rgba(255,107,122,.07); }
.diff-del .diff-text { color: var(--diff-del); background: rgba(255,107,122,.07); }
.diff-hunk .diff-text  { color: var(--diff-hunk); }
.diff-header .diff-text { color: var(--text-muted); font-weight: 600; padding-top: 8px; }

/* ── Changes empty state ─────────────────────────────────────────────── */
.changes-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  flex: 1;
  padding: 32px 24px;
  text-align: center;
  background: var(--surface);
  min-height: 200px;
}
.changes-empty-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text);
  margin-bottom: 6px;
}
.changes-empty-body {
  font-size: 12px;
  color: var(--text-muted);
  max-width: 300px;
  line-height: 1.5;
}

/* ── Inspector ───────────────────────────────────────────────────────── */
.inspector-pane-inner {
  display: flex;
  flex-direction: column;
  min-height: 100%;
}
.inspector-section {
  padding: 10px 12px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.inspector-label {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .06em;
  color: var(--text-muted);
  margin-bottom: 6px;
}
.inspector-row {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
  margin-bottom: 4px;
  font-size: 12px;
}
.inspector-key { color: var(--text-muted); }
.inspector-value { font-weight: 500; text-align: right; }

/* Inspector branch identity section */
.inspector-branch-name {
  font-size: 14px;
  font-weight: 700;
  word-break: break-all;
  margin-bottom: 6px;
}
.inspector-badges {
  display: flex;
  gap: 5px;
  flex-wrap: wrap;
}

/* Inspector commit list */
.inspector-commit {
  display: flex;
  gap: 8px;
  align-items: baseline;
  margin-bottom: 5px;
  font-size: 11px;
}
.commit-sha {
  font-family: var(--mono);
  font-size: 10px;
  color: var(--accent);
  flex-shrink: 0;
}
.commit-msg {
  color: var(--text);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
}

/* Inspector CTA bottom */
.inspector-spacer { flex: 1; min-height: 16px; }
.inspector-cta {
  padding: 12px;
  border-top: 1px solid var(--border);
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex-shrink: 0;
  position: sticky;
  bottom: 0;
  background: var(--surface);
  z-index: 2;
}
.inspector-cta-secondary {
  display: flex;
  gap: 6px;
}

/* Inspector actions section */
.inspector-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding-top: 4px;
}

/* ── Buttons ─────────────────────────────────────────────────────────── */
.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 12px;
  font-weight: 500;
  font-family: var(--sans);
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  cursor: pointer;
  transition: background .1s, border-color .1s;
  white-space: nowrap;
  line-height: 1.4;
  text-decoration: none;
}
.btn:hover { background: var(--surface-hover); border-color: var(--border-strong); }
.btn:disabled { opacity: .4; cursor: default; pointer-events: none; }
.btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }

.btn-primary {
  background: var(--accent);
  color: var(--accent-text);
  border-color: var(--accent);
  font-weight: 600;
}
.btn-primary:hover { opacity: .88; }

.btn-full { width: 100%; justify-content: center; }
.btn-danger { border-color: var(--danger); color: var(--danger); }
.btn-danger:hover { background: var(--danger); color: #fff; }
.btn-icon { padding: 4px 7px; }
.btn-checkout {
  align-self: center;
  font-size: var(--fs-label);
  font-weight: 600;
  letter-spacing: .02em;
  text-transform: uppercase;
  padding: 3px 7px;
  border-radius: 5px;
  background: transparent;
  border-color: transparent;
  color: var(--text-muted);
  opacity: 0;
  flex-shrink: 0;
  transition: opacity .12s ease, background .12s ease, color .12s ease;
}
.branch-card:hover .btn-checkout,
.branch-card.selected .btn-checkout,
.btn-checkout:focus-visible { opacity: 1; }
.btn-checkout:hover {
  background: var(--surface-raised);
  border-color: var(--border);
  color: var(--text);
}

/* ── Chips / badges ──────────────────────────────────────────────────── */
.meta-chip {
  font-size: var(--fs-micro);
  font-weight: 650;
  letter-spacing: .03em;
  padding: 1.5px 6px;
  border-radius: 999px;
  line-height: 1.4;
  white-space: nowrap;
  flex-shrink: 0;
  border: 1px solid transparent;
}
.meta-chip.chip-trunk   { background: transparent; border-color: var(--border-strong); color: var(--text-muted); }
.meta-chip.chip-head    { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--focus); }
.meta-chip.chip-diverge { background: var(--surface-hover); color: var(--text-muted); }
.meta-chip.chip-warning { background: color-mix(in srgb, var(--warning) 16%, transparent); color: var(--warning); }
.meta-chip.chip-pr      { background: transparent; border-color: color-mix(in srgb, var(--focus) 40%, transparent); color: var(--focus); font-family: var(--mono); font-weight: 550; }

/* ── Stat add / del ──────────────────────────────────────────────────── */
.stat-add { color: var(--diff-add); }
.stat-del { color: var(--diff-del); }

/* ── Overlays ────────────────────────────────────────────────────────── */
.overlay-backdrop {
  position: fixed; inset: 0;
  background: rgba(0,0,0,.45);
  display: flex; align-items: center; justify-content: center;
  z-index: 100;
}
.overlay-card {
  background: var(--surface-raised);
  border: 1px solid var(--border);
  border-radius: var(--radius-card);
  padding: 20px 24px;
  min-width: 340px;
  max-width: 520px;
  box-shadow: var(--shadow-md);
}
.overlay-title { font-weight: 700; font-size: 14px; margin-bottom: 12px; }
.overlay-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px; }

/* ── Banners ─────────────────────────────────────────────────────────── */
.banner {
  padding: 8px 14px;
  font-size: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.banner-success { background: rgba(40,122,69,.1); color: var(--success); border-bottom: 1px solid rgba(40,122,69,.2); }
.banner-error   { background: rgba(177,58,54,.09); color: var(--danger);  border-bottom: 1px solid rgba(177,58,54,.2); }
.banner-info    { background: rgba(124,104,255,.1); color: var(--accent);  border-bottom: 1px solid rgba(124,104,255,.2); }

/* ── Skeleton loading ────────────────────────────────────────────────── */
.skeleton {
  background: var(--border);
  border-radius: var(--radius);
  height: 12px;
  margin-bottom: 8px;
  animation: pulse 1.4s infinite;
}
@keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: .4 } }

/* ── Op spinner ──────────────────────────────────────────────────────── */
.op-spinner {
  display: inline-block;
  width: 12px; height: 12px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: spin .6s linear infinite;
}
@keyframes spin { to { transform: rotate(360deg) } }

/* ── Misc utilities ──────────────────────────────────────────────────── */
.text-muted  { color: var(--text-muted); }
.text-danger { color: var(--danger); }
.text-success{ color: var(--success); }
.text-warning{ color: var(--warning); }
.spacer { flex: 1; }
.pane-hidden { display: none !important; }
.flex { display: flex; }
.flex-col { display: flex; flex-direction: column; }
.gap-2 { gap: 8px; }
.mt-2 { margin-top: 8px; }

/* ── HTMX loading treatment ──────────────────────────────────────────── */
#changes-pane, #inspector-pane { position: relative; }

/* Requesting element gets .htmx-request from htmx itself. Delay the fade-in so
   a cache-warm diff (the common case) never flashes a spinner. */
.pane-body.htmx-request { pointer-events: none; }
.pane-body.htmx-request > * { opacity: .45; transition: opacity .12s ease; }
.pane-body.htmx-request::after {
  content: "";
  position: absolute;
  top: 14px;
  right: 14px;
  width: 14px; height: 14px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  z-index: 5;
  animation: spin .6s linear infinite, fade-in .15s .18s both;
}
@keyframes fade-in { from { opacity: 0 } to { opacity: 1 } }

.htmx-request.mutating-btn { opacity: 0.5; }

/* Static spinner slot rendered inside the pane on first paint. */
.pane-spinner { display: none; }

/* Loading overlay for the whole panel during stack-targeted mutations */
.pane.is-loading .pane-body {
  opacity: 0.4;
  pointer-events: none;
  transition: opacity 0.15s;
}

/* ── Form controls ───────────────────────────────────────────────────── */
form { display: contents; }

/* Scoped exceptions: forms that need explicit layout inside grids */
.inspector-actions form,
.inspector-section .move-form,
.inspector-section .reorder-form {
  display: flex;
  gap: 4px;
  align-items: center;
}

input[type="text"] {
  padding: 4px 8px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-family: var(--sans);
  font-size: 12px;
  width: 100%;
}
input[type="text"]:focus { outline: none; border-color: var(--focus); }

.project-select,
.theme-select {
  padding: 4px 7px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-family: var(--sans);
  font-size: 12px;
  cursor: pointer;
}
.project-select { max-width: 160px; }
.theme-select   { max-width: 90px; }

.project-add {
  width: 120px;
  min-width: 0;
  flex-shrink: 1;
  padding: 4px 8px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-family: var(--sans);
  font-size: 12px;
}

/* ── Stack empty state ───────────────────────────────────────────────── */
.stack-empty {
  padding: 16px 12px;
  font-size: 12px;
  color: var(--text-muted);
  line-height: 1.5;
}

/* ── Responsive ──────────────────────────────────────────────────────── */
/* Narrow topbar: actions wrap to a second full-width row so Restack / Open PR /
   Submit remain reachable. The repository-path input compacts but stays visible. */
@media (max-width: 900px) {
  .topbar {
    flex-wrap: wrap;
    height: auto;
    min-height: 42px;
    padding: 4px 10px;
    row-gap: 4px;
  }
  .project-add {
    width: 80px;
  }
  .topbar-actions {
    width: 100%;
    justify-content: flex-end;
    padding-bottom: 4px;
  }
}

/* Medium: inspector moves below review */
@media (max-width: 1100px) {
  .stage {
    grid-template-columns: var(--stack-pane-w) 8px minmax(0, 1fr);
    grid-template-rows: 1fr auto;
  }
  .stage.stack-hidden {
    grid-template-columns: minmax(0, 1fr);
  }
  .pane-inspector {
    grid-column: 1 / -1;
    max-height: 260px;
    border-top: 1px solid var(--border);
    border-right: none;
  }
}

/* Narrow: stacked, review first */
@media (max-width: 800px) {
  .stage {
    display: flex;
    flex-direction: column;
    overflow-y: auto;
    overflow-x: hidden;
  }
  .pane-changes  { order: -1; min-height: 300px; }
  .pane-stack    { order: 0;  max-height: 280px; }
  .pane-inspector { order: 1; max-height: none; }
  .pane-stack { min-width: 0; }
  .stack-rail { width: 100%; }
  .stack-resizer { display: none; }
  .card-name { overflow-wrap: anywhere; white-space: normal; }

  .changes-panel { flex-direction: column; }
  .file-nav {
    width: 100%;
    max-height: 160px;
    border-right: none;
    border-bottom: 1px solid var(--border);
  }
  .diff-pane { min-height: 200px; }
}

/* Very narrow (~500px): allow project/search/utility controls to wrap freely
   so no first-row overflow occurs and all primary actions remain reachable. */
@media (max-width: 500px) {
  .topbar-project {
    flex-wrap: wrap;
    flex-shrink: 1;
    min-width: 0;
  }
  .project-select {
    max-width: 100%;
    min-width: 0;
  }
  .project-add {
    width: 100%;
    min-width: 0;
  }
  .topbar-search {
    min-width: 60px;
    max-width: none;
  }
  .topbar-group {
    flex-wrap: wrap;
  }
}
"#;

pub const APP_JS: &str = r#"
// st web — keyboard shortcuts, pane rehydration, file-list diff navigation
document.addEventListener('DOMContentLoaded', function() {
  document.addEventListener('keydown', function(e) {
    // / focuses search
    if (e.key === '/' && !isInput(e.target)) {
      e.preventDefault();
      var s = document.getElementById('search-input');
      if (s) { s.focus(); s.select(); }
    }
    // Esc: dismiss overlay or blur search
    if (e.key === 'Escape') {
      var backdrop = document.querySelector('.overlay-backdrop');
      if (backdrop) backdrop.remove();
      var s = document.getElementById('search-input');
      if (s && document.activeElement === s) { s.blur(); }
    }
    // 1/2/3 toggle panes
    if (!isInput(e.target) && !document.querySelector('.overlay-backdrop')) {
      if (e.key === '1') togglePane('pane-stack', 'stack');
      if (e.key === '2') togglePane('pane-changes', 'changes');
      if (e.key === '3') togglePane('pane-inspector', 'inspector');
    }
    // Quick action shortcuts: N=new branch, R=restack, S=submit
    // Plain key only — no meta/ctrl/alt modifiers (avoids stealing Cmd+R, Cmd+S, etc.)
    if (!isInput(e.target) && !document.querySelector('.overlay-backdrop')) {
      if (!e.metaKey && !e.ctrlKey && !e.altKey) {
        if (e.key === 'n' || e.key === 'N') {
          var btn = document.querySelector('.qa-new-branch');
          if (btn && !btn.disabled) btn.click();
        }
        if (e.key === 'r' || e.key === 'R') {
          var btn = document.querySelector('.qa-restack');
          if (btn && !btn.disabled) btn.click();
        }
        if (e.key === 's' || e.key === 'S') {
          var btn = document.querySelector('.qa-submit');
          if (btn && !btn.disabled) btn.click();
        }
      }
      // Cmd/Ctrl+Z → Undo quick action
      if ((e.metaKey || e.ctrlKey) && e.key === 'z' && !e.shiftKey) {
        var undoBtn = document.querySelector('.qa-undo');
        if (undoBtn && !undoBtn.disabled) { e.preventDefault(); undoBtn.click(); }
      }
      // ? → open shortcuts help
      if (e.key === '?') {
        document.getElementById('help-overlay')?.remove();
        var tpl = document.getElementById('help-template');
        if (tpl) document.body.insertAdjacentHTML('beforeend', tpl.innerHTML);
      }
    }
  });

  // Rehydrate changes pane's file-list JS when it swaps. Note: htmx only
  // processes 'load' at initial page/swap time and never registers a
  // listener for it afterward, so dispatching 'load' here would do nothing —
  // the panes instead refresh via the server-driven HX-Trigger:
  // stax:branch-selected header (see routes.rs), which they listen for via
  // hx-trigger="... stax:branch-selected from:body".
  document.body.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail && e.detail.target && e.detail.target.id === 'changes-pane') {
      initFileList();
    }
  });

  initFileList();
  initStackPaneSizing();
});

var stackPaneSizingController = null;

function clampStackPaneWidth(width, min, max) {
  var numeric = Number(width);
  if (!Number.isFinite(numeric)) numeric = min;
  return Math.max(min, Math.min(max, numeric));
}

function intrinsicTextWidth(element) {
  if (!document.createRange) return element.scrollWidth;
  var range = document.createRange();
  range.selectNodeContents(element);
  var width = range.getBoundingClientRect().width;
  if (range.detach) range.detach();
  return Math.ceil(width);
}

function initStackPaneSizing() {
  if (!stackPaneSizingController) {
    var stage = document.querySelector('.stage');
    var resizer = document.querySelector('.stack-resizer');
    if (!stage || !resizer) return;
    stackPaneSizingController = createStackPaneSizingController(stage, resizer);
    window.staxStackPaneSizing = stackPaneSizingController;
    window.staxStackSizingMath = { clampWidth: clampStackPaneWidth };
  }
  stackPaneSizingController.refreshSizing();
}

function refreshStackPaneSizing() {
  if (stackPaneSizingController) stackPaneSizingController.refreshSizing();
  else initStackPaneSizing();
}

function createStackPaneSizingController(stage, resizer) {
  var key = 'stax.stack-pane-width:' + (stage.getAttribute('data-repository-key') || 'default');
  var manualMin = 200;
  var changesMin = 320;
  var mode = readStoredWidth() === null ? 'auto' : 'manual';
  var manualWidth = readStoredWidth();
  var autoWidth = 240;

  function readStoredWidth() {
    var stored = localStorage.getItem(key);
    if (stored === null || stored === '') return null;
    var width = Number(stored);
    return Number.isFinite(width) ? width : null;
  }

  function isVisible(element) {
    if (!element || element.hidden || element.classList.contains('pane-hidden')) return false;
    return getComputedStyle(element).display !== 'none';
  }

  function visibleWidth(element) {
    return isVisible(element) ? element.getBoundingClientRect().width : 0;
  }

  function bounds() {
    var stageWidth = stage.getBoundingClientRect().width || stage.clientWidth || window.innerWidth;
    var stacked = window.matchMedia('(max-width: 800px)').matches;
    if (stacked) {
      var fullWidth = Math.max(0, Math.round(stageWidth));
      return { min: fullWidth, max: fullWidth, stacked: true };
    }
    var inspector = document.querySelector('.pane-inspector');
    var changes = document.querySelector('.pane-changes');
    var inspectorWidth = window.matchMedia('(max-width: 1100px)').matches ? 0 : visibleWidth(inspector);
    var dividerWidth = visibleWidth(resizer) || 8;
    var reservedChanges = isVisible(changes) ? changesMin : 0;
    var max = Math.max(manualMin, Math.floor(stageWidth - inspectorWidth - dividerWidth - reservedChanges));
    return { min: manualMin, max: max, stacked: false };
  }

  function topologyMin(rail) {
    if (!rail) return 240;
    return Math.max(manualMin, parseInt(rail.getAttribute('data-topology-min-width') || '240', 10));
  }

  function automatic(rail, range) {
    var width = topologyMin(rail);
    if (rail && isVisible(rail) && rail.getBoundingClientRect().width > 0) {
      var railWidth = rail.getBoundingClientRect().width;
      rail.querySelectorAll('.card-name').forEach(function(name) {
        var chromeWidth = railWidth - name.getBoundingClientRect().width;
        width = Math.max(width, intrinsicTextWidth(name) + chromeWidth + 2);
      });
      autoWidth = width;
    } else {
      width = Math.max(width, autoWidth);
    }
    return clampStackPaneWidth(width, Math.min(topologyMin(rail), range.max), range.max);
  }

  function apply(width, applicationMode, rail, range) {
    if (range.stacked) {
      stage.style.setProperty('--stack-pane-w', '100%');
      resizer.setAttribute('aria-valuemin', range.min);
      resizer.setAttribute('aria-valuemax', range.max);
      resizer.setAttribute('aria-valuenow', range.max);
      return range.max;
    }
    var effectiveMin = applicationMode === 'auto'
      ? Math.min(topologyMin(rail), range.max)
      : range.min;
    var applied = clampStackPaneWidth(width, effectiveMin, range.max);
    stage.style.setProperty('--stack-pane-w', applied + 'px');
    resizer.setAttribute('aria-valuemin', range.min);
    resizer.setAttribute('aria-valuemax', range.max);
    resizer.setAttribute('aria-valuenow', Math.round(applied));
    return applied;
  }

  function refreshSizing() {
    var rail = document.querySelector('.stack-rail');
    var range = bounds();
    if (range.stacked) {
      apply(range.max, mode, rail, range);
      return;
    }
    if (mode === 'manual' && manualWidth !== null) apply(manualWidth, 'manual', rail, range);
    else apply(automatic(rail, range), 'auto', rail, range);
  }

  if (window.PointerEvent) {
    resizer.addEventListener('pointerdown', function(e) {
      if (bounds().stacked) return;
      resizer.setPointerCapture(e.pointerId);
      var start = e.clientX;
      var initial = parseFloat(getComputedStyle(stage).getPropertyValue('--stack-pane-w')) || autoWidth;
      var width = initial;
      function move(event) {
        var range = bounds();
        width = clampStackPaneWidth(initial + event.clientX - start, range.min, range.max);
        apply(width, 'manual', document.querySelector('.stack-rail'), range);
      }
      function cleanup() {
        resizer.removeEventListener('pointermove', move);
        resizer.removeEventListener('pointerup', end);
        resizer.removeEventListener('pointercancel', cancel);
        resizer.removeEventListener('lostpointercapture', cancel);
      }
      function release(event) {
        if (resizer.hasPointerCapture(event.pointerId)) resizer.releasePointerCapture(event.pointerId);
      }
      function end(event) {
        cleanup(); release(event);
        mode = 'manual'; manualWidth = width;
        localStorage.setItem(key, String(manualWidth));
      }
      function cancel(event) { cleanup(); release(event); refreshSizing(); }
      resizer.addEventListener('pointermove', move);
      resizer.addEventListener('pointerup', end);
      resizer.addEventListener('pointercancel', cancel);
      resizer.addEventListener('lostpointercapture', cancel);
    });
  }

  resizer.addEventListener('keydown', function(e) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    var range = bounds();
    if (range.stacked) return;
    var current = parseFloat(getComputedStyle(stage).getPropertyValue('--stack-pane-w')) || autoWidth;
    manualWidth = clampStackPaneWidth(current + (e.key === 'ArrowLeft' ? -16 : 16), range.min, range.max);
    mode = 'manual';
    localStorage.setItem(key, String(manualWidth));
    apply(manualWidth, 'manual', document.querySelector('.stack-rail'), range);
  });
  resizer.addEventListener('dblclick', function() {
    localStorage.removeItem(key);
    mode = 'auto'; manualWidth = null;
    refreshSizing();
  });
  document.body.addEventListener('htmx:afterSwap', function(e) {
    var target = e.detail && e.detail.target;
    if (target && (target.id === 'stack-pane' || target.querySelector?.('#stack-pane'))) refreshSizing();
  });
  window.addEventListener('resize', refreshSizing);

  return {
    refreshSizing: refreshSizing,
    getState: function() {
      var range = bounds();
      return {
        mode: mode,
        manualWidth: manualWidth,
        min: range.min,
        max: range.max,
        now: Number(resizer.getAttribute('aria-valuenow')),
        stacked: range.stacked,
      };
    },
  };
}

function isInput(el) {
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.contentEditable === 'true';
}

function togglePane(id, pane) {
  var el = document.getElementById(id);
  if (el) el.classList.toggle('pane-hidden');
  var stage = document.querySelector('.stage');
  var resizer = document.querySelector('.stack-resizer');
  if (pane === 'stack' && stage && resizer) {
    var hidden = el.classList.contains('pane-hidden');
    stage.classList.toggle('stack-hidden', hidden);
    resizer.hidden = hidden;
    if (!hidden) refreshStackPaneSizing();
  }
  var csrf = document.querySelector('input[name="csrf"]');
  var base = location.pathname.replace(/\/?$/, '');
  if (csrf && pane) {
    var body = new URLSearchParams({ pane: pane, csrf: csrf.value });
    fetch(base + '/panes', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: body.toString(),
    }).catch(function() {});
  }
}

function initFileList() {
  var panel = document.querySelector('.changes-panel');
  if (!panel) return;
  panel.querySelectorAll('.file-row').forEach(function(row) {
    row.addEventListener('click', function() {
      panel.querySelectorAll('.file-row').forEach(function(r) { r.classList.remove('active'); });
      row.classList.add('active');
      // Update the diff file header
      var fileNameEl = document.getElementById('diff-file-path');
      if (fileNameEl) {
        fileNameEl.textContent = row.getAttribute('data-file-name') || row.getAttribute('data-diff-file') || '';
      }
      var fid = row.getAttribute('data-diff-file');
      if (!fid) return;
      var target = document.getElementById('diff-file-' + fid);
      if (target) target.scrollIntoView({ behavior: 'smooth', block: 'start' });
    });
  });
  // Auto-select and reveal the first file
  var first = panel.querySelector('.file-row');
  if (first) {
    first.classList.add('active');
    var fileNameEl = document.getElementById('diff-file-path');
    if (fileNameEl) {
      fileNameEl.textContent = first.getAttribute('data-file-name') || '';
    }
  }
}

document.addEventListener('htmx:beforeRequest', function(e) {
  // Use e.detail.elt (the HTMX-triggering element) so form-backed mutations —
  // where e.target is the form but the submit button carries .mutating-btn —
  // are also detected and participate in global control disabling.
  // Descendant search is limited to FORM elements so that branch-card selection
  // (a plain div trigger) does not accidentally find the checkout .mutating-btn
  // inside the card and disable all controls.
  var elt = (e.detail && e.detail.elt) || e.target;
  var isMutating = elt && (
    elt.classList.contains('mutating-btn') ||
    (elt.tagName === 'FORM' && typeof elt.querySelector === 'function' && elt.querySelector('.mutating-btn'))
  );
  if (isMutating) {
    document.querySelectorAll('.mutating-btn').forEach(function(b) { b.disabled = true; });
  }
  var swapTarget = e.detail && e.detail.target;
  if (swapTarget) {
    var pane = swapTarget.closest('.pane');
    if (pane) pane.classList.add('is-loading');
  }
});
function clearPaneLoading() {
  document.querySelectorAll('.pane.is-loading').forEach(function(p) { p.classList.remove('is-loading'); });
}
document.addEventListener('htmx:afterRequest', function(e) {
  document.querySelectorAll('.mutating-btn').forEach(function(b) { b.disabled = false; });
  clearPaneLoading();
});
document.addEventListener('htmx:responseError', clearPaneLoading);
document.addEventListener('htmx:sendError', clearPaneLoading);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_selector_options_present() {
        assert!(
            APP_CSS.contains(r#"[data-theme="dark"]"#),
            "forced dark theme selector missing"
        );
        assert!(
            APP_CSS.contains(r#"[data-theme="system"]"#),
            "system theme selector missing"
        );
        assert!(
            APP_CSS.contains(r#"[data-theme="light"]"#),
            "forced light theme selector missing"
        );
    }

    #[test]
    fn dark_and_system_dark_token_parity() {
        // The accent colour is a reliable proxy token — it must appear in both
        // [data-theme="dark"] and the @media prefers-color-scheme:dark block.
        let dark_accent = "--accent:           #6f58e8";
        let count = APP_CSS.matches(dark_accent).count();
        assert!(
            count >= 2,
            "dark accent token should appear in both forced dark and system dark media query; found {count}"
        );
    }

    #[test]
    fn sans_and_mono_vars_defined() {
        assert!(
            APP_CSS.contains("--sans:"),
            "sans font variable missing from CSS"
        );
        assert!(
            APP_CSS.contains("--mono:"),
            "mono font variable missing from CSS"
        );
    }

    #[test]
    fn stack_column_is_sized_by_rendered_topology_width() {
        assert!(
            APP_CSS
                .contains("grid-template-columns: var(--stack-pane-w) 8px minmax(0, 1fr) 280px;"),
            "desktop stack track should follow the adaptive stack width"
        );
        assert!(
            APP_CSS.contains("width: 100%;"),
            "stack rail should fill the adaptive pane width"
        );
        assert!(
            !APP_CSS.contains("min(var(--stack-rail-w, 240px), 40vw)"),
            "a viewport cap before the 800px breakpoint would squash dense stacks"
        );
    }

    #[test]
    fn hidden_stack_grid_drops_stack_tracks_at_desktop_and_medium_widths() {
        assert!(
            APP_CSS
                .contains(".stage.stack-hidden { grid-template-columns: minmax(0, 1fr) 280px; }"),
            "desktop hidden-stack layout should contain only changes and inspector tracks"
        );
        let medium = APP_CSS
            .split("@media (max-width: 1100px)")
            .nth(1)
            .and_then(|css| css.split("@media (max-width: 800px)").next())
            .expect("medium layout media query should exist");
        assert!(
            medium.contains(".stage.stack-hidden")
                && medium.contains("grid-template-columns: minmax(0, 1fr);"),
            "medium hidden-stack layout should not retain stack or divider tracks"
        );
    }

    #[test]
    fn topology_rails_bleed_across_card_chrome() {
        assert!(
            APP_CSS.contains(
                "--rail-bleed:     calc(var(--card-pad-y) + var(--card-border-w) + var(--card-margin-y));"
            ),
            "rail bleed should equal one card's padding, border, and margin"
        );
        assert!(
            APP_CSS
                .contains(".tc-rail.tc-top    { top: calc(-1 * var(--rail-bleed)); bottom: 50%; }"),
            "top rail should extend into the preceding card gap"
        );
        assert!(
            APP_CSS
                .contains(".tc-rail.tc-bottom { top: 50%; bottom: calc(-1 * var(--rail-bleed)); }"),
            "bottom rail should extend into the following card gap"
        );
    }

    #[test]
    fn topology_connectors_paint_above_card_surfaces() {
        let rail = APP_CSS
            .split(".tc-rail {")
            .nth(1)
            .and_then(|css| css.split('}').next())
            .expect("tc-rail rule should exist");
        assert!(
            rail.contains("z-index: 2;"),
            "bleeding rails should paint above adjacent card surfaces"
        );

        let node = APP_CSS
            .split(".tc-node {")
            .nth(1)
            .and_then(|css| css.split('}').next())
            .expect("tc-node rule should exist");
        assert!(
            node.contains("z-index: 3;"),
            "nodes should paint above rails"
        );
    }

    #[test]
    fn selection_driven_panes_show_a_pending_indicator() {
        assert!(
            APP_CSS.contains(".pane-body.htmx-request::after"),
            "panes must render a spinner while their request is in flight"
        );
    }

    #[test]
    fn dead_load_retrigger_is_not_reintroduced() {
        assert!(
            !APP_JS.contains("'load')"),
            "pane refresh must use the stax:branch-selected event, not htmx.trigger(el,'load')"
        );
    }
}
