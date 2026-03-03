# Interactive TUI

Run `st` with no arguments to open the terminal UI.

```bash
stax
```

![stax TUI](../assets/tui.png)

## Features

- Stack tree with PR status, sync indicators, and ahead/behind counts
- Branch diff viewer
- Keyboard-driven checkout, restack, submit, create, rename, and delete
- Reorder mode for branch reparenting

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
| `Tab` | Toggle focus between stack and diff panes |
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
