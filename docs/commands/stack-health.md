# Stack health

Commands to validate, repair, and test your stack metadata.

## `st validate`

Check that all branch metadata is consistent.

```bash
st validate
```

Runs:

- **Orphaned metadata** — metadata refs for deleted branches
- **Missing parents** — metadata points to a parent that no longer exists
- **Cycle detection** — loops in the parent chain
- **Invalid metadata** — unparseable JSON
- **Stale parent revision** — parent has moved since last restack

Exit code `0` if healthy, `1` if issues found.

## `st fix`

Auto-repair broken metadata. Wrapped in a transaction so `st undo` works.

```bash
st fix            # interactive repair
st fix --yes      # auto-approve all fixes
st fix --dry-run  # preview without changing anything
```

Repairs:

- Deletes orphaned metadata
- Reparents orphaned branches to trunk when the parent is gone
- Deletes invalid metadata
- Reports branches that need restack

## `st run <cmd>` (alias: `st test <cmd>`)

Run a shell command on each branch in the stack, bottom to top (excluding trunk), returning to the starting branch afterward.

```bash
st run "cargo test"                    # current stack
st run --stack "make test"             # explicit current stack
st run --stack=feature-a "make test"   # a specific stack
st run --all "true"                    # all tracked branches
st run --fail-fast "cargo check"       # stop on first failure
```

| Flag | Behavior |
|---|---|
| `--fail-fast` | Stop after the first failing branch |
| `--all` | Run on all tracked branches |
| `--stack[=<branch>]` | Run one stack (current by default) |

Example output:

```text
Running 'cargo test' on 3 branch(es)...

  feature-a:   SUCCESS
  feature-b:   FAIL
  feature-c:   SUCCESS

2 succeeded, 1 failed
Failed branches:
  feature-b
```

Exit code `1` if any branch fails.
