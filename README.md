<div align="center">
  <h1>stax</h1>
  <p><strong>Keep working while your first PR is in review.</strong></p>
  <p>
    <a href="https://github.com/cesarferreira/stax/actions/workflows/rust-tests.yml"><img alt="Tests" src="https://img.shields.io/github/actions/workflow/status/cesarferreira/stax/rust-tests.yml?branch=main&amp;logo=github&amp;label=tests"></a>
    <a href="https://crates.io/crates/stax"><img alt="Crates.io" src="https://img.shields.io/crates/v/stax"></a>
    <a href="https://crates.io/crates/stax"><img alt="Crates.io downloads" src="https://img.shields.io/crates/d/stax"></a>
    <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-green"></a>
  </p>
  <img src="assets/screenshot.png" width="100%" alt="stax in action">
  <p>
    <a href="#install">Install</a> ·
    <a href="#quickstart">Quickstart</a> ·
    <a href="#commands">Commands</a> ·
    <a href="https://cesarferreira.github.io/stax/">Docs</a>
  </p>
</div>

Build the UI on top of the API branch instead of waiting for the API to merge. Each gets its own PR, and reviewers see only what that branch adds. stax tracks which Git branches build on each other, opens PRs against the right base, and rebases the remaining branches when the base changes.

## Install

```bash
brew install cesarferreira/tap/stax
```

Or use `cargo binstall stax`. See the [installation guide](docs/getting-started/install.md) for prebuilt binaries, Windows, and source builds. Both `stax` and its short alias `st` are installed.

## Quickstart

Run `st setup` once to configure GitHub auth and shell integration. Then, in a GitHub repo you can push to, start on a clean, up-to-date `main`. Create an API branch, then stack the UI and tests on top of it as siblings:

```bash
st create auth-api
echo "Token refresh API" > auth-api-demo.md
git add auth-api-demo.md
git commit -m "Document the token refresh API"

st create auth-ui
echo "Token refresh UI" > auth-ui-demo.md
git add auth-ui-demo.md
git commit -m "Document the token refresh UI"

st create auth-tests --from auth-api
echo "Token refresh test cases" > auth-tests-demo.md
git add auth-tests-demo.md
git commit -m "Outline the token refresh tests"

st ls
```

```text
◉       auth-tests 1↑
│ ○     auth-ui 1↑
○─┘     auth-api 1↑
○    ☁️ main
```

Read it from the bottom up: `auth-api` builds on `main`, then the stack splits. Both `auth-ui` and `auth-tests` build on `auth-api`, not on each other. `◉` marks the checked-out branch; `1↑` means one commit ahead of the parent.

To open PRs for both sides of the split, submit from their shared base:

```bash
git switch auth-api
st upstack submit
```

This opens `auth-api` against `main` and both `auth-ui` and `auth-tests` against `auth-api`. The UI and tests can be reviewed independently.

Once `auth-api` merges, run `st refresh` from each remaining branch to rebase it and update its PR.

[Setup and auth help](docs/getting-started/quick-start.md) · [Merge and cascade guide](docs/workflows/merge-and-cascade.md)

## Commands

| I want to… | Run |
|---|---|
| Create a branch on top of the current one | `st create <name>` |
| See my stacks and PR status | `st ls` |
| Switch branches (pick interactively or pass a name) | `st co` / `st co <branch>` |
| Open or update PRs for the current stack | `st ss` |
| Open or update PRs for the current branch and all its descendants | `st upstack submit` |
| Preview a submit without making changes | `st ss --plan` |
| Sync trunk, rebase, and update the stack's PRs | `st refresh` |
| See which CI checks are failing | `st ci` |
| Merge from the bottom up to my current PR | `st merge` |
| Recover local branch state after a restack or sync | `st undo` |

[Core commands](docs/commands/core.md) · [Full command reference](docs/commands/reference.md)

## Beyond the basics

Run `st` to browse branches, diffs, and CI in the [terminal UI](docs/interface/tui.md), or run `st web` for the [local browser workspace](docs/interface/web.md).

![stax terminal interface showing the branch stack, summary, and diff](assets/tui.png)

- **Native GitHub stacks.** `st ss` registers a native GitHub stack when the repo has the feature enabled and the `gh-stack` extension is installed. Otherwise you still get linked PRs. [GitHub setup →](docs/integrations/github-native-stacks.md)
- **Parallel work.** Use `st wt` to manage Git worktrees, or `st lane` to run each AI coding session on its own branch and worktree. AI is optional; it can also [draft PR text](docs/integrations/pr-templates-and-ai.md) or [help resolve a rebase conflict](docs/commands/reference.md#st-resolve). [Agent lanes →](docs/workflows/agent-worktrees.md)
- **Recover from a bad restack.** `restack`, `submit`, `sync`, and `reorder` snapshot local branch state. `st undo` restores that state; it doesn't roll back GitHub changes. [Undo and redo →](docs/safety/undo-redo.md)

## More resources

[Configuration](docs/configuration/index.md) · [tmux plugin](docs/integrations/tmux.md) · [AI agent instructions](skills.md) · [Freephite and Graphite compatibility](docs/compatibility/freephite-graphite.md) · [Benchmarks](docs/reference/benchmarks.md)

Contributing? See [repository guidelines](AGENTS.md) and the [release workflow](docs/workflows/releasing.md).

[MIT License](LICENSE)
