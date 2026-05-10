# libghostty agent terminal memory

[libghostty](https://mitchellh.com/writing/libghostty-is-coming) is Ghostty's embeddable terminal-emulation library. The first planned component, `libghostty-vt`, focuses on parsing terminal sequences and maintaining terminal state without forcing applications to build their own ad-hoc ANSI parser.

For stax, the useful framing is not "embed a terminal because it is cool." The useful product primitive is:

> Every stacked branch and AI lane can have durable terminal memory.

`st lane` already gives each agent a real branch, worktree, and usually a tmux session. `st lane watch` is the first concrete step: it shows each managed lane's branch, tmux state, classified status, and last captured tmux terminal line. A future libghostty-backed layer could make the terminal side even more first-class: inspectable, replayable, status-aware, and tied to the branch/PR lifecycle.

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

Implemented first step: a non-interactive lane cockpit for active lanes:

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

The first version keeps the current tmux launch model and adds a lightweight cockpit.

1. Keep launching lanes exactly as today: worktree + branch + configured agent + tmux when available.
2. Discover tmux sessions matching managed lane names.
3. Capture each lane's current tmux pane with `tmux capture-pane`.
4. Classify obvious states from Git/worktree state plus the last terminal line.
5. Print the result via `st lane watch`.

A later libghostty-backed layer can replace the raw tmux snapshot with a normalized terminal-state parser:

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

## Suggested next step

The command now ships an MVP:

```bash
st lane watch
```

Current behavior:

1. lists stax-managed lanes
2. shows branch, tmux/session state, and Git/worktree status
3. shows the last captured tmux terminal line
4. classifies obvious states such as `running`, `waiting`, `conflict`, `failed`, and `done`
5. keeps terminal capture ephemeral by reading tmux on demand

Next, wire a real terminal parser such as `libghostty-vt` between captured output and state classification so stax can track rendered terminal screens instead of only the latest raw line.
