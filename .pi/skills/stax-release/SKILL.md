---
name: stax-release
description: Land a verified stax change git-natively — commit on its stacked branch, push, and open a DRAFT PR via stax — then hand off. Use as the final pipeline stage after the verifier PASSES. Commit/push/draft-PR are automatic; never merges to main and never promotes a draft to ready without explicit approval.
---

# stax-release — Landing a Verified stax Change (git-native)

You are the single git owner of the run. Commit the verified change on its stacked branch and open a draft PR. You never merge to main.

## Hard gates
- Run ONLY when the verifier verdict is **PASS**. FAIL/BLOCKED → stop, report, point back to the repair loop.
- **Never merge to main**; never promote a draft PR to ready-for-review without explicit approval. Those are the only human-gated steps.
- Never add agent attribution to commits, PR titles, or bodies.

## Automatic steps (no approval — reversible/low-risk, and the whole point of the harness)
1. **Commit on green** — one clean commit on the run's tracked stacked branch: `stax modify -m "<user-visible outcome>" --all` (creates the first branch-local commit on a fresh tracked branch). No agent attribution.
2. **Changelog / docs** — if repo convention requires (Keep-a-Changelog style if present) and not already done, add the entry before committing so it's in the same commit.
3. **Push + Draft PR** — `stax submit --draft` pushes the branch and opens a DRAFT PR based on its stack parent. Never a non-draft submit; never `--publish`.

## Human-gated (never without explicit approval)
- `stax undraft` — promote draft → ready-for-review.
- `stax merge` — merge PRs. **Never merge to main via this harness.**

## git/forge policy
Use stax for ALL branch/stack/commit/PR ops (`stax worktree create`, `stax modify`, `stax submit`, `stax status`/`stax ll`, `stax undraft`, `stax merge`). Inspect the stack first, preserve parent relationships, never raw-rebase / delete branches / change PR bases on stax-managed branches. Fall back to `git` + `gh` only if stax genuinely cannot do a step — and still never merge or promote without approval.

## Output
Write `_workspace/<run_id>/05_release.md`: commit SHA, branch, draft PR URL, PR body (motivation, material changes, verification from `04_verify.md`, benchmark note from `04b_bench.md` if present, residual risk from `03_review.md`). Return a copy-ready handoff (outcome first, then material changes, verification, draft PR link) + explicit note of what is done (commit/push/draft PR) vs. awaiting approval (ready-for-review, merge).

## Principles
- Benchmark REGRESSION present → still commit + draft-PR, but call it out prominently so the human decides before promoting.
- If asked to carry CI to completion, monitor until green or genuinely blocked.
- Preserve the user's in-progress work; don't discard or overwrite unrelated changes.
