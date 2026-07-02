# GitHub native Stacked PRs

GitHub's native Stacked PRs feature adds a stack map, final-target branch protection, and native stack rebase/merge controls to the GitHub PR UI. stax can register its existing stacked PRs with that native feature when the repo has access.

## How it works

stax still owns local stack management: branch creation, parent metadata, restack, submit, PR bodies, and body/comment stack links. After `st submit` creates or updates the PRs, stax can run:

```bash
gh stack link <bottom-pr> <next-pr> ... --base <trunk> --remote <remote>
```

That registers the already-submitted PRs as a native GitHub Stack. stax passes PR numbers in bottom-to-top order and keeps its own body/comment stack links unless you opt out.

## Requirements

- GitHub remote.
- Repo has GitHub native Stacked PRs enabled.
- GitHub CLI `gh` is installed.
- `github/gh-stack` extension is installed:

```bash
gh extension install github/gh-stack
```

`st doctor` reports this status. `st doctor --fix` can offer to install the extension when `gh` is available.

## Default behavior

The default is zero-config:

```toml
[submit]
native_stack = "auto"
stack_links_when_native = "keep"
```

With `auto`, stax attempts native registration only when the extension is installed and the repo is eligible. If the repo is not enabled for the private preview, stax caches that result locally and stops retrying. Submit still succeeds and behaves like normal stax.

`stack_links_when_native = "keep"` means PR body/comment links continue to sync even when GitHub native registration succeeds.

## Manual commands

```bash
st stack link
st stack unlink
```

Use `st stack link` to re-register the current stack manually. Use `st stack unlink` to remove the native GitHub Stack object without deleting branches or stax metadata.

## Submit overrides

```bash
st submit --native-stack     # force an attempt for this run
st submit --no-native-stack  # skip native registration for this run
```

These only affect native GitHub registration. PR creation, branch pushes, and stax-managed stack links continue to follow the normal submit options.
