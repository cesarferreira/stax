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
st merge --when-ready                       # wait for readiness explicitly
st merge --when-ready --interval 10
st merge --no-wait --no-delete --no-sync
st merge --timeout 60 --yes
```

`--downstack-only` (`--ds`) merges only ancestors below the current branch, then rebases the current branch onto trunk and keeps descendants stacked above it. It is incompatible with `--all`, `--remote`, and `--queue`.

`--when-ready` is incompatible with `--dry-run`, `--no-wait`, and `--remote`.

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
