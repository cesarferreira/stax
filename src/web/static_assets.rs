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
  --lane-0:           #1677a8;
  --lane-1:           #287a45;
  --lane-2:           #668a14;
  --radius:           6px;
  --radius-card:      8px;
  --shadow-sm:        0 1px 3px rgba(0,0,0,.08);
  --shadow-md:        0 2px 8px rgba(0,0,0,.12);
  --sans:             -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --mono:             "Menlo", "Monaco", "Consolas", "SF Mono", monospace;
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
  --lane-0:           #4ec9ff;
  --lane-1:           #4ec97a;
  --lane-2:           #a3e635;
  --radius:           6px;
  --radius-card:      8px;
  --shadow-sm:        0 1px 3px rgba(0,0,0,.5);
  --shadow-md:        0 2px 8px rgba(0,0,0,.6);
  --sans:             -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --mono:             "Menlo", "Monaco", "Consolas", "SF Mono", monospace;
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
    --lane-0:           #4ec9ff;
    --lane-1:           #4ec97a;
    --lane-2:           #a3e635;
    --radius:           6px;
    --radius-card:      8px;
    --shadow-sm:        0 1px 3px rgba(0,0,0,.5);
    --shadow-md:        0 2px 8px rgba(0,0,0,.6);
    --sans:             -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
    --mono:             "Menlo", "Monaco", "Consolas", "SF Mono", monospace;
    color-scheme: dark;
  }
}

@media (prefers-color-scheme: light) {
  [data-theme="system"] {
    color-scheme: light;
  }
}

/* ── Theme-independent layout metrics ────────────────────────────────── */
:root {
  --card-pad-y:    6px;
  --card-border-w: 1px;
  --card-margin-y: 2px;
  --rail-bleed:     calc(var(--card-pad-y) + var(--card-border-w) + var(--card-margin-y));
  --topo-lane-w:   20px;
  --stack-rail-w:  240px;
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

html, body {
  height: 100%;
  font-family: var(--sans);
  font-size: 13px;
  line-height: 1.5;
  background: var(--window);
  color: var(--text);
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
  grid-template-columns: max-content 1fr 280px;
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
  width: var(--stack-rail-w, 240px);
}

.stack-header {
  padding: 10px 12px 8px;
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.stack-header-labels {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}
.stack-header-label {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .08em;
  color: var(--text-muted);
}
.stack-trunk-badge {
  font-size: 10px;
  font-weight: 600;
  font-family: var(--mono);
  color: var(--text-muted);
  background: var(--border);
  padding: 1px 6px;
  border-radius: 3px;
}
.stack-title-row {
  display: flex;
  align-items: baseline;
  gap: 8px;
}
.stack-title {
  font-size: 14px;
  font-weight: 700;
  color: var(--text);
}
.stack-meta {
  font-size: 11px;
  color: var(--text-muted);
}

.branch-cards {
  flex: 1;
  overflow-y: auto;
  padding: 8px 0;
}

/* ── Branch cards ────────────────────────────────────────────────────── */
.branch-card {
  display: flex;
  align-items: stretch;
  padding: var(--card-pad-y) 10px var(--card-pad-y) 4px;
  margin: var(--card-margin-y) 8px;
  border-radius: var(--radius-card);
  border: var(--card-border-w) solid transparent;
  position: relative;
  transition: background .1s, border-color .1s;
  gap: 6px;
}
.branch-card:hover {
  background: var(--surface-hover);
  border-color: var(--border);
}
.branch-card.selected {
  background: var(--surface-selected);
  border-color: var(--accent);
}
.branch-card.is-current .card-name { font-weight: 600; }
.branch-card.is-trunk { opacity: .8; }

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
.tc-rail.lane-0 { background: var(--lane-0); opacity: .7; }
.tc-rail.lane-1 { background: var(--lane-1); opacity: .7; }
.tc-rail.lane-2 { background: var(--lane-2); opacity: .7; }
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
  width: 10px;
  height: 10px;
  border-radius: 50%;
  position: relative;
  z-index: 3;
  flex-shrink: 0;
}
.tc-node.lane-0 { background: var(--lane-0); }
.tc-node.lane-1 { background: var(--lane-1); }
.tc-node.lane-2 { background: var(--lane-2); }
.tc-node.current {
  background: var(--accent);
  box-shadow: 0 0 0 2px color-mix(in srgb, var(--focus) 35%, transparent);
}

/* ── Card inner ──────────────────────────────────────────────────────── */
.card-inner {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.card-top {
  display: flex;
  align-items: center;
  gap: 6px;
}
.card-name {
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  flex: 1;
  min-width: 0;
}
.card-chips {
  display: flex;
  align-items: center;
  gap: 3px;
  flex-shrink: 0;
}
.card-bottom {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10px;
  color: var(--text-muted);
}
.card-ci {
  font-size: 10px;
  font-weight: 500;
}
.card-ci.ci-pass { color: var(--success); }
.card-ci.ci-fail { color: var(--danger); }
.card-ci.ci-pending { color: var(--warning); }
.card-diverge {
  font-family: var(--mono);
  font-size: 10px;
  color: var(--text);
}

/* ── Quick actions ───────────────────────────────────────────────────── */
.quick-actions {
  flex-shrink: 0;
  border-top: 1px solid var(--border);
  padding: 10px 12px;
}
.quick-actions-label {
  font-size: 10px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: .08em;
  color: var(--text-muted);
  margin-bottom: 6px;
}
.quick-action {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  border: none;
  border-radius: var(--radius);
  background: transparent;
  color: var(--text);
  font-family: var(--sans);
  font-size: 12px;
  cursor: pointer;
  transition: background .1s;
  text-align: left;
}
.quick-action:hover { background: var(--surface-hover); }
.quick-action:disabled { opacity: .4; cursor: default; pointer-events: none; }
.qa-icon {
  width: 16px;
  flex-shrink: 0;
  color: var(--text-muted);
  font-size: 12px;
}
.qa-label { flex: 1; }
.qa-key {
  font-family: var(--mono);
  font-size: 10px;
  color: var(--text-muted);
  background: var(--border);
  padding: 1px 5px;
  border-radius: 3px;
  flex-shrink: 0;
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
.btn-checkout { font-size: 10px; padding: 2px 6px; flex-shrink: 0; }

/* ── Chips / badges ──────────────────────────────────────────────────── */
.meta-chip {
  font-size: 9px;
  font-weight: 700;
  padding: 1px 5px;
  border-radius: 3px;
  line-height: 1.3;
  white-space: nowrap;
  flex-shrink: 0;
}
.meta-chip.chip-trunk { background: var(--border); color: var(--text); }
.meta-chip.chip-head { background: var(--surface-selected); color: var(--focus); }
.meta-chip.chip-diverge { background: var(--surface-hover); color: var(--text-muted); }
.meta-chip.chip-warning { background: rgba(232,180,78,.15); color: var(--warning); }
.meta-chip.chip-pr { background: var(--surface-selected); color: var(--focus); }

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
.pane-body.htmx-request {
  opacity: 0.6;
  transition: opacity 0.15s;
  pointer-events: none;
}
.htmx-request.mutating-btn { opacity: 0.5; }

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
    grid-template-columns: max-content 1fr;
    grid-template-rows: 1fr auto;
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

  // Rehydrate inspector + changes when stack pane reloads
  document.body.addEventListener('htmx:afterSwap', function(e) {
    if (e.detail && e.detail.target && e.detail.target.id === 'stack-pane') {
      htmx.trigger('#inspector-pane', 'load');
      htmx.trigger('#changes-pane', 'load');
    }
    if (e.detail && e.detail.target && e.detail.target.id === 'changes-pane') {
      initFileList();
    }
  });

  initFileList();
});

function isInput(el) {
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.contentEditable === 'true';
}

function togglePane(id, pane) {
  var el = document.getElementById(id);
  if (el) el.classList.toggle('pane-hidden');
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
            APP_CSS.contains("grid-template-columns: max-content 1fr 280px;"),
            "desktop stack track should follow the rendered stack width"
        );
        assert!(
            APP_CSS.contains("width: var(--stack-rail-w, 240px);"),
            "stack rail should use the lane-count-driven width until the stacked breakpoint"
        );
        assert!(
            !APP_CSS.contains("min(var(--stack-rail-w, 240px), 40vw)"),
            "a viewport cap before the 800px breakpoint would squash dense stacks"
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
}
