# Stax Skills for AI Coding Agents

This document teaches AI coding agents (Claude Code, Codex, Gemini CLI, OpenCode) how to use `stax` to manage stacked Git branches and PRs.

## Use with Gemini CLI

Gemini CLI reads project instructions from `GEMINI.md`. To use this guidance with Gemini in a repo:

```bash
curl -o GEMINI.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

## Use with OpenCode

OpenCode loads skills from `~/.config/opencode/skills/<name>/SKILL.md`. To install this guidance:

```bash
mkdir -p ~/.config/opencode/skills/stax
curl -o ~/.config/opencode/skills/stax/SKILL.md https://raw.githubusercontent.com/cesarferreira/stax/main/skills.md
```

## What is Stax?

Stax manages stacked branches: small focused branches layered on top of each other. Each branch maps to one PR targeting its parent branch.

## Core Concepts

- **Stack**: A chain of branches where each branch builds on its parent
- **Trunk**: The main branch (`main` or `master`)
- **Parent**: The branch a stacked branch is based on
- **Tracked branch**: A branch with stax metadata (parent and PR linkage)

## Command Map

```bash
stax status|s|ls              # Stack status (tree)
stax ll                        # Stack status with PR URLs/details
stax log|l                     # Stack status with commits + PR info

stax submit|ss                 # Submit full stack
stax merge                     # Merge PRs from stack bottom upward
stax sync|rs                   # Sync trunk + clean merged branches
stax restack                   # Rebase branch/stack onto parents
stax cascade                   # Restack bottom-up and submit updates

stax checkout|co|bco           # Checkout branch (interactive by default)
stax trunk|t                   # Checkout trunk
stax up|u [n]                  # Move to child branch
stax down|d [n]                # Move to parent branch
stax top                       # Move to stack tip
stax bottom                    # Move to first branch above trunk
stax prev|p                    # Checkout previous branch

stax branch ...|b              # Branch subcommands
stax upstack ...|us            # Descendant-scope commands
stax downstack ...|ds          # Ancestor-scope commands

stax create|c                  # Create stacked branch
stax modify|m                  # Stage all + amend commit
stax rename                    # Rename current branch
stax detach                    # Remove branch from stack, reparent children
stax reorder                   # Interactive stack reorder
stax split                     # Interactive branch split into stack

stax continue|cont             # Continue after conflict resolution
stax abort                     # Abort in-progress rebase/conflict flow
stax undo [op-id]              # Undo last/specific operation
stax redo [op-id]              # Redo last/specific undone operation

stax pr                        # Open current branch PR
stax open                      # Open repo in browser
stax comments                  # Show current PR comments
stax copy [--pr]               # Copy branch name or PR URL
stax ci                        # CI status
stax standup                   # Recent activity summary
stax changelog <from> [to]     # Changelog between refs
stax generate --pr-body        # AI PR body generation

stax auth [status]             # GitHub auth setup/status
stax config                    # Print config path + contents
stax doctor                    # Health checks
stax validate                  # Validate stack metadata
stax fix                       # Auto-repair metadata
stax test <cmd...>             # Run command on each branch
stax demo                      # Interactive tutorial
```

## High-Value Commands and Flags

### Create and Edit Branches

```bash
stax create <name>                 # Create branch stacked on current
stax create -m "message"           # Use commit message
stax create -a                     # Stage all before creating
stax create -am "message"          # Stage all + commit
stax create --from <branch>        # Create from explicit base
stax create --prefix feature/      # Override branch prefix
stax bc <name>                     # Hidden shortcut alias

stax m                             # Stage all + amend current commit
stax m -m "new msg"                # Amend with a new commit message

stax rename <name>                 # Rename current branch
stax rename --edit                 # Edit commit message while renaming
stax rename --push                 # Push renamed branch + cleanup remote

stax detach [branch] --yes         # Remove branch from stack, keep descendants
stax reorder --yes                 # Reorder stack interactively
stax split                         # Split current branch into multiple stacked branches
```

### Submit, Merge, Sync, Restack

```bash
stax submit                        # Submit full stack
stax ss                            # Alias for submit
stax submit --draft                # Create draft PRs
stax submit --no-pr                # Push only (no PR create/update)
stax submit --no-fetch             # Skip git fetch
stax submit --open                 # Open current PR after submit
stax submit --reviewers a,b        # Set reviewers
stax submit --labels bug,urgent    # Set labels
stax submit --assignees alice      # Set assignees
stax submit --template backend     # Use named PR template
stax submit --no-template          # Skip template picker
stax submit --edit                 # Always edit PR body
stax submit --ai-body              # Generate PR body with AI
stax submit --rerequest-review     # Re-request existing reviewers on update

stax branch submit                 # Submit current branch only
stax bs                            # Hidden shortcut alias for branch submit
stax upstack submit                # Submit current + descendants
stax downstack submit              # Submit ancestors + current

stax merge --all                   # Merge whole stack
stax merge --dry-run               # Preview merge plan only
stax merge --method squash         # squash|merge|rebase
stax merge --when-ready            # Wait for CI + approval before each merge
stax merge --interval 30           # Poll interval in seconds for --when-ready
stax merge --no-wait               # Fail fast if CI is pending
stax merge --timeout 60            # Max wait minutes per PR
stax merge --no-delete             # Keep branches after merge
stax merge --no-sync               # Skip post-merge sync
stax merge-when-ready              # Backward-compatible alias

stax rs                            # Sync trunk + clean merged branches
stax rs --restack                  # Sync then restack
stax sync --continue               # Continue after resolved sync conflicts
stax sync --safe                   # Avoid hard reset on trunk update
stax sync --force                  # Force sync without prompts
stax sync --prune                  # Prune stale remotes
stax sync --no-delete              # Keep merged branches
stax sync --auto-stash-pop         # Stash/pop dirty target worktrees

stax restack                       # Restack current branch onto parent
stax restack --all                 # Restack whole stack
stax restack --continue            # Continue after conflicts
stax restack --dry-run             # Predict conflicts only
stax restack --submit-after yes    # ask|yes|no
stax restack --auto-stash-pop      # Stash/pop dirty target worktrees

stax cascade                       # Restack bottom-up then submit
stax cascade --no-pr               # Push only, skip PR updates
stax cascade --no-submit           # Local restack only
stax cascade --auto-stash-pop      # Stash/pop dirty target worktrees
```

### Navigation and Scopes

```bash
stax co                            # Interactive branch picker
stax co <branch>                   # Checkout specific branch
stax checkout --trunk              # Jump to trunk
stax checkout --parent             # Jump to parent
stax checkout --child 1            # Jump to first child
stax t                             # Trunk alias
stax u 3                           # Move up 3 branches
stax d                             # Move down 1 branch
stax top                           # Tip of current stack
stax bottom                        # Base branch above trunk
stax p                             # Previous branch

stax branch track --parent main    # Track existing branch under parent
stax branch track --all-prs        # Import your open PRs
stax branch untrack <branch>       # Remove stax metadata only
stax branch reparent --parent new  # Change parent branch
stax branch delete <branch>        # Delete branch + metadata
stax branch squash -m "message"    # Squash all commits into one
stax branch fold --keep            # Fold into parent; optionally keep branch
stax branch up                     # Move to child (branch scope command)
stax branch down                   # Move to parent
stax branch top                    # Move to stack tip
stax branch bottom                 # Move to stack base

stax upstack restack               # Restack descendants
stax downstack get                 # Show branches below current
```

### Diagnostics, CI, Comments, and Reporting

```bash
stax ls                            # Fast stack tree
stax ll                            # Stack + PR URLs
stax log                           # Stack + commit details
stax diff                          # Diff each branch vs parent + aggregate stack diff
stax range-diff                    # Range-diff branches needing restack

stax comments                      # Show current PR comments
stax comments --plain              # Raw markdown output

stax ci                            # CI for current branch
stax ci --stack                    # CI for current stack
stax ci --all                      # CI for all tracked branches
stax ci --watch --interval 30      # Watch CI, custom poll interval
stax ci --refresh                  # Force refresh (bypass cache)
stax ci --json                     # Machine-readable output
stax ci --verbose                  # Compact summary cards

stax standup --hours 48            # Summarize recent activity window
stax standup --all --json          # All stacks in JSON

stax changelog v1.2.0 HEAD         # Changelog from ref to ref
stax changelog v1.2.0 --path src/  # Filter by path
stax changelog v1.2.0 --json       # JSON output

stax generate --pr-body            # Generate and update PR body with AI
stax generate --pr-body --edit     # Open editor before update
stax generate --pr-body --agent codex --model gpt-5
```

### Maintenance, Safety, and Setup

```bash
stax continue                      # Continue after resolving rebase conflicts
stax abort                         # Abort in-progress rebase/conflict flow

stax undo                          # Undo last risky operation
stax undo <op-id>                  # Undo a specific operation
stax undo --no-push                # Undo locally only
stax redo                          # Re-apply last undone operation
stax redo <op-id> --no-push        # Redo locally only

stax validate                      # Validate stack metadata health
stax fix --dry-run                 # Preview metadata repairs
stax fix --yes                     # Apply metadata repairs non-interactively

stax test --all --fail-fast -- make lint
stax test -- cargo test -p my-crate

stax auth --token <token>          # Save GitHub PAT
stax auth --from-gh                # Import from gh auth token
stax auth status                   # Show active auth source
stax config                        # Print config location + values
stax doctor                        # Repo/config health checks
stax demo                          # Interactive tutorial
```

## Common Workflows

### Start a New Feature Stack

```bash
stax t
stax rs
stax create api-layer
# ...changes...
stax m
stax create ui-layer
# ...changes...
stax m
stax ss
```

### Update Reviewed Branch and Re-request Review

```bash
stax co <branch>
# ...fixes...
stax m
stax ss --rerequest-review
```

### Merge with Safety Gates (CI + approvals)

```bash
stax merge --when-ready --interval 15
```

### After Base PR Merges

```bash
stax rs --restack
stax ss
```

### Resolve Rebase Conflicts

```bash
stax restack
# ...resolve conflicts...
git add -A
stax continue
```

### Repair Broken Metadata

```bash
stax validate
stax fix --dry-run
stax fix --yes
```

## Reading Stack Output

```
◉  feature/validation 1↑         # ◉ = current branch, 1↑ = commits ahead of parent
○  feature/auth 1↓ 2↑ ⟳          # ⟳ = needs restack
│ ○    ☁ feature/payments PR #42 # ☁ = has remote, PR #N = open PR
○─┘    ☁ main                    # trunk branch
```

Symbols:

- `◉` = current branch
- `○` = other branch
- `☁` = has remote tracking
- `↑` = commits ahead of parent
- `↓` = commits behind parent
- `⟳` = needs restacking (parent changed)
- `PR #N` = open PR

## Best Practices

1. Keep branches small and reviewable.
2. Sync often (`stax rs`).
3. Restack after merges (`stax rs --restack`).
4. Prefer amend flow (`stax m`) to keep one commit per branch.
5. Validate and repair metadata (`stax validate`, `stax fix`) before deep stack surgery.
6. Check stack shape (`stax ls` / `stax ll`) before submit or merge.

## Tips

- Run `stax` with no args to launch the interactive TUI.
- Use `stax --help` or `stax <command> --help` for exact flags.
- Hidden convenience shortcuts: `stax bc`, `stax bu`, `stax bd`, `stax bs`.
- Use `--yes` for non-interactive scripting.
- Use `--json` on supported commands for machine-readable output.
