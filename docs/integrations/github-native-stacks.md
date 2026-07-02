# GitHub native Stacked PRs

GitHub's native Stacked PRs feature adds a stack map, final-target branch protection, and native stack rebase/merge controls to the GitHub PR UI. stax can register its existing stacked PRs with that native feature when the repo has access.

## How it works

stax still owns local stack management: branch creation, parent metadata, restack, submit, PR bodies, and body/comment stack links. After `st submit` creates or updates the PRs, stax can run:

```bash
gh stack link <pr> [<next-pr> ...] --base <trunk> --remote <remote>
```

That registers one or more already-submitted PRs as a native GitHub Stack. stax passes PR numbers in bottom-to-top order and keeps its own body/comment stack links unless you opt out.

## Requirements

- GitHub remote.
- Repo has GitHub native Stacked PRs enabled.
- GitHub CLI `gh` is installed.
- `github/gh-stack` extension is installed:

```bash
gh extension install github/gh-stack
```

The extension must be recent enough to provide the `gh stack link` command (added after `v0.0.1`). Older versions fail with `unknown flag: --base`.

```bash
gh extension upgrade gh-stack
```

`st doctor` reports this status — including when the installed extension is too old to expose `gh stack link`. `st doctor --fix` can install the extension when `gh` is available, or upgrade it when it is outdated.

## Default behavior

The default is zero-config:

```toml
[submit]
native_stack = "auto"
stack_links_when_native = "keep"
```

With `auto`, stax attempts native registration only when the extension is installed, the repo is eligible, and the stack has **at least two PRs** (`gh stack link` requires two or more — a native stack is inherently multi-PR). Single-PR stacks are skipped silently; once a second PR joins the stack, the next submit registers both. If the repo is not enabled for the private preview, stax caches that result locally and stops retrying. Submit still succeeds and behaves like normal stax.

`stack_links_when_native = "keep"` means PR body/comment links continue to sync even when GitHub native registration succeeds.

## Manual commands

```bash
st stack link
st stack unlink
```

Use `st stack link` to register the current stack manually (requires at least two PRs).

`st stack unlink` calls `gh stack unstack`, which only operates on a stack that gh-stack tracks locally. Because stax registers stacks with `gh stack link` (which does **not** create local tracking), `st stack unlink` cannot remove a stax-registered stack directly — it reports that the current branch is not part of a tracked stack. To remove such a stack, run `gh stack checkout <pr>` to adopt it locally first, then `gh stack unstack`, or remove it from the GitHub PR UI.

## Submit overrides

```bash
st submit --native-stack     # force an attempt for this run
st submit --no-native-stack  # skip native registration for this run
```

These only affect native GitHub registration. PR creation, branch pushes, and stax-managed stack links continue to follow the normal submit options.
