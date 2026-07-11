# Action-First Sync Summary Design

## Goal

Make the final `st sync` / `st rs` output answer two questions without adding
meaningful runtime: what changed, and what still needs attention?

## Default output

Keep the existing one-line completion footer and enrich it only with information
already produced by the sync flow:

```text
Sync complete! main +108 commits | 752 files +30,954 -5,422 | cleaned 2 merged | updated 1 imported | 14.022s
```

The footer omits zero-value optional segments. An up-to-date trunk retains the
existing `main up to date` wording.

## Conditional follow-up

After the footer, print a compact attention section only when sync leaves work
behind:

```text
⚠ Cleanup skipped for old-auth (dirty worktree)
→ Checked out main after cleanup
Next: st sweep
```

The possible messages are:

- cleanup attempts that were declined or could not safely delete a branch,
  including the branch and concise reason;
- trunk not reaching the fetched remote revision;
- a checkout change caused by cleaning the current branch;
- imported branches updated during sync.

Print at most one `Next:` command. Priority is trunk recovery, then inspecting
skipped cleanup with `st sweep`. Routine restack health is intentionally absent:
it is ambient repository state already available in `st ls` and the TUI, and
would otherwise turn the completion summary into a persistent nag.

## Data and performance

- Preserve the file count already emitted by `git diff --shortstat`; do not add a
  second diff.
- Accumulate cleanup, imported-update, and checkout results while their existing
  operations run.
- Do not add network calls or Git subprocesses to the default reporting path.

## Error handling

Reporting must never hide an existing detailed warning. The final summary may
repeat exceptional actionable state caused by sync in shorter form.

## Tests

- Unit tests cover shortstat parsing, footer rendering, attention rendering, and
  next-command priority.
- Integration tests exercise the binary for a trunk update and verify that a
  branch needing routine restacking does not add follow-up noise.
- Existing sync tests remain unchanged unless the richer output intentionally
  extends their assertions.

## Documentation

Document the compact footer and conditional attention lines in
`docs/commands/reference.md` and `skills.md`. The README command table does not
need a change because command semantics and flags are unchanged.
