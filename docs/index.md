# stax

A modern CLI for stacked Git branches and PRs.

![stax screenshot](assets/screenshot.png)

## Why stax?

- Blazing fast Rust binary (~22ms `st ls` on a 10-branch stack)
- Interactive TUI with tree view, PR status, diffs, reorder mode, and split mode
- Submit/update complete stacks with correct PR bases in one command
- Transactional history operations with `st undo` and `st redo`
- Freephite-compatible metadata, so existing stacks can be used immediately

## Start Here

1. [Install](getting-started/install.md)
2. [Quick Start](getting-started/quick-start.md)
3. [Core Commands](commands/core.md)

## Common Tasks

- Learn stacked-branch flow: [Stacked Branches](concepts/stacked-branches.md)
- Work interactively in terminal: [Interactive TUI](interface/tui.md)
- Merge an entire stack safely: [Merge and Cascade](workflows/merge-and-cascade.md)
- Manage worktree lanes and the `st wt` dashboard: [Worktrees](worktrees/index.md)
- Understand repo-wide linked-checkout behavior: [Multi-Worktree Behavior](workflows/multi-worktree.md)
- Run parallel AI sessions in worktree lanes: [AI Worktree Lanes](workflows/agent-worktrees.md)
- Recover from risky rewrites: [Undo and Redo](safety/undo-redo.md)
- Configure auth and naming: [Config Reference](configuration/index.md)
- Validate and repair stack health: [Stack Health](commands/stack-health.md)
