---
name: stax-review
description: Evidence-based code review of a stax diff against its plan and repo conventions, producing severity-ranked PASS/FAIL findings with file:line references. Use to review Codex's implementation before mechanical verification, or to re-review after a repair pass. Read-only on source.
---

# stax-review — Reviewing stax Changes

Your value is judgment the compiler can't provide. Every finding is evidence-backed and cites `file:line`.

## What to check
1. **Plan conformance** — does the diff implement `01_plan.md`? Missing steps or undeclared scope creep are findings.
2. **Correctness / root cause** — logic errors, wrong error handling, borrow/ownership traps, async `block_on` misuse, metadata & ref invariants (`refs/branch-metadata/*`, `refs/stax/trunk`), transaction/`OpKind` completeness, `cascade.rs`↔`restack.rs` drift.
3. **Convention-fit** — stax command scaffolding, minimal-change discipline, no gratuitous comments, no agent attribution, no backwards-compat cruft.
4. **Tests** — do they genuinely cover happy/error/edge? Are new `tests/*_tests.rs` files registered in `tests/all_tests.rs`?
5. **Docs** — were `README.md` / `docs/` / `skills.md` updated when behavior changed (Documentation Policy)?

## How to review
- Read `01_plan.md`, `02_impl.md`, and the working-tree diff (`git diff`). Use CodeGraph / `rg` for context.
- Severity-rank every finding: **blocker** > **major** > **minor/nit**. Only blockers and majors gate a PASS.
- Facts vs. inference — cite the concrete problem and a fix or repro. Don't invent findings; a clean diff earns a PASS with noted residual risk.

## Output
Write `_workspace/<run_id>/03_review.md`:
```
# Review — verdict: PASS | FAIL
## Blockers        (file:line — problem — required fix)
## Majors
## Minors / nits
## Residual risk / unverified areas
```
Return the verdict + blocker/major counts. **FAIL** if any blocker or major remains.

## Edge rules
- Material divergence from the plan is a blocker — unless it's a clear improvement that still meets acceptance criteria, then accept and note it.
- If you can't assess an area, say so under residual risk rather than guessing PASS.
