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
st merge --stack --when-ready              # wait only for the selected tip PR/MR, then land
st merge --when-ready                       # wait for readiness explicitly
st merge --when-ready --interval 10
st merge --no-wait --no-delete --no-sync
st merge --timeout 60 --yes
```

`--downstack-only` (`--ds`) merges only ancestors below the current branch, then rebases the current branch onto trunk and keeps descendants stacked above it. It composes with `--stack`, and is incompatible with `--all`, `--full`, `--remote`, and `--queue`.

`--full` is only valid with `--stack`; it includes descendants above the current branch in the selected stack merge.

`--when-ready` is incompatible with `--dry-run`, `--no-wait`, `--remote`, and `--queue`. With `--stack`, it waits only for the selected tip PR/MR.

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

## `st merge --stack` (GitHub and GitLab)

Lands the selected stack range through one SHA-preserving tip PR/MR merge. By default the selected range is stack bottom through the current branch:

```bash
st merge --stack
st merge --stack --when-ready
st merge --stack --downstack-only
st merge --stack --full
st merge --stack --dry-run
```

For `main ← A ← B ← C` while checked out on `B`, stax checks that local `main` matches `origin/main`, verifies the local stack is linear, checks `A` for review blockers, and validates CI/mergeability on selected tip `B`. With the default `merge` method, Stax targets both `A` and `B` to `main` before merging only `B`. Preserving the existing commit SHAs and making them reachable from `main` lets GitHub or GitLab mark `A` indirectly merged. `C` remains open and is rebased/retargeted onto `main`.

Use `st merge --stack --downstack-only` to exclude the checked-out branch from the selected range. Use `st merge --stack --full` to include descendants above the current branch and land the full stack through the actual stack tip. The default merge method for `--stack` is `merge`. On GitLab, Stax first checks that the project uses `merge`, `rebase_merge`, or `ff` and does not require squashing, then sends `squash: false`; explicit stack `rebase` and `squash` are rejected before mutation. GitHub also rejects those rewriting methods for multi-PR ranges before mutation because rewritten SHAs prevent lower PRs from reaching genuine merged state. A single selected GitHub PR has no lower PR to reconcile, so it may still use `rebase` or `squash`.

This avoids re-running CI for every lower PR because the selected tip already contains that range. The post-merge sync updates trunk and PR metadata without running generic merged-branch deletion; branch cleanup stays scoped to the stack range that was just landed. If trunk moves before the merge, stax aborts and asks you to restack and wait for fresh selected-tip CI.

Indirect-merge detection is asynchronous. Stax requires GitLab's authoritative `state: merged` (a merely closed MR does not count). If a lower PR/MR is still open after the bounded poll, Stax leaves it open and reports it as pending so the forge can reconcile it later. Any base changes are restored if retargeting, the final trunk check, or the selected-tip merge fails.

Gitea/Forgejo does not support `st merge --stack`.

For the no-extra-CI behavior, GitHub branch protection should require status checks but should not require branches to be up to date before merging. If GitHub requires up-to-date branches, it can force another revalidation at merge time.

### Native GitHub Stacks: atomic merge via `gh stack merge`

If the current repo's stack is a confirmed-enabled native GitHub Stack (see [GitHub native Stacked PRs](../integrations/github-native-stacks.md)) and the installed `github/gh-stack` extension is v0.1.0+, `st merge --stack` delegates to `gh stack merge` instead of the retarget-then-merge flow above: GitHub lands every selected PR up to the tip atomically, or none of them. This skips the per-PR base retargeting and indirect-merge polling described above entirely. If the base branch uses a merge queue, the stack may be enqueued instead of merged immediately; `st merge --stack` reports that and skips local branch cleanup (run `st sync` once the queue lands it). If the installed `gh-stack` predates v0.1.0, `st merge --stack` falls back to the forge-API flow above and prints a `note:` recommending an upgrade.

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
st merge --queue --timeout 60 --interval 10
```

Flow: retarget all PRs to trunk → enqueue each → poll until merged (respects `--timeout` and `--interval`) → auto `st rs` unless `--no-sync` → desktop notification.

`--timeout` accepts positive whole minutes (default: 30), and `--interval` accepts positive whole seconds (default: 15); zero is rejected for either flag. The queue timeout is a hard polling deadline: the final sleep is capped to the remaining time, and stax does not begin another forge status poll once that deadline is reached.

| Forge | Requirement |
|---|---|
| **GitHub** | Merge queue enabled in branch protection. Available on Team/Enterprise Cloud or any public repo. ([setup docs](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue)) |
| **GitLab** | Premium or Ultimate + [merge request pipelines](https://docs.gitlab.com/ci/pipelines/merge_request_pipelines/). Uses the [merge trains API](https://docs.gitlab.com/api/merge_trains/). MRs enter the train when their pipeline succeeds. |
| **Gitea / Forgejo** | Not supported. Use `st merge` or `st merge --when-ready`. |

`--queue` is incompatible with `--dry-run`, `--when-ready`, `--remote`, and `--no-wait`.

## `st cascade`

Restack + push + create/update PRs in a single flow, without fetching trunk (offline-friendly).

| Command | Behavior |
|---|---|
| `st cascade` | restack → push → create/update PRs |
| `st cascade --no-pr` | restack → push |
| `st cascade --no-submit` | restack only |
| `st cascade --auto-stash-pop` | auto stash/pop dirty worktrees |

## `st refresh`

The "bottom PR merged, catch me up" command. Prints the plan up front, then syncs trunk without merged-branch cleanup, restacks, and submits.

| Command | Behavior |
|---|---|
| `st refresh` | sync trunk → restack → push → create/update PRs |
| `st refresh --no-pr` | sync trunk → restack → push |
| `st refresh --no-submit` | sync trunk → restack |
| `st refresh --all-stacks` | sync trunk once → restack and submit every independent stack; needs a clean tree unless `--auto-stash-pop` is set; stops at the first conflict |
| `st refresh --force` | force the sync step instead of prompting |
| `st refresh --force --yes --no-prompt` | run the full trunk-sync/restack/submit flow without prompts |
| `st refresh --verbose` | show detailed sync/restack/submit timing |
