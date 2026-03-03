# Navigation and Stack View

## Navigation commands

| Command | What it does |
|---|---|
| `st u` | Move up to child branch |
| `st d` | Move down to parent branch |
| `st u 3` | Move up 3 branches |
| `st d 2` | Move down 2 branches |
| `st top` | Jump to stack tip |
| `st bottom` | Jump to stack base |
| `st trunk` / `st t` | Jump to trunk |
| `st prev` | Toggle to previous branch |
| `st co` | Interactive branch picker |

## Checkout shortcuts

Use `st checkout` (or `st co`) with navigation flags:

- `st checkout --trunk` jump directly to trunk
- `st checkout --parent` jump to parent of current branch
- `st checkout --child 1` jump to first child branch

## Reading `st ls`

```text
○        feature/validation 1↑
◉        feature/auth 1↓ 2↑ ⟳
│ ○    ☁ feature/payments PR #42
○─┘    ☁ main
```

| Symbol | Meaning |
|---|---|
| `◉` | Current branch |
| `○` | Other tracked branch |
| `☁` | Remote tracking exists |
| `1↑` | Commits ahead of parent |
| `1↓` | Commits behind parent |
| `⟳` | Needs restack |
| `PR #42` | Open pull request |
