---
name: stax-dev
description: >
  Development harness for the stax Rust CLI project. Use for any feature request,
  bug fix, new command, code change, or refactor in the stax codebase. Triggers on:
  "add a command", "implement X in stax", "fix bug", "change how stax does Y",
  "make stax support Z", "update stax to", "I want stax to". Also triggers for
  follow-up work: "fix the error", "try again", "re-run", "update the implementation",
  "that didn't work". Do NOT trigger for pure questions about stax usage or git concepts.
---

# stax-dev — Development Harness

**Execution mode:** Sub-agent pipeline — planner → implementer → verifier

## Phase 0: Context Check

Before dispatching any agents, check for prior plan output:

```bash
ls .claude/plans/stax-plan.md 2>/dev/null
```

- If the file exists **and** the user's request is a follow-up ("fix the error", "retry", "it failed"), skip Phase 1 and use the existing plan.
- Otherwise: start fresh. If `.claude/plans/stax-plan.md` exists from a different task, note this to the user.

## Phase 1: Plan

Dispatch the planner. Pass the user's full request verbatim plus the project root path.

```
Agent(
  subagent_type: "stax-planner",
  model: "opus",
  prompt: "
    Project root: /Users/cesarferreira/code/github/stax

    Request: <user's full request verbatim>

    Produce a complete implementation plan following your output format.
    Read patterns.md at .claude/skills/stax-dev/references/patterns.md first.
  "
)
```

Save the plan text to `.claude/plans/stax-plan.md` (create the directory if needed).
Show the plan summary to the user before proceeding to Phase 2.

## Phase 2: Implement

Dispatch the implementer with the full plan:

```
Agent(
  subagent_type: "stax-implementer",
  model: "sonnet",
  prompt: "
    Project root: /Users/cesarferreira/code/github/stax

    Execute this implementation plan exactly:
    ---
    <full plan content>
    ---

    Read patterns.md at .claude/skills/stax-dev/references/patterns.md first.
    Read each file before editing. Do not run build or tests.
    Output the CHANGED/CREATED summary at the end.
  "
)
```

## Phase 3: Verify

Dispatch the verifier:

```
Agent(
  subagent_type: "stax-verifier",
  model: "opus",
  prompt: "
    Project root: /Users/cesarferreira/code/github/stax

    Verify the changes just implemented. Run cargo check, clippy, fmt --check,
    then targeted tests with this filter: <Verification Steps from plan>.
    Report all results per your reporting format.
  "
)
```

## Phase 4: Fix Loop (if verifier reports failures)

If the verifier output contains failures:

1. Show the user which checks failed.
2. Dispatch the implementer again with the plan + verifier error report:

```
Agent(
  subagent_type: "stax-implementer",
  model: "sonnet",
  prompt: "
    Project root: /Users/cesarferreira/code/github/stax

    The previous implementation had errors. Fix them.

    Original plan:
    ---
    <plan content>
    ---

    Verifier errors to fix:
    ---
    <verifier output>
    ---

    Read each file before editing. Output the CHANGED summary.
  "
)
```

3. Re-run Phase 3.
4. Repeat up to 2 additional fix attempts (3 total). After the third failure, stop and present the remaining errors to the user for guidance.

## Output to User

After completing the pipeline:

1. **Files changed:** list from implementer's CHANGED/CREATED summary
2. **Verification:** pass/fail with any remaining issues
3. **Next step suggestion** if verification failed after max retries

## Test Scenarios

**Normal flow:** "Add a `stax ping` command that prints the current branch name"
→ Planner: identifies args.rs + mod.rs dispatch + new commands/ping.rs + commands/mod.rs
→ Implementer: creates the 4 files/edits
→ Verifier: cargo check + clippy pass, no targeted tests needed

**Fix loop:** Implementer misses a `use` import → verifier flags it → implementer adds it → verifier passes on second run.
