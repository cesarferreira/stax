# libghostty agent terminal memory

[libghostty](https://mitchellh.com/writing/libghostty-is-coming) is Ghostty's embeddable terminal-emulation library. The first planned component, `libghostty-vt`, focuses on parsing terminal sequences and maintaining terminal state without forcing applications to build their own ad-hoc ANSI parser.

For stax, the useful framing is not "embed a terminal because it is cool." The useful product primitive is:

> Every stacked branch and AI lane can have durable terminal memory.

`st lane` already gives each agent a real branch, worktree, and usually a tmux session. A libghostty-backed layer could make the terminal side just as first-class: inspectable, replayable, status-aware, and tied to the branch/PR lifecycle.

## Why this fits stax

AI lanes are not just shell sessions. They are units of stacked Git work:

- a branch and parent branch
- a worktree path
- an optional PR
- CI status
- dirty/conflict/rebase state
- an attached or detached agent process
- the terminal output that explains what happened

Today, terminal output is mostly owned by tmux or the user's terminal emulator. With a terminal-state parser, stax could keep a normalized view of an agent lane without depending on brittle regex parsing of ANSI escape sequences.

## Candidate features

### `st lane watch`

A live cockpit for active lanes:

```text
Lane             Branch             State                 Last terminal line
fix-login        fix-login          running tests         test auth_refresh ... ok
refactor-cache   refactor-cache     waiting for input     Approve command? [y/N]
docs-api         docs-api           done                  PR body generated
```

Selecting a lane could show the last rendered terminal screen, not just raw log text.

### `st lane replay <name>`

Replay the terminal history for a lane to answer:

- What commands did the agent run?
- Where did it hit a conflict?
- Did tests actually pass before it opened the PR?
- Why is the lane waiting?

This pairs naturally with stax's existing undo-first model: Git state explains *what changed*; terminal replay explains *how it happened*.

### Rich CI and build logs

`st ci`, `st pr`, and the TUI could render CI/build logs with proper terminal semantics:

- colors and styles
- carriage returns and progress bars
- line clearing/redrawing
- Unicode width handling
- final screen snapshots for failed jobs

That gives stax a better local log viewer without shipping a browser UI.

### Lane status detection

A normalized terminal screen makes it easier to classify agent state across Claude Code, Codex, Gemini CLI, OpenCode, and future tools:

- waiting for approval
- asking a question
- running tests
- blocked on merge conflicts
- generated a PR summary
- completed with a clean tree

The detection should remain heuristic and agent-agnostic. libghostty would provide correct terminal state; stax would still own the workflow semantics.

### Shareable lane artifacts

A future `st lane publish <name>` could produce a compact artifact for reviewers:

- branch and PR links
- diff summary
- test/CI result
- final terminal screen
- replay or transcript excerpt
- agent summary

This would make AI-generated branches easier to audit before review.

## Architecture sketch

The first version does not need to replace tmux or implement a full terminal emulator.

1. Keep launching lanes exactly as today: worktree + branch + configured agent + tmux when available.
2. Tee PTY/tmux output into a lane-local terminal log.
3. Feed output through a terminal-state parser such as `libghostty-vt`.
4. Persist compact snapshots under stax metadata for each lane.
5. Render those snapshots in the worktree dashboard, TUI, or future `st lane watch` command.

A lane record could eventually track:

```text
lane name
branch
worktree path
tmux session/window
last terminal screen snapshot
last N transcript chunks
agent status classification
last command / prompt marker, when detectable
updated_at
```

## Non-goals

- Do not turn stax into tmux.
- Do not require Ghostty the app.
- Do not make AI lanes depend on one specific agent CLI.
- Do not make terminal replay a blocker for normal stacked-branch workflows.
- Do not persist sensitive terminal history unless the user explicitly opts in or configures retention.

## Open questions

- Should terminal capture be opt-in globally, opt-in per lane, or enabled only for `st lane watch`?
- Where should transcript/snapshot data live relative to existing stax metadata?
- What retention policy avoids leaking secrets while keeping debugging value?
- Can the first implementation use tmux capture-pane as an interim source before PTY teeing exists?
- Should replay be raw transcript playback, screen snapshots over time, or both?
- How should status classifiers be configured for different agents?

## Suggested MVP

Start with an experimental command:

```bash
st lane watch
```

MVP behavior:

1. list active stax-managed lanes
2. show branch, worktree, tmux/session state, and Git status
3. show the last captured terminal screen for the selected lane
4. classify obvious states such as `running`, `waiting`, `conflict`, `tests failed`, and `done`
5. keep all terminal-memory storage local and easy to delete

This keeps the scope small while proving the differentiated value: stax becomes the place where branch state, PR state, and agent terminal state meet.
