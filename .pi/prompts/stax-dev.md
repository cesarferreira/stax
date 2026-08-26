---
description: "stax-dev pipeline (git-native) — worktree → plan → implement → review → verify → (optional) benchmark → commit → draft PR, with a bounded repair loop. Use for any code change to the stax Rust CLI: new commands, flags, bugfixes, refactors, behavior changes. Also for follow-ups: re-run, update, revise, continue, fix the FAILs, resume the repair loop, retry verification, or land a verified change. Direct usage/architecture questions do NOT need this pipeline."
argument-hint: "<feature or bugfix request>"
---

Run the **stax-dev** pipeline for "$@" using the `subagent` tool with `agentScope: "both"` (project agents live in `.pi/agents/`). Agents: `planner`, `codex`, `claude-reviewer`, `verifier`, `benchmarker`, `release-manager`.

Flow: **worktree → planner → codex → claude-reviewer → verifier**, then verifier **PASS → (optional) benchmarker → commit → draft PR**, verifier **FAIL → back to codex** (bounded). **Never merge to main.**

## Phase 0: Triage & context
1. Classify complexity and run only the matching depth:
   | Grade | Criteria | Execution |
   |-------|----------|-----------|
   | conversational | usage/architecture question | Answer directly. No pipeline, no worktree. |
   | small | one-file trivial change | worktree → planner (brief) → codex → verifier → commit → draft PR |
   | standard | new feature / multi-file | full pipeline below |
   | substantial | shared-core / architecture / migration | full pipeline + HITL plan approval before codex |
   If unsure, start one grade lower and escalate.
2. Pick a `run_id` (`stax-dev-01`, `-02`, …). Determine context:
   - No `_workspace/<run_id>/` → initial run. Create it + `manifest.md`.
   - Exists + "continue / fix the FAILs / retry / resume" → resume from the last incomplete stage using existing artifacts and the existing worktree.
   - Exists + new request → new `run_id`.
3. `_workspace/<run_id>/manifest.md`:
   ```
   # Run: <run_id>
   - input: "$@"
   - complexity: <grade>
   - worktree: <path>   branch: <stacked-branch>
   - stage: [ ] worktree [ ] plan [ ] impl [ ] review [ ] verify [ ] bench [ ] commit [ ] draft-pr
   - repair passes used: 0 / 2
   ```

## Phase 0.5: Isolated worktree + stacked branch  (skip for conversational)
- Create a dedicated worktree + tracked stacked branch with `stax worktree create <name>` (the branch stacks on the current branch, not on main directly unless main is the intended parent). Get its path with `stax worktree path <name>` if needed.
- All subsequent agents operate inside this worktree. Record the path + branch in `manifest.md`.
- Rationale: every task is isolated; every successful task will become one commit on its own stacked branch. Use stax for ALL git/PR work — no raw `git`/`gh` unless stax genuinely can't do the step.

## Phase 1: Plan
`subagent` single (agentScope: "both"): `planner` — "Produce an implementation plan for: $@. Load /skill:stax-plan. Write `_workspace/<run_id>/01_plan.md`. Note whether the change is performance-sensitive (hot paths: engine/stack.rs, git/repo.rs, metadata scans, branch/PR loops). Return goal + target files + step count + perf-sensitive yes/no."
- **substantial only:** show the plan to the user and get approval before Phase 2 (HITL gate).

## Phase 2–4: Implement → Review → Verify (chain, inside the worktree)
`subagent` chain (agentScope: "both"):
```
[ { agent: "codex",           task: "Implement `_workspace/<run_id>/01_plan.md` inside the run worktree. Load /skill:stax-implement. Write source, tests, docs; write `_workspace/<run_id>/02_impl.md`. Do NOT commit. Return changed files + self-verify (cargo check + make lint-fast) results." },
  { agent: "claude-reviewer", task: "Review the implementation against the plan and repo conventions. Load /skill:stax-review. Read 01_plan.md, 02_impl.md, and `git diff`. Write `_workspace/<run_id>/03_review.md`. Return PASS/FAIL + blocker/major counts. Context: {previous}" },
  { agent: "verifier",        task: "Run the scoped draft gate (build/lint/focused tests), widening to the full gate only when repo risk criteria require it. Load /skill:stax-verify. Read 01_plan.md, 02_impl.md. Write `_workspace/<run_id>/04_verify.md`. Return PASS/FAIL/BLOCKED + reason. Context: {previous}" } ]
```
> The main agent decides routing from both `03_review.md` and `04_verify.md`.

## Phase 5: Gate & bounded repair loop
Read `03_review.md` and `04_verify.md`.
- **verifier BLOCKED** (e.g. Docker down): stop, report the exact remediation, do not commit/PR.
- **verifier PASS and reviewer PASS** → Phase 5.5.
- **any FAIL** → repair pass (max **2**):
  1. `subagent` single: `codex` — "Repair ONLY the FAIL items in `03_review.md` / `04_verify.md`. No scope expansion. Do NOT commit. Update `02_impl.md` noting which FAIL each edit resolves."
  2. Re-run the review + verify chain (`claude-reviewer` → `verifier`).
  3. Increment repair count in `manifest.md`.
  - Both PASS → Phase 5.5.
  - Still FAILing after 2 passes → stop. Report unresolved FAILs, hand to the user. Do not commit/PR.
  - Genuine plan gap (codex reports the plan infeasible) → route back to `planner` (single) to revise `01_plan.md`, then restart Phase 2–4. Plan revisions don't consume the 2 repair passes but cap total at 3.

## Phase 5.5: Benchmark (optional — only if the plan marked the change perf-sensitive)
`subagent` single (agentScope: "both"): `benchmarker` — "Benchmark the change vs its baseline. Load /skill:stax-benchmark. Read 01_plan.md, 02_impl.md. Write `_workspace/<run_id>/04b_bench.md`. Return OK/REGRESSION/SKIPPED + headline delta."
- Non-perf-sensitive changes skip this entirely.
- REGRESSION does **not** block landing — it is surfaced in the PR body for the human to judge.

## Phase 6: Commit + draft PR (git-native, automatic after verifier PASS)
`subagent` single (agentScope: "both"): `release-manager` — "Land the verified change on its stacked branch. Load /skill:stax-release. Read 01–04 (+ 04b if present) artifacts. Commit on green with `stax modify -m \"<outcome>\" --all` (one clean commit, no agent attribution), then `stax submit --draft` to push + open a DRAFT PR. NEVER run `stax merge`; do NOT run `stax undraft`. Write `_workspace/<run_id>/05_release.md`. Return the draft PR URL + copy-ready handoff."
- Automatic: `stax modify` (commit) + `stax submit --draft` (push + draft PR). Human-gated: `stax undraft` (promote to ready) and `stax merge` — only on explicit user approval in "$@". **`stax merge` to main is never done by this harness.**

## Phase 7: Wrap up
- Mark `manifest.md` stages complete; preserve `_workspace/<run_id>/` and the worktree for audit.
- Present the draft PR link and handoff. Offer: "Anything to adjust in the pipeline (agent roles, gates, benchmark thresholds, test depth)?" — feed changes back into `.pi/agents/` or `.pi/skills/` and log them in AGENTS.md.

## Error handling
- A subagent failure gets one re-delegation; on second failure, report the gap and stop rather than fabricating results.
- Never weaken tests/lint or skip the verifier to reach commit. The verifier gate is mandatory; the benchmarker is not a correctness gate.
- Never merge to main under any circumstance; that is the one hard prohibition.
- chain stops at the first failing stage — inspect partial `_workspace/` artifacts and decide recovery.

## Test scenarios
- **Happy:** standard request → worktree → plan → impl → review PASS → verify PASS → (non-perf: bench skipped) → commit + draft PR. `01–05` present; `manifest.md` shows all stages done, "repair passes used: 0 / 2".
- **Repair:** verify FAIL (a failing test) → codex fixes only that test → re-review + re-verify PASS → commit + draft PR. `manifest.md` shows "repair passes used: 1 / 2".
- **Perf-sensitive:** plan marks change in `engine/stack.rs` perf-sensitive → benchmarker runs → REGRESSION → still commits + draft PR, regression flagged in PR body.
- **Scoped draft:** localized change → `cargo check` + `make lint-fast` + `git diff --check` + focused tests PASS → commit + draft PR without a redundant local full suite.
- **Blocked full gate:** a high-risk change requires `make test`, Docker is down → verifier BLOCKED → pipeline stops with "start Docker Desktop" remediation, no commit/PR.
- **Exhausted:** still FAIL after 2 repair passes → stop, report unresolved items, no commit/PR.
