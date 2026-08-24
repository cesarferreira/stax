# st web — Localhost Web Workspace

`st web` starts a fast, secure, browser-based workspace for stax on `127.0.0.1`. The layout is inspired by GitKraken Desktop: a grouped toolbar, stack graph as the hero pane, a file-list + patch Changes panel, branch Details inspector, and a status bar — all powered by [HTMX](https://htmx.org/) for partial-page updates without a heavy JavaScript framework.

## Quick start

```bash
st web            # opens http://127.0.0.1:8787/s/<token>/ in your browser
st web --port 0   # ephemeral port (chosen by OS)
st web --no-open  # start without opening browser; prints URL
st web /path/to/repo  # open a specific repository
```

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `[PATH]` | current directory | Repository to open |
| `--port <N>` | `8787` | Port to bind. `0` picks an ephemeral port. Fails if already in use (unless `0`). |
| `--no-open` | false | Skip opening the browser; just print the URL. |

## Layout

```
┌─ Toolbar: repo · search │ Restack Submit │ Open PR ↶ ↷ ↺ │ theme ? ─────────────┐
├──────────────┬──────────────────────────────┬─────────────────────────────────┤
│ Graph   [1]  │ Changes                 [2]  │ Details                     [3] │
│ topology│branch│Δ│PR│  file list (stat)    │ Branch / divergence / actions   │
│  ● main      │  foo.rs  +12 −3            │                                 │
│  ○ feat/x    │  ─────────────────         │                                 │
│              │  patch hunks below         │                                 │
├──────────────┴──────────────────────────────┴─────────────────────────────────┤
│ Status: HEAD main · Selected feat/x · Δ parent 3↑ 0↓ · PR #42                   │
└─────────────────────────────────────────────────────────────────────────────────┘
```

Press `1`, `2`, `3` to toggle each pane.

### Panes

| Pane | Role |
|------|------|
| **Graph** | Stack topology table: lane graph, branch name, meta chips (HEAD, ahead/behind vs parent, restack, PR). Selecting a row does **not** check out — use the `co` button. |
| **Changes** | GitKraken-style commit panel: clickable file list from diffstat on top, patch below. Empty state shows “No changes vs parent” instead of a blank pane. |
| **Details** | Branch metadata, divergence, commits, and actions (create, rename, delete, move, reorder, restack). |
| **Status bar** | Current HEAD, selected branch, ahead/behind vs parent, restack/PR badges. Refreshes with the stack. |

## Appearance

The toolbar **System / Light / Dark** control sets the workspace theme:

| Mode | Behavior |
|------|----------|
| **System** (default) | Follows the OS `prefers-color-scheme` setting |
| **Light** | Forces the light palette (Stax.app light tokens) |
| **Dark** | Forces the dark palette (Stax.app dark tokens) |

The preference is stored per repository in `.git/stax/web-state.json`.

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `/` | Focus the search box |
| `1` | Toggle stack pane |
| `2` | Toggle changes pane |
| `3` | Toggle inspector pane |
| `Esc` | Dismiss overlay / blur search |

## Available operations (via toolbar / branch row)

- **Checkout** — click a branch row's `co` button to check it out
- **Restack** — restack the selected branch onto its parent
- **Submit** — push the stack and create/update PRs (draft mode)
- **Undo / Redo** — restore local transaction state
- **Refresh** — reload the repository snapshot (also rehydrates Changes + Details)
- **Rename** — rename the current branch (overlay)
- **Create** — create a new branch stacked on the selected one (overlay)
- **Delete** — delete a non-current branch (overlay with confirmation)
- **Move** — reparent a branch subtree (POST `/op/move`)

## Security model

- Binds **127.0.0.1 only** — not accessible on the network.
- Every URL contains an **unguessable 48-hex session token**: `/s/<token>/…`.
- Every mutating POST requires a matching **CSRF token** (`csrf` form field). Requests with wrong CSRF return `403 Forbidden`.
- Non-local `Host` / `Origin` headers are rejected with `403 Forbidden`.
- Only **one mutation at a time** — mutating controls are disabled while an operation is in flight.
- `--host` flag is intentionally absent — you cannot expose this server to the network.

## Routes reference

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/assets/app.css` | Embedded CSS (light, dark, and system themes) |
| `GET` | `/assets/htmx.min.js` | Embedded htmx 2.x |
| `GET` | `/assets/app.js` | Keyboard shortcuts, pane rehydration, file-list navigation |
| `GET` | `/s/:token/` | Full workspace page |
| `GET` | `/s/:token/stack?branch=` | Stack pane partial (+ status bar OOB swap) |
| `POST` | `/s/:token/select` | Update selected branch |
| `GET` | `/s/:token/details` | Inspector hydration |
| `GET` | `/s/:token/diff` | Diff view (file list + patch, or empty state) |
| `GET` | `/s/:token/ci` | CI summary |
| `POST` | `/s/:token/search` | Filter branches |
| `POST` | `/s/:token/panes` | Toggle pane visibility |
| `POST` | `/s/:token/theme` | Set appearance (`system` / `light` / `dark`) |
| `POST` | `/s/:token/refresh` | Reload snapshot |
| `POST` | `/s/:token/op/checkout` | Check out a branch |
| `POST` | `/s/:token/op/create` | Create a branch |
| `POST` | `/s/:token/op/rename` | Rename a branch |
| `POST` | `/s/:token/op/delete` | Delete a branch |
| `POST` | `/s/:token/op/restack` | Restack |
| `POST` | `/s/:token/op/submit` | Submit (draft PRs) |
| `POST` | `/s/:token/op/undo` | Undo last transaction |
| `POST` | `/s/:token/op/redo` | Redo last transaction |
| `POST` | `/s/:token/op/move` | Move branch subtree |
| `GET` | `/s/:token/op/open-pr?branch=` | Redirect to PR URL |

## Architecture

```
src/web/
  mod.rs          — entry point, run_server(), start_test_server()
  server.rs       — Axum server bind logic
  session.rs      — WebSession shared state (Arc<Mutex<…>>)
  routes.rs       — all Axum handlers; CSRF + host guards
  templates.rs    — maud HTML templates
  static_assets.rs — embedded CSS/JS via include_str!
  assets/
    htmx.min.js   — htmx 2.x (embedded at compile time)
```

All mutations run in `tokio::task::spawn_blocking` to avoid blocking the async runtime.
