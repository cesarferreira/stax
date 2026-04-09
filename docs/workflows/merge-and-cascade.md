# Merge and Cascade

## `st merge`

`st merge` merges PRs from the bottom of your stack up to your current branch.
Use `st merge --when-ready` for the explicit wait-for-ready mode (legacy alias: `st merge-when-ready` / `st mwr`).

### What happens

1. Wait for PR readiness (CI + approvals + mergeability) unless `--no-wait`
2. Merge PR with selected strategy
3. Rebase next branch onto updated trunk
4. Update next PR base
5. Force-push updated branch
6. Repeat until done
7. Run post-merge sync (`st rs --force`) unless `--no-sync`

### Common options

```bash
st merge --dry-run
st merge --all
st merge --method squash
st merge --method merge
st merge --method rebase
st merge --when-ready
st merge --when-ready --interval 10
st merge --remote
st merge --remote --all
st merge --no-wait
st merge --no-delete
st merge --no-sync
st merge --timeout 60
st merge --yes
```

`--when-ready` cannot be combined with `--dry-run`, `--no-wait`, or `--remote`.

### `--remote` mode (GitHub only)

`st merge --remote` merges the stack entirely via the GitHub API. No local git operations are performed (no checkout, rebase, or push) — you can keep working on other branches while it runs. Dependent PR head branches are updated on GitHub using the same mechanism as the **Update branch** button (REST `PUT .../pulls/{pull}/update-branch`).

```bash
st merge --remote
st merge --remote --all
st merge --remote --method squash
st merge --remote --timeout 60
st merge --remote --interval 10
```

After a successful run, run `st rs` to sync your local repository (delete merged local branches, reparent children, etc.). `--remote` uses `--interval` for CI polling, same as `--when-ready`.

`--remote` cannot be combined with `--dry-run`, `--when-ready`, or `--no-wait`. Only **GitHub** is supported (not GitLab/Gitea).

### `--queue` mode (GitHub & GitLab)

`st merge --queue` enqueues your stack PRs into the forge's merge queue instead of merging them one-by-one. The merge queue batches CI so it runs once on the combined result, which is significantly faster for stacks with slow CI pipelines.

```bash
st merge --queue
st merge --queue --all
st merge --queue --yes
```

**What happens:**

1. All stack PRs/MRs are retargeted to trunk
2. Each PR/MR is enqueued into the merge queue via the forge API
3. The forge handles CI validation and merging automatically

Once the forge finishes merging, `st rs` (sync) will automatically detect the merged branches and clean up — just like any other merge path. No extra steps required beyond your normal workflow.

#### GitHub

Uses the `enqueuePullRequest` GraphQL mutation. Requires merge queue enabled in branch protection rules. Available on **GitHub Team and Enterprise Cloud** plans, or on **public repositories** on any plan. See [GitHub's merge queue documentation](https://docs.github.com/en/repositories/configuring-branches-and-merges-in-your-repository/configuring-pull-request-merges/managing-a-merge-queue) for setup instructions.

#### GitLab

Uses the [merge trains REST API](https://docs.gitlab.com/api/merge_trains/). Requires **GitLab Premium or Ultimate** and [merge request pipelines](https://docs.gitlab.com/ci/pipelines/merge_request_pipelines/) configured in `.gitlab-ci.yml`. MRs are added with `auto_merge` so they enter the train once their pipeline succeeds.

#### Gitea / Forgejo

**Not supported.** Gitea does not have a merge queue or merge train feature. Use `st merge` or `st merge --when-ready` instead.

`--queue` cannot be combined with `--dry-run`, `--when-ready`, `--remote`, or `--no-wait`.

### Partial stack merge

```bash
# Stack: main <- auth <- auth-api <- auth-ui <- auth-tests
st checkout auth-api
st merge
```

This merges up to `auth-api` and leaves upper branches to merge later.

During merge flows, descendant branches are rebased with provenance-aware boundaries so already-integrated parent commits are not replayed after squash merges. Follow-up restacks also auto-normalize missing/merged-equivalent parents and keep old boundaries so descendants replay only novel commits.

## `st cascade`

`st cascade` combines restack + push + PR create/update in one flow.

| Command | Behavior |
|---|---|
| `st cascade` | restack -> push -> create/update PRs |
| `st cascade --no-pr` | restack -> push |
| `st cascade --no-submit` | restack only |
| `st cascade --auto-stash-pop` | auto stash/pop dirty worktrees |
