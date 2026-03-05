# Multi-Worktree Support

stax is worktree-aware. If a branch in your stack is checked out in another worktree, stax runs operations in the right worktree automatically.

## Worktree-safe operations

- `st restack` and `st sync --restack` run `git rebase` in the target worktree when needed.
- `st cascade` fast-forwards trunk before restacking, even if trunk is checked out elsewhere.
- `st sync` updates trunk in whichever worktree currently has trunk checked out.
- Metadata (`refs/branch-metadata/*`) is shared across all worktrees automatically.

## Dirty worktrees

By default, stax fails fast when target worktrees contain uncommitted changes.

Use `--auto-stash-pop` to stash before rebase and restore afterward:

```bash
st restack --auto-stash-pop
st upstack restack --auto-stash-pop
st sync --restack --auto-stash-pop
```

If conflicts occur, stax preserves the stash entry so changes are not lost.

---

## `stax worktree` — developer worktree management

`st worktree` (alias `st wt`) lets you create and navigate Git worktrees for parallel branch development. Unlike `st agent` (which is oriented around AI task isolation with title-based branch naming), `st worktree` works with your existing branches and is optimised for human developers switching between stacks.

### Quick start

```bash
# Create a worktree for an existing branch
st worktree create feature/payments-api

# List all worktrees (including those created outside stax)
st worktree list

# Jump to a worktree (requires shell integration — see below)
st worktree go payments-api

# Remove when done
st worktree remove payments-api
```

### Shell integration (one-time setup)

`st worktree go` needs to change your shell's working directory. Because a child process cannot `cd` its parent shell, a shell function wrapper is required.

```bash
# Print the snippet (add to ~/.zshrc manually)
st shell-setup

# Or install automatically
st shell-setup --install
```

`--install` auto-detects your shell (`$SHELL`), checks for idempotency, and appends:

```bash
eval "$(stax shell-setup)"
```

to `~/.zshrc`, `~/.bashrc`, or `~/.config/fish/config.fish`. Restart your shell or `source ~/.zshrc` once to activate.

After installing, the `stax` shell function and `sw` alias are available:

```bash
sw payments-api   # quick switch — equivalent to: st worktree go payments-api
```

### Full example

```bash
# Working on stack A
~/project $ st status
main
 └── feature/auth-api  ← (you are here)
      └── feature/auth-ui

# Need to work on stack B without losing context
~/project $ st worktree create feature/payments-api
  Created  worktree 'payments-api' → branch 'feature/payments-api'
  Path:   /home/you/project/.worktrees/payments-api

~/project $ st worktree list
  NAME           BRANCH                   PATH
  ─────────────────────────────────────────────────────────────────────────
*  main           feature/auth-api         ~/project
   payments-api   feature/payments-api     ~/project/.worktrees/payments-api

~/project $ st worktree go payments-api
~/project/.worktrees/payments-api $

# All stax commands work normally inside worktrees
~/project/.worktrees/payments-api $ st restack --all

# Clean up
~/project $ st worktree remove payments-api
  Removed  worktree 'payments-api' (branch 'feature/payments-api')
```

### Worktree directory layout

All managed worktrees live under `.worktrees/` in your repo root (automatically added to `.gitignore`):

```
<repo-root>/
  .worktrees/
    payments-api/       ← st worktree create feature/payments-api
    auth-v2/            ← st worktree create feature/auth-v2
```

Worktrees created outside stax (via raw `git worktree add`) still appear in `st worktree list` and can be navigated with `st worktree go`.

### Commands

| Command | Alias | Description |
|---------|-------|-------------|
| `st worktree create [branch]` | `st wt c` | Create a worktree for an existing or new branch |
| `st worktree list` | `st wt ls`, `st w` | List all worktrees |
| `st worktree go <name>` | `st wtgo <name>` | Navigate to a worktree (shell integration required) |
| `st worktree path <name>` | | Print absolute path (for scripting) |
| `st worktree remove <name>` | `st wt rm`, `st wtrm <name>` | Remove a worktree |
| `st shell-setup` | | Print shell integration snippet |
| `st shell-setup --install` | | Install shell integration automatically |

**Hidden top-level shortcuts:**

| Shortcut | Expands to |
|----------|-----------|
| `st w` | `st worktree list` |
| `st wtc [branch]` | `st worktree create [branch]` |
| `st wtls` | `st worktree list` |
| `st wtgo <name>` | `st worktree path <name>` (shell wrapper does the `cd`) |
| `st wtrm <name>` | `st worktree remove <name>` |
| `sw <name>` | `st worktree go <name>` (shell alias from `st shell-setup`) |

### Naming

The worktree short name is derived from the branch by taking the last `/`-delimited segment:

- `feature/payments-api` → `payments-api`
- `bugfix/auth-api` → `auth-api` (collision with `feature/auth-api`? stax prefixes: `bugfix-auth-api`)

Override with `--name`:

```bash
st worktree create feature/payments-api --name pay
```

### Non-existent branches

If you pass a branch name that doesn't exist yet, stax offers to create it stacked on your current branch:

```bash
st worktree create feature/new-thing
# Branch 'feature/new-thing' does not exist. Create it stacked on current branch? [Y/n]
```

---

## Agent worktrees

For running multiple AI agents (Cursor, Codex, Aider) in parallel, `st agent` automates the full worktree lifecycle with title-based branch naming, editor integration, and a registry.

See [Agent Worktrees](agent-worktrees.md) for details.

| | `st worktree` | `st agent` |
|---|---|---|
| Input | Existing or new branch name | Human title (slugified to branch) |
| Target user | Developer switching stacks | AI agent orchestration |
| Storage | Raw `git worktree list` | Registry at `.git/stax/agent-worktrees.json` |
| Editor opening | — | ✓ (`--open-cursor`, `--open-codex`) |
| Shell `cd` integration | ✓ (`st shell-setup`) | — |
| Post-create hook | — | ✓ (`agent.post_create_hook`) |
