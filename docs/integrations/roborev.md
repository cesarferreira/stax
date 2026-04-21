# Roborev

[Roborev](https://www.roborev.io/) is an AI code review tool that creates one commit per review turn. This clashes with stax's single-commit-per-branch model, but the two work well together with the patterns below.

## The tension

| | Roborev | stax |
|---|---|---|
| **Commits** | One per review turn (N commits) | Single commit per branch, amended |
| **History** | Incremental: "fix lint", "fix types", "address review" | Squashed: one clean commit = one PR |
| **Typical command** | `git commit` | `st modify -a -m "msg"` (amends) |

## Pattern 1: auto-squash on submit (recommended)

```bash
# roborev makes multiple commits during review…
st submit --squash   # squash + push + update PRs
st ss --squash       # short form
```

Keeps roborev's incremental history locally (useful for `git log` during review) while producing a clean single-commit PR.

## Pattern 2: manual squash after review

```bash
st branch squash -m "feat: the actual feature"
st ss
```

## Pattern 3: configure roborev to amend

If roborev supports a custom commit command, point it at `st modify`:

```bash
st modify -a -m "address review: <finding>"
```

Amends directly into the existing commit — no squash needed.

## Pattern 4: multi-branch review with `st absorb`

When roborev fixes findings across different branches in a stack:

```bash
git add -A
st absorb   # routes each file to the correct branch
st ss       # submit all
```

## Tips

- Roborev commits are preserved locally until you squash or submit with `--squash` — you can always review per-turn changes with `git log`.
- `st undo` can recover from an accidental squash.
- Combine patterns: `st absorb` to distribute, then `st submit --squash` to clean up.
