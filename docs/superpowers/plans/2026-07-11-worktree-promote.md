# Worktree Promote Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `st wt promote`, which safely retires the current linked worktree and checks its branch out in the main worktree.

**Architecture:** A focused `commands::worktree::promote` transaction will preflight both checkouts, run the existing removal hooks, detach the linked checkout, switch the main checkout, and then reuse the existing remove-or-park primitive. Small Git helpers provide detached checkout and HEAD capture for rollback. The existing shell-control protocol will relocate integrated shells only after the transaction succeeds.

**Tech Stack:** Rust, clap, git CLI/libgit2 wrapper, Stax worktree pool/hooks, consolidated nextest integration suite.

## Global Constraints

- The command surface is `st worktree promote` and `st wt promote`; v1 accepts no selector, `--stash`, or `--force`.
- Both the linked and main worktrees must be clean and free of locks, conflicts, rebases, or merges.
- Preserve branch commits, upstreams, PR links, and Stax metadata.
- Roll back both checkout locations after unexpected switch or retirement failures when their paths remain available.
- Use existing worktree hooks, pooling, removal, and shell-output mechanisms.
- Update user-facing documentation and `skills.md` in the same change.
- Use targeted `cargo nextest run worktree_promote_tests::` during development and `make test` for final full-suite validation.

---

### Task 1: Core promotion transaction

**Files:**
- Create: `src/commands/worktree/promote.rs`
- Modify: `src/commands/worktree/mod.rs`
- Modify: `src/commands/worktree/remove.rs`
- Modify: `src/git/repo.rs`
- Create: `tests/worktree_promote_tests.rs`
- Modify: `tests/all_tests.rs`

**Interfaces:**
- Consumes: `GitRepo::list_worktrees`, `compute_worktree_details`, worktree hook helpers, and `RemovalMode::AllowParking`.
- Produces: `promote::run(shell_output: bool) -> anyhow::Result<()>`, `GitRepo::switch_detached_in(cwd: &Path, target: Option<&str>) -> anyhow::Result<()>`, `GitRepo::head_oid_in(cwd: &Path) -> anyhow::Result<String>`, and `remove::retire_worktree(...) -> anyhow::Result<String>`.

- [ ] **Step 1: Write failing end-to-end tests**

Create `tests/worktree_promote_tests.rs` with helpers that create a committed feature branch in a linked worktree. Add tests that invoke `repo.run_stax_in(&linked, &["wt", "promote"])` and assert:

```rust
assert!(output.status.success());
assert_eq!(git_branch(&repo.path()), feature_branch);
assert!(!linked.exists());
assert!(repo.path().join("feature.txt").exists());
```

Add bad-path tests that dirty the linked checkout, dirty the main checkout, detach the linked checkout, and invoke promote from the main checkout. Each must fail, preserve the original worktree registrations/checkouts, and include the blocking reason in stderr. Register the module in `tests/all_tests.rs`.

- [ ] **Step 2: Run the new tests and confirm the command is absent**

Run: `cargo nextest run worktree_promote_tests::`

Expected: FAIL because clap does not recognize `promote`.

- [ ] **Step 3: Add rollback-capable Git helpers**

Add these focused methods in `src/git/repo.rs`, using the existing `run_git` error style:

```rust
pub(crate) fn head_oid_in(&self, cwd: &Path) -> Result<String>;

pub(crate) fn switch_detached_in(
    &self,
    cwd: &Path,
    target: Option<&str>,
) -> Result<()>;
```

`head_oid_in` runs `git rev-parse HEAD`. `switch_detached_in` runs `git switch --detach` with an optional target and includes the path and Git stderr in failures.

- [ ] **Step 4: Split hook orchestration from removal storage**

Extract the park-or-remove body of `remove_worktree_with_hooks` into:

```rust
pub(crate) fn retire_worktree(
    repo: &GitRepo,
    config: &Config,
    worktree: &WorktreeInfo,
    force: bool,
    mode: RemovalMode,
) -> Result<String>;
```

Keep `remove_worktree_with_hooks` behavior unchanged by running `pre_remove`, calling `retire_worktree`, then starting `post_remove`. This lets promotion run the pre-hook before changing checkout state and the post-hook only after the transaction commits.

- [ ] **Step 5: Implement the promotion transaction**

In `src/commands/worktree/promote.rs`, implement:

```rust
pub fn run(shell_output: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let config = Config::load()?;
    let worktrees = repo.list_worktrees()?;
    let source = worktrees.iter().find(|wt| wt.is_current).cloned()
        .context("Could not identify the current worktree")?;
    let main = worktrees.iter().find(|wt| wt.is_main).cloned()
        .context("Could not identify the main worktree")?;

    if source.is_main {
        bail!("Cannot promote the main worktree.");
    }
    let branch = source.branch.clone().context(
        "Cannot promote a detached worktree. Check out a branch first.",
    )?;
    ensure_clean_checkout(
        "current linked worktree",
        &compute_worktree_details(&repo, source.clone())?,
    )?;
    ensure_clean_checkout(
        "main worktree",
        &compute_worktree_details(&repo, main.clone())?,
    )?;

    let main_head = repo.head_oid_in(&main.path)?;
    run_blocking_hook(
        config.worktree.hooks.pre_remove.as_deref(),
        &main.path,
        "pre_remove",
    )?;
    repo.switch_detached_in(&source.path, None)?;

    if let Err(error) = repo.switch_branch_in(&main.path, &branch) {
        let rollback = repo.switch_branch_in(&source.path, &branch).err();
        return Err(transaction_error("switch the main worktree", error, rollback));
    }

    std::env::set_current_dir(&main.path)?;
    if let Err(error) = retire_worktree(
        &repo,
        &config,
        &source,
        false,
        RemovalMode::AllowParking,
    ) {
        let main_rollback = restore_checkout(&repo, &main, &main_head).err();
        let source_rollback = repo.switch_branch_in(&source.path, &branch).err();
        return Err(transaction_errors(
            "retire the linked worktree",
            error,
            [main_rollback, source_rollback],
        ));
    }

    spawn_background_hook(
        config.worktree.hooks.post_remove.as_deref(),
        &main.path,
        "post_remove",
    )?;
    finish_success(shell_output, &main.path, &branch);
    Ok(())
}
```

Define `ensure_clean_checkout(label: &str, details: &WorktreeDetails) -> Result<()>` to reject, in order, missing/prunable paths, locks, dirty state, rebases, merges, and conflicts with messages beginning `Cannot promote: the {label}`. Define `restore_checkout` to switch back to `main.branch` when present or detach at `main_head` otherwise. Define `transaction_error`/`transaction_errors` to preserve the original error and append non-empty rollback errors, and `finish_success` to emit either shell-control lines or direct-invocation guidance. Before retirement, call `std::env::set_current_dir(&main.path)` so removal is not executed from inside the retiring directory.

- [ ] **Step 6: Run and fix the focused tests**

Run: `cargo nextest run worktree_promote_tests::`

Expected: PASS for the happy path and all preflight errors.

- [ ] **Step 7: Add injected Git-failure rollback tests**

On Unix, add a temporary `git` shim that delegates to the real Git binary except when environment variables request failure of either `git switch <feature>` in the main worktree or `git worktree remove <linked>`. Assert both failures leave the main worktree on its original checkout and the linked worktree on the feature branch.

- [ ] **Step 8: Run the complete promote module**

Run: `cargo nextest run worktree_promote_tests::`

Expected: PASS, including both rollback tests.

- [ ] **Step 9: Commit the transaction**

```bash
git add src/commands/worktree/promote.rs src/commands/worktree/mod.rs \
  src/commands/worktree/remove.rs src/git/repo.rs \
  tests/worktree_promote_tests.rs tests/all_tests.rs
git commit -m "feat: add worktree promotion transaction"
```

### Task 2: CLI and shell integration

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/tests.rs`
- Modify: `src/commands/shell_setup.rs`
- Modify: `tests/worktree_promote_tests.rs`

**Interfaces:**
- Consumes: `promote::run(shell_output: bool)` and existing `STAX_SHELL_PATH` / `STAX_SHELL_MESSAGE` output parsing.
- Produces: clap parsing for `WorktreeCommands::Promote { shell_output: bool }` and POSIX/fish routing through `__stax_run_worktree_shell`.

- [ ] **Step 1: Add failing parser and shell-output tests**

Add a clap test asserting `st wt promote --shell-output` parses as `WorktreeCommands::Promote { shell_output: true }`. Add an integration assertion that successful shell-output mode includes:

```text
STAX_SHELL_PATH=<absolute main worktree path>
STAX_SHELL_MESSAGE=Promoted '<branch>' to the main worktree
```

Add shell-snippet unit assertions that both generated snippets route `promote` through `__stax_run_worktree_shell`.

- [ ] **Step 2: Run tests and confirm parser/routing failures**

Run: `cargo nextest run cli::tests worktree_promote_tests:: commands::shell_setup::tests`

Expected: FAIL because the variant and shell dispatch case are missing.

- [ ] **Step 3: Wire clap and command dispatch**

Add to `WorktreeCommands`:

```rust
/// Move the current linked-worktree branch to the main worktree
Promote {
    #[arg(long, hide = true)]
    shell_output: bool,
},
```

Dispatch it in `src/cli/mod.rs` with:

```rust
Some(WorktreeCommands::Promote { shell_output }) =>
    commands::worktree::promote::run(shell_output),
```

- [ ] **Step 4: Route integrated shells**

In both POSIX and fish snippets, include `promote` in the worktree subcommand case that calls `__stax_run_worktree_shell`. On success, the wrapper consumes the payload and changes directory; on failure, `__stax_run_worktree_shell` returns before changing directory.

- [ ] **Step 5: Implement direct-invocation guidance**

When `shell_output` is false, print the main path and a copyable `cd <path>` line with `Current shell did not move automatically.`. If shell integration is not installed, also print the existing `stax setup` tip. Do not emit shell-control lines on any failure.

- [ ] **Step 6: Run CLI, shell, and promotion tests**

Run: `cargo nextest run cli::tests worktree_promote_tests:: commands::shell_setup::tests`

Expected: PASS.

- [ ] **Step 7: Commit CLI integration**

```bash
git add src/cli/args.rs src/cli/mod.rs src/cli/tests.rs \
  src/commands/shell_setup.rs tests/worktree_promote_tests.rs
git commit -m "feat: expose worktree promote command"
```

### Task 3: User documentation

**Files:**
- Modify: `README.md`
- Modify: `docs/worktrees/index.md`
- Modify: `docs/commands/reference.md`
- Modify: `skills.md`

**Interfaces:**
- Consumes: the completed `st wt promote` behavior.
- Produces: consistent command maps, workflow guidance, safety rules, and AI-agent guidance.

- [ ] **Step 1: Update every command map**

Add `st wt promote` alongside create/go/remove in each command table. Describe it as retiring the current linked worktree and checking its branch out in the main worktree.

- [ ] **Step 2: Document safety and shell behavior**

In `docs/worktrees/index.md`, add a short promotion section covering clean-checkout requirements, preserved metadata, rollback behavior, shell integration, and the manual `cd` fallback. Explicitly state that the command does not stash or delete the branch.

- [ ] **Step 3: Check documentation consistency**

Run: `rg -n "wt promote|worktree promote" README.md docs skills.md`

Expected: entries in the worktree overview, command reference, canonical worktree guide, and agent skill map.

- [ ] **Step 4: Commit documentation**

```bash
git add README.md docs/worktrees/index.md docs/commands/reference.md skills.md
git commit -m "docs: document worktree promotion"
```

### Task 4: Verification and Stax PR

**Files:**
- Verify all files changed by Tasks 1-3.

**Interfaces:**
- Consumes: completed implementation, tests, and documentation.
- Produces: formatted, fully tested branch submitted as a Stax PR.

- [ ] **Step 1: Format and inspect the diff**

Run: `cargo fmt --all -- --check`

Expected: PASS. If it fails, run `cargo fmt --all`, inspect the formatting-only diff, and rerun the check.

Run: `git diff --check main...HEAD && git status --short`

Expected: no whitespace errors and only intentional files.

- [ ] **Step 2: Run targeted verification once more**

Run: `cargo nextest run worktree_promote_tests::`

Expected: PASS.

- [ ] **Step 3: Run the repository-prescribed full suite**

Run: `make test`

Expected: PASS through the Docker test path on macOS. If Docker is unavailable, launch Docker Desktop as directed by `AGENTS.md` and retry; do not fall back to a native full suite.

- [ ] **Step 4: Review final branch state**

Run: `git status --short && git log --oneline main..HEAD && stax status`

Expected: a clean tracked `codex/worktree-promote` branch stacked on `main`.

- [ ] **Step 5: Submit the Stax PR**

Run: `stax submit --no-fetch --draft --no-template --edit`

Expected: the branch is pushed and a draft PR is created or updated with `main` as its base. Remove `--edit` if the environment is non-interactive and use `stax submit --ai --yes` to generate the title/body instead.
