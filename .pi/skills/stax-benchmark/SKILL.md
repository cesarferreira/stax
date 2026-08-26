---
name: stax-benchmark
description: Measure whether a stax change regresses performance — release-build command latency via hyperfine, or cargo bench where present — against a baseline of the pre-change revision. Use only for perf-sensitive changes (stack tree build, git rebase/merge/worktree ops, metadata scanning, branch/PR loops); skip pure correctness/docs changes.
---

# stax-benchmark — Performance Regression Checks

Speed claims need numbers from a controlled comparison. This stage is optional — run it only when a change can plausibly move a hot path.

## Decide relevance first
Perf-sensitive: `src/engine/stack.rs` (tree build), `src/git/repo.rs` (rebase/merge/worktree), metadata ref scanning, loops over branches/PRs. If the change is docs, a flag with no hot-path effect, or a localized correctness fix → **SKIPPED**, say so, return.

## Measure correctly
1. `cargo build --release` — never benchmark debug builds.
2. **Baseline vs change under identical conditions:** measure the parent revision (clean worktree at the parent commit, or `git stash`) and the changed revision on the same machine, same warm-up, ≥10 runs.
3. Tooling:
   - `cargo bench` if the repo has benches.
   - else `hyperfine` on representative `stax` commands in a realistic temp repo (e.g. a stack of N branches): `hyperfine --warmup 3 './target/release/stax status'`.
4. Report absolute deltas + percentages + variance. Results inside noise are **OK**, not regressions.

## Output
Write `_workspace/<run_id>/04b_bench.md`:
```
# Benchmark — verdict: OK | REGRESSION | SKIPPED
## Scenario & commands
## Results (baseline vs change, Δ abs / Δ %, variance)
## Assessment (within noise? real regression? magnitude)
```
Return **OK / REGRESSION / SKIPPED** + the headline delta.

## Principles
- No number, no claim. Paste commands and the timing table.
- macOS timings are noisy (endpoint-security tooling) — if you can't get a stable baseline, report inconclusive rather than a shaky OK.
- You don't gate correctness (the verifier does). You surface regressions; the user decides acceptability.
