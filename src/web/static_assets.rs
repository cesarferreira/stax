//! Static asset bytes embedded at compile time.

pub const HTMX_JS: &str = include_str!("assets/htmx.min.js");

pub const APP_CSS: &str = r#"
/* stax web workspace — theme aligned with crates/stax-gui/src/theme.rs */

/* Light palette (default + forced light) */
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
  --accent:           #2b67ae;
  --accent-text:      #ffffff;
  --focus:            #1f72cf;
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
  --shadow-sm:        0 1px 3px rgba(0,0,0,.08);
  --shadow-md:        0 2px 8px rgba(0,0,0,.12);
  color-scheme: light;
}

/* Forced dark */
[data-theme="dark"] {
  --window:           #1c1c2b;
  --sidebar:          #3a3a46;
  --surface:          #242433;
  --surface-raised:   #303040;
  --surface-hover:    #41414e;
  --surface-selected: #4a4a58;
  --border:           #3a3a4a;
  --border-strong:    #515164;
  --text:             #dedeea;
  --text-muted:       #bfc0d2;
  --accent:           #aeb7ff;
  --accent-text:      #171724;
  --focus:            #94a0ff;
  --success:          #8bd5a1;
  --warning:          #e5c07b;
  --danger:           #ffb3c2;
  --diff-add:         #8bd5a1;
  --diff-del:         #ffb3c2;
  --diff-hunk:        #b8a7ff;
  --disabled-surface: #343442;
  --disabled-text:    #77778a;
  --lane-0:           #46bff7;
  --lane-1:           #4ddd9a;
  --lane-2:           #a3e635;
  --shadow-sm:        0 1px 3px rgba(0,0,0,.35);
  --shadow-md:        0 2px 8px rgba(0,0,0,.45);
  color-scheme: dark;
}

/* System follows OS preference */
@media (prefers-color-scheme: dark) {
  [data-theme="system"] {
    --window:           #1c1c2b;
    --sidebar:          #3a3a46;
    --surface:          #242433;
    --surface-raised:   #303040;
    --surface-hover:    #41414e;
    --surface-selected: #4a4a58;
    --border:           #3a3a4a;
    --border-strong:    #515164;
    --text:             #dedeea;
    --text-muted:       #bfc0d2;
    --accent:           #aeb7ff;
    --accent-text:      #171724;
    --focus:            #94a0ff;
    --success:          #8bd5a1;
    --warning:          #e5c07b;
    --danger:           #ffb3c2;
    --diff-add:         #8bd5a1;
    --diff-del:         #ffb3c2;
    --diff-hunk:        #b8a7ff;
    --disabled-surface: #343442;
    --disabled-text:    #77778a;
    --lane-0:           #46bff7;
    --lane-1:           #4ddd9a;
    --lane-2:           #a3e635;
    --shadow-sm:        0 1px 3px rgba(0,0,0,.35);
    --shadow-md:        0 2px 8px rgba(0,0,0,.45);
    color-scheme: dark;
  }
}

@media (prefers-color-scheme: light) {
  [data-theme="system"] {
    color-scheme: light;
  }
}

*, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

html, body {
  height: 100%;
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  font-size: 13px;
  line-height: 1.5;
  background: var(--window);
  color: var(--text);
}

/* ── Layout ─────────────────────────────────────────────── */
.workspace {
  display: flex;
  flex-direction: column;
  height: 100vh;
}
.toolbar {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  background: var(--sidebar);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.toolbar .repo-label {
  font-weight: 600;
  color: var(--text);
  margin-right: 8px;
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  max-width: 300px;
}
.pane-area {
  display: flex;
  flex: 1;
  overflow: hidden;
}
.pane {
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border-right: 1px solid var(--border);
  background: var(--surface);
}
.pane:last-child { border-right: none; }
.pane-stack  { width: 280px; min-width: 200px; flex-shrink: 0; }
.pane-changes { flex: 1; min-width: 200px; }
.pane-inspector { width: 340px; min-width: 220px; flex-shrink: 0; }
.pane-header {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: .03em;
  text-transform: uppercase;
  color: var(--text-muted);
  background: var(--surface);
  border-bottom: 1px solid var(--border);
  flex-shrink: 0;
}
.pane-body { flex: 1; overflow-y: auto; padding: 6px 0; }

/* ── Branch rows ──────────────────────────────────────────── */
.branch-row {
  display: flex;
  align-items: center;
  padding: 5px 12px;
  cursor: pointer;
  user-select: none;
  gap: 6px;
}
.branch-row:hover { background: var(--surface-hover); }
.branch-row.selected { background: var(--surface-selected); }
.branch-row.is-current .branch-name { font-weight: 600; }
.branch-name {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 12px;
}
.branch-label {
  font-size: 10px;
  padding: 1px 5px;
  border-radius: 3px;
  background: var(--border);
  color: var(--text-muted);
  flex-shrink: 0;
}
.branch-label.needs-restack { background: #fff3cd; color: var(--warning); }
.branch-label.pr-open { background: var(--surface-selected); color: var(--accent); }

/* ── Topology ─────────────────────────────────────────────── */
.topo-grid { display: flex; align-items: center; flex-shrink: 0; gap: 0; }
.topo-cell {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 20px;
  position: relative;
  flex-shrink: 0;
}
.topo-node {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--lane-0);
  z-index: 1;
  flex-shrink: 0;
}
.topo-node.current { background: var(--accent); box-shadow: 0 0 0 2px var(--focus); }
.topo-node.lane-1 { background: var(--lane-1); }
.topo-node.lane-2 { background: var(--lane-2); }
.topo-connector-v {
  position: absolute;
  left: 50%; top: 0; bottom: 0;
  width: 2px;
  transform: translateX(-50%);
  background: var(--border-strong);
  z-index: 0;
}
.topo-connector-h-left {
  position: absolute;
  right: 50%; top: 50%;
  height: 2px;
  left: 0;
  transform: translateY(-50%);
  background: var(--border-strong);
  z-index: 0;
}
.topo-connector-h-right {
  position: absolute;
  left: 50%; top: 50%;
  height: 2px;
  right: 0;
  transform: translateY(-50%);
  background: var(--border-strong);
  z-index: 0;
}

/* ── Toolbar buttons ─────────────────────────────────────── */
.btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 10px;
  font-size: 12px;
  font-weight: 500;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  cursor: pointer;
  transition: background .1s, border-color .1s;
  white-space: nowrap;
}
.btn:hover { background: var(--surface-hover); border-color: var(--border-strong); }
.btn:disabled { opacity: .45; cursor: default; pointer-events: none; }
.btn-primary {
  background: var(--accent);
  color: var(--accent-text);
  border-color: var(--accent);
}
.btn-primary:hover { opacity: .9; }
.btn-danger { border-color: var(--danger); color: var(--danger); }
.btn-danger:hover { background: var(--danger); color: #fff; }
.btn-icon { padding: 4px 6px; }

/* ── Search ──────────────────────────────────────────────── */
.search-input {
  flex: 1;
  max-width: 220px;
  padding: 4px 8px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-size: 12px;
  outline: none;
}
.search-input:focus { border-color: var(--focus); box-shadow: 0 0 0 2px rgba(31,114,207,.25); }

/* ── Inspector ──────────────────────────────────────────── */
.inspector-section { padding: 12px; border-bottom: 1px solid var(--border); }
.inspector-label { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: .04em; color: var(--text-muted); margin-bottom: 8px; }
.inspector-row { display: flex; align-items: baseline; justify-content: space-between; margin-bottom: 4px; font-size: 12px; }
.inspector-key { color: var(--text-muted); }
.inspector-value { font-weight: 500; text-align: right; }

/* ── Diff ────────────────────────────────────────────────── */
.diff-view { font-family: "Menlo", "Monaco", "Consolas", monospace; font-size: 11px; line-height: 1.5; overflow: auto; flex: 1; }
.diff-line { padding: 0 12px; white-space: pre; }
.diff-add { color: var(--diff-add); background: rgba(31,122,63,.07); }
.diff-del { color: var(--diff-del); background: rgba(178,56,50,.07); }
.diff-hunk { color: var(--diff-hunk); }
.diff-header { color: var(--text-muted); font-weight: 600; padding-top: 8px; }

/* ── Overlays ────────────────────────────────────────────── */
.overlay-backdrop {
  position: fixed; inset: 0;
  background: rgba(0,0,0,.35);
  display: flex; align-items: center; justify-content: center;
  z-index: 100;
}
.overlay-card {
  background: var(--surface-raised);
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 20px 24px;
  min-width: 340px;
  max-width: 520px;
  box-shadow: var(--shadow-md);
}
.overlay-title { font-weight: 600; font-size: 14px; margin-bottom: 12px; }
.overlay-actions { display: flex; gap: 8px; justify-content: flex-end; margin-top: 16px; }

/* ── Banners ─────────────────────────────────────────────── */
.banner {
  padding: 8px 12px;
  font-size: 12px;
  display: flex;
  align-items: center;
  gap: 8px;
}
.banner-success { background: rgba(40,122,69,.12); color: var(--success); border-bottom: 1px solid rgba(40,122,69,.2); }
.banner-error   { background: rgba(177,58,54,.1);  color: var(--danger);  border-bottom: 1px solid rgba(177,58,54,.2); }
.banner-info    { background: rgba(43,103,174,.1); color: var(--accent);  border-bottom: 1px solid rgba(43,103,174,.2); }

/* ── Loading skeleton ─────────────────────────────────────── */
.skeleton { background: var(--border); border-radius: 4px; height: 12px; margin-bottom: 8px; animation: pulse 1.4s infinite; }
@keyframes pulse { 0%, 100% { opacity: 1 } 50% { opacity: .4 } }

/* ── Misc ─────────────────────────────────────────────────── */
.text-muted { color: var(--text-muted); }
.text-danger { color: var(--danger); }
.text-success { color: var(--success); }
.text-warning { color: var(--warning); }
.spacer { flex: 1; }
.project-form { display: flex; align-items: center; gap: 6px; }
.project-select,
.theme-select {
  max-width: 180px;
  padding: 4px 8px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-size: 12px;
}
.theme-select { max-width: 110px; }
.project-add {
  width: 140px;
  padding: 4px 8px;
  border-radius: var(--radius);
  border: 1px solid var(--border);
  background: var(--surface-raised);
  color: var(--text);
  font-size: 12px;
}
.op-spinner { display: inline-block; width: 12px; height: 12px; border: 2px solid var(--border); border-top-color: var(--accent); border-radius: 50%; animation: spin .6s linear infinite; }
@keyframes spin { to { transform: rotate(360deg) } }
form { display: contents; }
input[type="text"] { padding: 4px 8px; border-radius: var(--radius); border: 1px solid var(--border); background: var(--surface-raised); color: var(--text); font-size: 12px; width: 100%; }
input[type="text"]:focus { outline: none; border-color: var(--focus); }
.flex { display: flex; }
.flex-col { display: flex; flex-direction: column; }
.gap-2 { gap: 8px; }
.mt-2 { margin-top: 8px; }
.pane-hidden { display: none !important; }
"#;

pub const APP_JS: &str = r#"
// st web — minimal inline JS for keyboard shortcuts and focus
document.addEventListener('DOMContentLoaded', function() {
  // '/' focuses search
  document.addEventListener('keydown', function(e) {
    if (e.key === '/' && !isInput(e.target)) {
      e.preventDefault();
      const s = document.getElementById('search-input');
      if (s) { s.focus(); s.select(); }
    }
    if (e.key === 'Escape') {
      const backdrop = document.querySelector('.overlay-backdrop');
      if (backdrop) backdrop.remove();
      const s = document.getElementById('search-input');
      if (s && document.activeElement === s) { s.blur(); }
    }
    // 1/2/3 toggle pane visibility (optimistic UI + persist to server)
    if (!isInput(e.target)) {
      if (e.key === '1') togglePane('pane-stack', 'stack');
      if (e.key === '2') togglePane('pane-changes', 'changes');
      if (e.key === '3') togglePane('pane-inspector', 'inspector');
    }
  });
});

function isInput(el) {
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.contentEditable === 'true';
}

function togglePane(id, pane) {
  const el = document.getElementById(id);
  if (el) el.classList.toggle('pane-hidden');
  const csrf = document.querySelector('input[name="csrf"]');
  const base = location.pathname.replace(/\/?$/, '');
  if (csrf && pane) {
    const body = new URLSearchParams({ pane: pane, csrf: csrf.value });
    fetch(base + '/panes', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: body.toString(),
    }).catch(function() {});
  }
}

// Disable mutating buttons while HTMX request is in flight
document.addEventListener('htmx:beforeRequest', function(e) {
  if (e.target.classList.contains('mutating-btn')) {
    document.querySelectorAll('.mutating-btn').forEach(b => b.disabled = true);
  }
});
document.addEventListener('htmx:afterRequest', function(e) {
  document.querySelectorAll('.mutating-btn').forEach(b => b.disabled = false);
});
"#;
