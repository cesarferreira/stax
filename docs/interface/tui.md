# Interactive TUI

Run `st` with no arguments to open the terminal UI.

```bash
stax
```

![stax TUI](../assets/tui.png)

## Features

- Full-height stack tree with PR status, sync indicators, and ahead/behind counts
- Selected-branch summary with recommended next actions
- Patch viewer with a compact diffstat header and scrollable patch body
- Keyboard-driven checkout, restack, submit, create, rename, and delete
- Reorder mode for branch reparenting

The main `st` TUI is focused on stacks and patches. Worktree management lives in the dedicated `st wt` dashboard.

## Worktree Dashboard

Run `st wt` in an interactive terminal to open the worktree dashboard.

- Left pane: all Git worktrees, including unmanaged entries
- Right pane: branch/base/path/status details plus tmux session state
- `Enter`: attach or switch to the derived tmux session for the selected worktree
- `c`: create a lane and open it in tmux
- `d`: remove the selected worktree
- `R`: restack all stax-managed worktrees
- `?`: show help
- `q`/`Esc`: quit
- The footer keeps these shortcuts visible and highlights the key itself in the TUI

## Keybindings

| Key | Action |
|---|---|
| `j/k` or `↑/↓` | Navigate branches |
| `Enter` | Checkout branch |
| `r` | Restack selected branch |
| `R` (Shift+r) | Restack all branches in stack |
| `s` | Submit stack |
| `p` | Open selected branch PR |
| `o` | Enter reorder mode |
| `n` | Create branch |
| `e` | Rename current branch |
| `d` | Delete branch |
| `/` | Search/filter branches |
| `Tab` | Toggle focus between stack and patch panes |
| `?` | Show keybindings |
| `q`/`Esc` | Quit |

## Reorder Mode

![Reorder mode](../assets/reordering-stacks.png)

1. Select a branch and press `o`
2. Move with `Shift+↑/↓`
3. Review previewed reparent operations
4. Press `Enter` to apply and restack

## Split Mode

Split a branch with many commits into multiple stacked branches.

```bash
st split
```

| Key | Action |
|---|---|
| `j/k` or `↑/↓` | Navigate commits |
| `s` | Add split point at cursor |
| `d` | Remove split point |
| `S-J/K` | Move split point down/up |
| `Enter` | Execute split |
| `?` | Toggle help |
| `q`/`Esc` | Cancel |

Split operations are transactional and recoverable with `st undo`.

## Hunk Split Mode

Split a branch by selecting individual diff hunks rather than whole commits. Useful when a branch has a single commit that should be broken into smaller stacked branches.

```bash
st split --hunk
```

The hunk picker shows a two-pane layout: file/hunk list on the left, diff preview on the right. Splitting happens in rounds — select hunks, name the new branch, repeat until all hunks are assigned.

### List mode (default)

| Key | Action |
|---|---|
| `j/k` or `↑/↓` | Navigate files and hunks |
| `Space` | Toggle hunk selection |
| `a` | Toggle all hunks in current file |
| `u` | Undo last selection change |
| `Tab` | Switch to sequential mode |
| `Enter` | Finish round (proceed to naming) |
| `?` | Show help |
| `q`/`Esc` | Abort split |

### Sequential mode

Step through hunks one by one with yes/no prompts. Press `Tab` to return to list mode.

| Key | Action |
|---|---|
| `y` | Accept hunk and advance |
| `n` | Skip hunk and advance |
| `a` | Toggle file and jump to next file |
| `u` | Undo |
| `Enter` | Finish round |

### Naming

After finishing a round, enter a branch name for the selected hunks. The default suggestion is `<original>_split_N` for intermediate rounds and the original branch name for the final round.

### Workflow

1. Select hunks for the first new branch
2. Press `Enter`, confirm the branch name
3. Repeat for remaining hunks
4. After the last round, stax creates the branch chain and updates stack metadata

Hunk split operations are transactional and recoverable with `st undo`.
