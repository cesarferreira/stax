# Merge and cascade

How to merge an entire stack safely.

## `st merge`

Cascade-merges PRs from the bottom of your stack up to your current branch. For each PR, stax:

1. Waits for readiness (CI + approvals + mergeability) unless `--no-wait`
2. Merges with the selected strategy
3. Rebases the next branch onto updated trunk
4. Updates the next PR base
5. Force-pushes the updated branch
6. Repeats
7. Runs `st rs --force` afterwards unless `--no-sync`

During descendant rebases, boundaries are provenance-aware so already-integrated parent commits are not replayed after squash merges.

### Common options

```bash
st merge --dry-run
st merge --all
st merge --downstack-only                 # alias: --ds
st merge --method squash|merge|rebase
st merge --stack                           # validate current PR once, land through current
st merge --stack --downstack-only          # land ancestors below current through one PR
st merge --stack --full                    # land the full stack even from the middle
st merge --stack --when-ready              # wait only for the selected tip PR, then land
st merge --when-ready                       # wait for readiness explicitly
st merge --when-ready --interval 10
st merge --no-wait --no-delete --no-sync
st merge --timeout 60 --yes
```

`--downstack-only` (`--ds`) merges only ancestors below the current branch, then rebases the current branch onto trunk and keeps descendants stacked above it. It composes with `--stack`, and is incompatible with `--all`, `--full`, `--remote`, and `--queue`.

`--full` is only valid with `--stack`; it includes descendants above the current branch in the selected stack merge.

`--when-ready` is incompatible with `--dry-run`, `--no-wait`, `--remote`, and `--queue`. With `--stack`, it waits only for the selected tip PR.

### Partial stack merge

Checkout the branch you want to merge up to, then:

```bash
# stack: main ← auth ← auth-api ← auth-ui ← auth-tests
st checkout auth-api
st merge
```

Merges up to `auth-api`; `auth-ui` and `auth-tests` remain for later.

### Downstack-only merge

Use `--downstack-only` when you want to land prerequisites but keep the checked-out branch open:

```bash
# stack: main ← auth ← auth-api ← auth-ui ← auth-tests
st checkout auth-ui
st merge --ds
```

Merges `auth` and `auth-api`; `auth-ui` is rebased onto `main`, and `auth-tests` remains stacked on `auth-ui`.

## `st merge --stack` (GitHub only)

Fast-forwards the selected stack range through one GitHub PR merge. By default the selected range is stack bottom through the current branch:

```bash
st merge --stack
st merge --stack --when-ready
st merge --stack --downstack-only
st merge --stack --full
st merge --stack --dry-run
```

For `main ← A ← B ← C` while checked out on `B`, stax checks that local `main` matches `origin/main`, verifies the local stack is linear, checks `A` for review blockers, validates CI/mergeability on selected tip PR `B`, retargets `B` to `main`, and merges only `B` through GitHub's merge API. Stax then waits briefly for GitHub to mark PR `A` merged; if GitHub does not, Stax marks `A` as absorbed with a comment pointing at `B`. PR `C` remains open and is rebased/retargeted onto `main`.

Use `st merge --stack --downstack-only` to exclude the checked-out branch from the selected range. Use `st merge --stack --full` to include descendants above the current branch and land the full stack through the actual stack tip. The default merge method for `--stack` is `rebase`; pass `--method squash` only when you explicitly want GitHub to squash the selected range into one commit.

This avoids re-running CI for every lower PR because the selected tip already contains that range. The post-merge sync updates trunk and PR metadata without running generic merged-branch deletion; branch cleanup stays scoped to the stack range that was just landed. If trunk moves before the merge, stax aborts and asks you to restack and wait for fresh selected-tip CI.

GitHub may still display an absorbed lower PR as closed rather than merged if its background merge detection does not fire. In that fallback, Stax leaves an explicit absorbed-by comment so the closure is intentional and traceable.

For the no-extra-CI behavior, GitHub branch protection should require status checks but should not require branches to be up to date before merging. If GitHub requires up-to-date branches, it can force another revalidation at merge time.

## `st merge --remote` (GitHub only)

Merges the entire stack via the GitHub API — no local git operations. You can keep working on other branches while it runs. Dependent PR head branches are updated on GitHub using the same mechanism as the **Update branch** button (REST `PUT .../pulls/{pull}/update-branch`).

```bash
st merge --remote
st merge --remote --all
st merge --remote --method squash
st merge --remote --interval 10 --timeout 60
```

After a successful run, `st rs` locally to clean up. Incompatible with `--dry-run`, `--when-ready`, and `--no-wait`. GitLab/Gitea not supported.

## `st merge --queue`

Enqueue the stack into your forge's merge queue (GitHub) or merge trains (GitLab). The forge batches CI so it runs once on the combined result.

```bash
st merge --queue
st merge --queue --all --yes
```

Flow: retarget all PRs to trunk → enqueue each → poll until merged (respects `--timeout` and `--interval`) → auto `st rs` unless `--no-sync` → desktop notification.

| Forge | Requirement |
|---|---|
| **GitHub** | Merge queue enabled in branch protection. Available on Team/Enterprise Cloud or any public repo. ([setup docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)) |
| **GitLab** | Premium or Ultimate + [merge request pipelines](https://docs.gitlab.com/ci/pipelines/merge_request_pipelines/). Uses the [merge trains API](https://docs.gitlab.com/api/merge_trains/). MRs enter the train when their pipeline succeeds. |
| **Gitea / Forgejo** | Not supported. Use `st merge` or `st merge --when-ready`. |

`--queue` is incompatible with `--dry-run`, `--when-ready`, `--remote`, and `--no-wait`.

## `st cascade`

Restack + push + create/update PRs in a single flow.

| Command | Behavior |
|---|---|
| `st cascade` | restack → push → create/update PRs |
| `st cascade --no-pr` | restack → push |
| `st cascade --no-submit` | restack only |
| `st cascade --auto-stash-pop` | auto stash/pop dirty worktrees |

## `st update`

The "bottom PR merged, catch me up" command. Prints the plan up front, then syncs trunk without merged-branch cleanup, restacks, and submits.

| Command | Behavior |
|---|---|
| `st update` | sync trunk → restack → push → create/update PRs |
| `st update --no-pr` | sync trunk → restack → push |
| `st update --no-submit` | sync trunk → restack |
| `st update --force` | force the sync step instead of prompting |
| `st update --force --yes --no-prompt` | run the full trunk-sync/restack/submit flow without prompts |
| `st update --verbose` | show detailed sync/restack/submit timing |
