---
name: stax-dev
description: >
  Use when implementing a feature, bug fix, new command, refactor, or behavior
  change in the stax CLI codebase, including follow-up repair requests. Do not
  use for direct stax usage or architecture questions that require no code change.
---

# stax-dev

Run a bounded, evidence-carrying pipeline for one scoped stax change:

`context/worktree → plan → implement → review + verify → repair (maximum 2) → optional benchmark → release`

The harness may create a task worktree during triage. Only `stax-release-manager` may commit, push, or submit after the gates. Nothing in this workflow may merge or undraft a PR.

## Run Contract

Resolve the project root dynamically with `git rev-parse --show-toplevel`; never assume a personal path. Create a unique run directory at `<project-root>/_workspace/<run_id>/`. These files are transient evidence and must not be committed:

```text
context.md
plan.md
implementation-pass-0.md
review-pass-0.md
verification-pass-0.md
implementation-pass-1.md   # only after first repair
review-pass-1.md
verification-pass-1.md
implementation-pass-2.md   # only after second repair
review-pass-2.md
verification-pass-2.md
benchmark.md
release.md
```

Use pass `0` for initial implementation and passes `1` and `2` for the only permitted repair attempts. Every agent receives the project root, run directory, user request, approved plan, current pass, and paths of all prior artifacts it needs. Persist agent output to the named artifact when the agent cannot write it directly.

## 0. Context and Worktree Triage

Read `AGENTS.md` and inspect `git status`, the current branch/worktree, `stax worktree ll`, and `stax ll`. Record the request, project root, current branch, parent, worktree decision, pre-existing changes, and applicable instructions in `context.md`.

- Continue in the current checkout only when it is already a dedicated task worktree on the intended stacked branch.
- If the checkout is shared, trunk, or belongs to unrelated work, create or select a dedicated stax task worktree without moving, deleting, stashing, or overwriting user changes. Re-resolve the project root inside it.
- Stop for user direction if branch ownership, parentage, or unrelated dirty changes cannot be resolved safely.

## 1. Plan

Dispatch `stax-planner` with the request and `context.md`. It must read `.claude/skills/stax-dev/references/patterns.md` and return its required plan format. Save it as `plan.md`.

Check that the plan has acceptance criteria, non-goals, source/tests/docs impact, RED evidence, happy/error/edge tests, exact focused nextest filters, risk/full-gate classification, and performance classification. If material information is missing, allow one planner revision only (two planner passes total). Stop rather than inventing a third plan.

## 2. Implement

Dispatch `stax-implementer` for pass `0` with `plan.md`. The implementer owns source, tests, and documentation edits but no git state. Require its CHANGED/CREATED, command, and unresolved report in `implementation-pass-0.md`.

## 3. Independent Gates

Dispatch `stax-reviewer` and `stax-verifier` independently for the same pass. Neither may edit files or git state.

- Reviewer writes `review-pass-N.md` and checks plan conformance, correctness, conventions, tests, docs, and unintended scope.
- Verifier writes `verification-pass-N.md`, labels evidence as `scoped draft gate` or `full local gate`, and runs the commands required by `AGENTS.md` and the plan.

Release requires both artifacts to say `VERDICT: PASS`. `BLOCKED` is not a pass.

## 4. Bounded Repair

If either gate fails, dispatch `stax-implementer` in repair-only mode with the current diff, plan, and both gate reports. Write the result as `implementation-pass-1.md`, then rerun both independent gates into their pass-1 artifacts. Repeat once more as pass `2` if needed.

Maximum: two repair passes after initial implementation. Never reset the counter, weaken tests, revise acceptance criteria, or omit a previously failing gate. After pass `2`, stop with the remaining findings. If verification is `BLOCKED`, stop and report the infrastructure blocker rather than spending a repair pass.

## 5. Optional Benchmark

After reviewer and verifier both pass, dispatch `stax-benchmarker`. It writes `benchmark.md` with `PASS`, `SKIPPED`, `FAIL`, or `BLOCKED`. A non-performance or delegation/deletion-only diff should be explicitly `SKIPPED`; a performance-sensitive change needs comparable parent-baseline and changed-worktree evidence. Continue only on `PASS` or `SKIPPED`.

## 6. Release

Dispatch `stax-release-manager` with all final artifacts. It is the sole owner of staging/commit, push, and PR submission. It must exclude `_workspace/`, validate the branch and parent, commit using stax, and submit only the current branch:

- New PR or existing draft: `stax branch submit --draft --ai --yes`.
- Existing ready PR: `stax branch submit --ai --yes` to preserve its ready-for-review state.

Remember: plain `stax submit` submits the full stack and is forbidden here. `--yes` accepts generated details; it does not waive verification. The release manager validates the URL, body, base, and draft/ready state and records them in `release.md`. Never merge, undraft, mark ready, or reparent automatically.

## Final Report

Return the changed/created files, final review and verification verdicts with evidence tier, benchmark verdict, commit, PR URL/base/state, and any unresolved blocker. Do not claim success unless `release.md` says `VERDICT: PASS`.
