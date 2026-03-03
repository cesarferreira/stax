# Health & Testing Commands

Stax provides commands to validate, repair, and test your stack metadata.

## `st validate`

Check that all branch metadata is consistent and healthy.

```bash
st validate
```

Runs these checks:
- **Orphaned metadata** - metadata refs exist for branches that have been deleted
- **Missing parents** - metadata points to a parent branch that no longer exists
- **Cycle detection** - detects loops in the parent chain
- **Invalid metadata** - unparseable JSON in metadata refs
- **Stale parent revision** - parent has moved since last restack

Exit code `0` if healthy, `1` if issues found.

## `st fix`

Auto-repair broken metadata.

```bash
st fix           # Interactive repair
st fix --yes     # Auto-approve all fixes
st fix --dry-run # Preview without changing anything
```

Repairs:
- Deletes orphaned metadata (metadata for deleted branches)
- Reparents orphaned branches to trunk (when parent doesn't exist)
- Deletes invalid metadata (unparseable JSON)
- Reports branches that need restack

Wrapped in a transaction for undo support (`st undo`).

## `st test <cmd>`

Run a shell command on each branch in the stack.

```bash
st test "cargo test"          # Run tests on each branch
st test "make lint"           # Run linting on each branch
st test --fail-fast "cargo check"  # Stop on first failure
st test --all "true"          # Run on all tracked branches
```

| Flag | Description |
|------|-------------|
| `--fail-fast` | Stop after the first branch that fails |
| `--all` | Run on all tracked branches, not just the current stack |

The command checks out each branch (bottom to top, excluding trunk), runs the command, and reports pass/fail. Returns to the original branch when done. Exit code `1` if any branch fails.

Example output:
```
Running 'cargo test' on 3 branch(es)...

  feature-a ... PASS
  feature-b ... FAIL
  feature-c ... PASS

2 passed, 1 failed
Failed branches:
  feature-b
```
