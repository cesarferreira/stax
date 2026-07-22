# GitHub native Stacked PRs

GitHub's native Stacked PRs feature adds a stack map, final-target branch protection, and native stack rebase/merge controls to the GitHub PR UI. stax can register its existing stacked PRs with that native feature when the repo has access.

## Install

This feature needs the [`github/gh-stack`](https://github.com/github/gh-stack) GitHub CLI extension. Install it once:

```bash
gh extension install github/gh-stack
# or let stax install it for you:
st doctor --fix
```

Upgrade it the same way:

```bash
gh extension upgrade stack
# or:
st doctor --fix
```

No further setup is required — once the extension is installed, native stack registration is fully automatic (see [Default behavior](#default-behavior) below).

## Requirements

- GitHub remote.
- Repo has GitHub native Stacked PRs enabled.
- GitHub CLI `gh` is installed and authenticated through any normal supported source (`gh auth login`, `GH_TOKEN`, or `GITHUB_TOKEN`).
- `github/gh-stack` extension is installed (see [Install](#install) above), and recent enough to provide the `gh stack link` command (added after `v0.0.1`). Older versions fail with `unknown flag: --base`.

`st doctor` reports this status — including the installed version, whether it is current, when the extension is too old to expose `gh stack link`, or when it is missing entirely. `st doctor --fix` can install the extension when `gh` is available, or upgrade it when it is outdated.

**Recommended: v0.0.8+.** v0.0.8 migrated to GitHub's public Stacks REST API, removing the old Personal Access Token restriction. `st doctor` marks earlier versions as out of date and `st doctor --fix` upgrades them, even though `gh stack link` itself remains available on older link-capable releases.

## How it works

stax still owns local stack management: branch creation, parent metadata, restack, submit, PR bodies, and body/comment stack links. After `st submit` creates or updates the PRs, stax can run:

```bash
gh stack link <pr> [<next-pr> ...] --base <trunk> --remote <remote>
```

That registers one or more already-submitted PRs as a native GitHub Stack. stax passes PR numbers in bottom-to-top order and keeps its own body/comment stack links unless you opt out.

<a id="default-behavior"></a>
## Default behavior

The default is zero-config:

```toml
[submit]
native_stack = "auto"
stack_links_when_native = "keep"
```

With `auto`, stax attempts native registration only when the extension is installed, the repo is eligible, and the stack has **at least two PRs** (`gh stack link` requires two or more — a native stack is inherently multi-PR). Single-PR stacks are skipped silently; once a second PR joins the stack, the next submit registers both. If the repo is not enabled for the private preview, stax caches that result locally and stops retrying. Submit still succeeds and behaves like normal stax.

### Authentication across gh-stack versions

With gh-stack v0.0.8+, stax preserves the normal GitHub CLI environment. `GH_TOKEN` and `GITHUB_TOKEN` therefore work for native stack registration in local automation and CI, alongside keyring accounts created by `gh auth login`.

Known older versions still use private-preview endpoints that reject Personal Access Tokens. For those versions only, stax removes `GH_TOKEN`/`GITHUB_TOKEN` from `gh stack link` and `gh stack unstack`, allowing `gh` to fall back to its stored OAuth-authenticated account. This has no effect on stax's own GitHub API calls.

When a known legacy version and a token override are both present, `st doctor` performs one token-stripped `gh auth status` probe. A missing keyring login produces a soft warning that recommends upgrading or running `gh auth login`; v0.0.8+ needs no extra OAuth probe.

`stack_links_when_native = "keep"` means PR body/comment links continue to sync even when GitHub native registration succeeds.

## Base branch ownership after linking

Once a stack is registered natively, GitHub owns base-branch transitions for the linked PRs and rejects any `PATCH` that touches `base` — even from stax's own retarget calls — with:

```
Cannot change the base branch because the pull request is part of a stack.
```

stax treats this as non-fatal wherever it would otherwise just be re-affirming or cascading a base after a merge (`st submit`, and the retarget-after-merge step in `st merge`, `st merge --queue`, `st merge --remote`, and `st merge --when-ready`): it prints a short `note:` and continues instead of aborting. GitHub either applies the retarget itself shortly after (e.g. once the merged branch is deleted) or leaves it for `st stack link` to reconcile later.

Where a base change to trunk is a hard precondition — merging a single PR out of stack order with `st merge --stack` or `st merge --queue` — stax fails with a clear message instead, since proceeding without the real base would merge into the wrong target. Run `st stack unlink` first if you need to do that.

## Forked stacks aren't supported

GitHub's native Stack feature can only represent a single straight line of PRs. If a branch in your local stack has two or more children (e.g. `test-3` has both `test-4` and `test-3-1` branching off it), stax detects that fork itself and skips native registration for that submit, printing a `note:` instead — it never hands `gh stack link` a branch set that doesn't form a real linear chain. This matters because gh-stack doesn't reliably reject forked input: it sometimes does (surfacing as a `409`/`422` from GitHub), but it can also silently accept it and linearize the PRs in whatever order it was given, which would misrepresent which branch each PR actually builds on.

stax's own PR body/comment stack links (see [`stack_links`](../configuration/index.md)) have no such limitation and continue to render forked stacks correctly, with sibling branches indented at the same depth. Run `st stack unlink` on one side of the fork if you need that side registered as its own native stack.

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

`--native-stack` is intentionally more talkative than `auto`: if `gh` is unavailable, `github/gh-stack` is missing, or the extension is too old to expose `gh stack link`, submit still succeeds but prints an actionable `note:` pointing at `st doctor --fix` and the relevant `gh extension` command. In default `auto` mode, those setup gaps stay quiet so ordinary submits are not noisy.
