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
gh extension upgrade gh-stack
# or:
st doctor --fix
```

No further setup is required — once the extension is installed, native stack registration is fully automatic (see [Default behavior](#default-behavior) below).

## Requirements

- GitHub remote.
- Repo has GitHub native Stacked PRs enabled.
- GitHub CLI `gh` is installed and logged in with an **OAuth-authenticated account** (`gh auth login`). GitHub's native Stacked PRs API is in private preview and rejects Personal Access Tokens outright.
- `github/gh-stack` extension is installed (see [Install](#install) above), and recent enough to provide the `gh stack link` command (added after `v0.0.1`). Older versions fail with `unknown flag: --base`.

`st doctor` reports this status — including when the installed extension is too old to expose `gh stack link`, or missing entirely. `st doctor --fix` can install the extension when `gh` is available, or upgrade it when it is outdated.

**Recommended: v0.0.6+.** Versions below v0.0.6 report Personal Access Token rejections with the same message used for a genuinely feature-disabled repo ("Stacked PRs are not enabled..."), so stax can't tell the two apart and may incorrectly cache the repo as unsupported. `st doctor` flags this with a soft warning (and `--fix` upgrades it) even though `gh stack link` itself works on any version that exposes the `link` command.

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

### Personal Access Tokens are shadowed automatically

`gh` treats `GH_TOKEN`/`GITHUB_TOKEN` env vars as overriding whichever account you last logged in with, and native Stacked PRs reject that kind of token during private preview. If you export a PAT for other tooling (CI scripts, other CLIs), it would otherwise silently break native stack registration even though you have a perfectly good OAuth login sitting unused. stax works around this: when it shells out to `gh stack link`/`gh stack unstack`, it always strips `GH_TOKEN`/`GITHUB_TOKEN` first, so `gh` falls back to its stored OAuth-authenticated account. This has no effect on stax's own GitHub API calls (PR creation, comments, etc.), which still use your configured token normally.

If no OAuth-authenticated `gh` account is available at all, native registration is skipped with a note pointing at `gh auth login` — this case is never cached as "feature disabled," since it depends on your local `gh` auth state rather than the repo/org's eligibility.

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
