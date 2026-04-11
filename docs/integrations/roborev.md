# Roborev Integration

[Roborev](https://www.roborev.io/) is an AI code review tool that creates one commit per review turn. This clashes with stax's single-commit-per-branch model, but the two work well together with the patterns below.

## The tension

| | roborev | stax |
|---|---|---|
| **Commits** | One per review turn (N commits) | Single commit per branch, amended |
| **History** | Incremental: "fix lint", "fix types", "address review" | Squashed: one clean commit = one PR |
| **Typical command** | `git commit` | `st modify -a -m "msg"` (amends) |

## Pattern 1: Auto-squash on submit (recommended)

Use `--squash` when submitting to squash all commits on each branch down to one before pushing:

```bash
# roborev makes multiple commits during review...
st submit --squash          # squash + push + update PRs
st ss --squash              # short form
```

This keeps roborev's incremental history locally (useful for `git log` during the review cycle) while producing a clean single-commit PR.

## Pattern 2: Manual squash after review

After roborev finishes its review cycle, squash explicitly:

```bash
st branch squash -m "feat: the actual feature"
st ss                       # submit
```

## Pattern 3: Configure roborev to amend

If roborev supports custom commit commands, configure it to use `st modify` instead of `git commit`:

```bash
st modify -a -m "address review: <finding>"
```

This amends into the existing commit directly, keeping the stax model intact. No squash needed.

## Pattern 4: Multi-branch review with `st absorb`

When roborev fixes findings that span files across different branches in a stack:

```bash
# roborev committed fixes across multiple files
git add -A
st absorb                   # routes each file to the correct branch
st ss                       # submit all
```

## Tips

- Roborev commits are preserved locally until you squash or submit with `--squash`, so you can always review what changed per turn with `git log`.
- `st undo` can recover from an accidental squash.
- Combine patterns: use `st absorb` to distribute changes, then `st submit --squash` to clean up history.
