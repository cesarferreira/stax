# Stax Skills for Claude Code

This document teaches Claude Code how to use `stax` - a CLI for managing stacked Git branches and PRs.

## What is Stax?

Stax manages **stacked branches** - a workflow where you build small, focused branches on top of each other instead of one massive PR. Each branch becomes a separate PR that targets its parent branch.

## Core Concepts

- **Stack**: A chain of branches where each builds on the previous one
- **Trunk**: The main branch (usually `main` or `master`)
- **Parent**: The branch that a stacked branch is based on
- **Tracked branch**: A branch with stax metadata (parent info, PR info)

## Essential Commands

### View Status
```bash
stax ls              # Simple tree view of your stack
stax status          # Same as ls
stax log             # Detailed view with commits and PR info
stax ll              # Show PR URLs
```

### Create Branches
```bash
stax create <name>           # Create branch stacked on current
stax bc <name>               # Shorthand for create
stax create -m "message"     # Create with commit message
stax create -a               # Stage all changes before creating
stax create -am "message"    # Stage all and commit
```

### Navigate Stack
```bash
stax u                # Move up to child branch
stax d                # Move down to parent branch
stax u 3              # Move up 3 branches
stax top              # Jump to top of stack
stax bottom           # Jump to base of stack (first above trunk)
stax t                # Jump to trunk (main)
stax co               # Interactive branch picker
stax co <branch>      # Checkout specific branch
```

### Submit PRs
```bash
stax ss               # Submit stack - push and create/update PRs
stax submit           # Same as ss
stax branch submit    # Submit only current branch
stax bs               # Alias for branch submit
stax upstack submit   # Submit current branch + descendants
stax downstack submit # Submit ancestors + current branch
stax ss --draft       # Create PRs as drafts
stax ss --reviewers alice,bob    # Add reviewers
stax ss --labels bug,urgent      # Add labels
```

### Sync & Rebase
```bash
stax rs               # Sync - pull trunk, delete merged branches
stax sync             # Same as rs
stax rs --restack     # Sync and rebase all branches
stax restack          # Rebase current branch onto parent
stax restack --all    # Rebase all branches needing it
```

### Merge PRs
```bash
stax merge            # Merge PRs from bottom of stack to current
stax merge --all      # Merge entire stack
stax merge --dry-run  # Preview without merging
stax merge --method squash   # Squash merge (default)
```

### Modify Code
```bash
stax m                # Stage all + amend current commit
stax modify           # Same as m
stax m -m "new msg"   # Amend with new message
```

### Branch Management
```bash
stax branch track --parent main     # Track existing branch
stax branch track --all-prs         # Import all your open PRs
stax branch untrack <name>          # Remove stax metadata only
stax branch reparent --parent new   # Change parent
stax branch delete <name>           # Delete branch
stax branch rename <name>           # Rename current branch
stax branch fold                    # Fold into parent
stax branch squash                  # Squash commits on branch
```

### Recovery
```bash
stax undo             # Undo last operation
stax redo             # Redo last undone operation
stax continue         # Continue after resolving conflicts
```

### Utilities
```bash
stax pr               # Open PR in browser
stax open             # Open repo in browser
stax copy             # Copy branch name to clipboard
stax copy --pr        # Copy PR URL to clipboard
stax ci               # Show CI status for stack
stax standup          # Show recent activity summary
stax doctor           # Check repo health
```

## Common Workflows

### Starting a New Feature Stack
```bash
stax t                        # Go to trunk
stax rs                       # Sync with remote
stax create api-layer         # Create first branch
# ... make changes ...
stax m                        # Amend changes to commit
stax create ui-layer          # Stack another branch on top
# ... make changes ...
stax m
stax ss                       # Submit all PRs
```

### After PR Review - Making Changes
```bash
stax co <branch>              # Go to branch needing changes
# ... make fixes ...
stax m                        # Amend the commit
stax ss                       # Re-push (updates PR)
```

### After Base PR is Merged
```bash
stax rs --restack             # Sync trunk, rebase remaining branches
stax ss                       # Update PR targets
```

### Importing Existing PRs
```bash
stax branch track --all-prs   # Import all your open PRs from GitHub
```

### Handling Rebase Conflicts
```bash
stax restack                  # Start rebase
# ... resolve conflicts in editor ...
git add -A                    # Stage resolved files
stax continue                 # Continue rebase
```

### Undoing a Mistake
```bash
stax undo                     # Restore previous state
stax undo --no-push           # Undo locally only
```

## Reading Stack Output

```
◉  feature/validation 1↑        # ◉ = current branch, 1↑ = 1 commit ahead
○  feature/auth 1↓ 2↑ ⟳         # ○ = other branch, ⟳ = needs restack
│ ○    ☁ feature/payments PR #42  # ☁ = has remote, PR #42 = open PR
○─┘    ☁ main                   # trunk branch
```

Symbols:
- `◉` = Current branch
- `○` = Other branch
- `☁` = Has remote tracking
- `↑` = Commits ahead of parent
- `↓` = Commits behind parent
- `⟳` = Needs restacking (parent changed)
- `PR #N` = Has open PR

## Best Practices

1. **Keep branches small** - Each branch should be a focused, reviewable unit
2. **Use descriptive names** - Branch names become PR titles
3. **Sync frequently** - Run `stax rs` to stay up to date
4. **Restack after merges** - Run `stax rs --restack` after PRs merge
5. **Amend, don't commit** - Use `stax m` to add changes to existing commit
6. **Check before submit** - Use `stax ls` to review stack before `stax ss`

## Tips

- Run `stax` with no args to launch the interactive TUI
- Use `stax --help` or `stax <command> --help` for detailed help
- The `bc`, `bu`, `bd` shortcuts work for quick branch creation and navigation
- Use `--yes` flag to skip confirmation prompts in scripts
- Use `--json` flag for machine-readable output
