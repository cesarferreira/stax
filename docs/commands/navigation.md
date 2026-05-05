# Navigation and stack view

## Move around the stack

| Command | What it does |
|---|---|
| `st u [n]` | Move up `n` children (default 1) |
| `st d [n]` | Move down `n` parents (default 1) |
| `st top` | Jump to stack tip |
| `st bottom` | Jump to stack base |
| `st trunk` / `st t` | Jump to trunk |
| `st trunk <branch>` | Set trunk to `<branch>` |
| `st prev` | Toggle to previous branch |
| `st co` | Interactive branch picker |

## Checkout shortcuts

```bash
st checkout --trunk       # jump to trunk
st checkout --parent      # jump to parent
st checkout --child 1     # jump to first child
```

## Reading `st ls`

```text
○        feature/validation 1↑
◉        feature/auth       2↑ 1↓ ⟳
○        feature/old-base   (missing parent: feature/base)
│ ○    ☁ feature/payments   PR #42
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
| `(missing parent: X)` | Metadata points to a deleted or missing parent; run `st fix --yes` |
| `PR #42` | Open pull request |
