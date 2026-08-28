# Repository Board Dashboard

Run `st board` (or the alias `st home`) to open a full-screen dashboard of open
pull requests and issues for the current repository.

```bash
stax board
```

GitHub only: on GitLab or Gitea remotes, `st board` errors and points you at
`st pr list` / `st issue list` instead.

## Tabs

- **PULL REQUESTS** — open PRs sorted by most recently updated.
- **ISSUES** — open issues (pull requests excluded), sorted by most recently
  updated.

`Tab`, `1`, and `2` switch between them; each tab remembers its own selection.

By default both tabs are filtered to items authored by the current GitHub
user ("mine only"). Press `a` to toggle between "mine" and "all"; the choice
is saved to `~/.config/stax/config.toml` (`[board] mine_only`) and reused
next time. The header shows `(3/12)` when the filter is hiding items, and a
`· mine` marker while it's active. Until your GitHub login is resolved (a
one-time lookup at startup), the filter fails open and shows everything
rather than an empty list.

## Detail pane

The detail pane is a live preview of whatever row is currently selected — it
loads automatically as you move through the list, no need to press `Enter`
first (on terminals narrower than 100 columns it's hidden and the list takes
the full width instead). Once fetched, a PR or issue's detail, files, CI
checks, diff, and comments stay cached for the rest of the session, so
revisiting something you already viewed doesn't re-fetch it — press `r` to
force a refresh. Press `v` to hide the pane entirely (falls back to a
list-only layout regardless of terminal width, and also stops the automatic
prefetching); `Enter` or `v` again brings it back.

- **PR detail**: branch (`head → base`), files changed with `+`/`-` counts,
  per-check CI status (`✓` success/neutral/skipped, `✗` failure/timed
  out/cancelled/action-required, `•` still running), labels, comment count,
  and the PR body.
- **Issue detail**: labels, comment count, and the issue body.

## Overlays

| Key | Opens |
|---|---|
| `d` | Inline diff viewer (pull requests only) |
| `c` | Full comment thread (issue comments + review comments for PRs) |
| `l` | Label picker — `Space` toggles a label on/off, `Esc` closes |
| `m` | Merge confirmation (pull requests only, see below) |
| `?` | Help |

## Actions

- `t` toggles a PR between draft and ready for review.
- `m` opens a confirmation, then performs a **squash merge via the GitHub API
  only**. Unlike `st merge`, this does not rebase descendants, retarget PR
  bases, or delete branches — run `st sync` afterwards to bring your local
  stack up to date.
- `o` opens the selected PR or issue in your browser.
- `r` refreshes the current view.
- `/` filters the current tab's list by number, title, author, branch (PRs),
  or labels.

## Keybindings

| Key | Action |
|---|---|
| `Tab` / `1` / `2` | Switch between PULL REQUESTS and ISSUES |
| `j/k` or `↑/↓` | Move selection |
| `g` / `G` | Jump to top / bottom |
| `Ctrl-d` / `Ctrl-u` | Page down / up |
| `Enter` | Open detail |
| `d` | Diff (PRs) |
| `c` | Comments |
| `l` | Label picker |
| `t` | Toggle draft (PRs) |
| `m` | Merge (PRs, opens confirmation) |
| `o` | Open in browser |
| `r` | Refresh |
| `/` | Filter |
| `v` | Toggle the detail pane on/off |
| `a` | Toggle "mine only" / "all" (persisted) |
| `?` | Help |
| `q` / `Esc` | Back one mode, or quit from the list |

Note the divergence from the main `st` TUI and `st ready`: there, `d` toggles
draft status. In the board dashboard `d` opens the diff viewer instead — `t`
handles the draft toggle here, chosen so no binding depends on Shift state
(terminals disagree on whether Shift+key arrives as the plain or the
uppercase character).

## Flags

| Flag | Description |
|---|---|
| `--limit <N>` | Maximum PRs/issues to list per tab (default 30, max 100) |
| `--tab prs\|issues` | Tab to open on launch (default: `prs`) |
| `--interval <seconds>` | Auto-refresh interval for the interactive dashboard (default: 60) |
| `--plain` | Render static PR and issue tables instead of the interactive dashboard (also used automatically when stdin/stdout aren't a TTY) |
