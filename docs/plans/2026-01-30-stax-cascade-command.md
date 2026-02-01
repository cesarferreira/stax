# Stax Cascade Command Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a new `stax cascade` command that restacks the entire current stack from the bottom, then submits updates non-interactively, so stacks are fully up to date and pushed.

**Architecture:** Implement a new command module that orchestrates existing command functions: navigate to the bottom branch, restack the current stack, upstack restack descendants, and submit with `--yes --no-prompt`. Guard against rebase-in-progress to avoid submitting during conflicts, and restore the original branch when safe. Wire the new subcommand into CLI parsing and command dispatch, and document it in the README.

**Tech Stack:** Rust, clap (CLI), existing stax command modules.

### Task 1: Add failing test for cascade (no-submit smoke test)

**Files:**
- Modify: `tests/integration_tests.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn test_cascade_no_submit_keeps_original_branch() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.run_stax(&["bc", "feature-2"]);
    let original = repo.current_branch();

    let output = repo.run_stax(&["cascade", "--no-submit"]);
    assert!(output.status.success());

    let after = repo.current_branch();
    assert_eq!(after, original, "cascade should restore original branch");
}
```

**Step 2: Run test to verify it fails**

Run: `RUSTC_WRAPPER= cargo test test_cascade_no_submit_keeps_original_branch -v`
Expected: FAIL with "unrecognized subcommand 'cascade'" (or similar clap error).

### Task 2: Add the `cascade` command module

**Files:**
- Create: `src/commands/cascade.rs`
- Modify: `src/commands/mod.rs`

**Step 1: Write minimal implementation**

```rust
use crate::commands;
use crate::git::GitRepo;
use anyhow::Result;
use colored::Colorize;

pub fn run(no_submit: bool, no_pr: bool) -> Result<()> {
    let repo = GitRepo::open()?;
    let original = repo.current_branch()?;

    println!("{}", "Cascading stack...".bold());

    commands::navigate::bottom()?;
    commands::restack::run(false, false, true)?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    commands::upstack::restack::run()?;

    if repo.rebase_in_progress()? {
        return Ok(());
    }

    if !no_submit {
        commands::submit::run(
            false,
            no_pr,
            false,
            true,
            true,
            vec![],
            vec![],
            vec![],
            false,
            None,
            false,
            false,
        )?;
    }

    if !repo.rebase_in_progress()? && repo.current_branch()? != original {
        repo.checkout(&original)?;
    }

    Ok(())
}
```

**Step 2: Wire module into command registry**

Add to `src/commands/mod.rs`:

```rust
pub mod cascade;
```

### Task 3: Add CLI subcommand + dispatch

**Files:**
- Modify: `src/main.rs`

**Step 1: Add new command definition**

```rust
    /// Restack the current stack from the bottom and submit updates
    Cascade {
        /// Skip submit step (restack only)
        #[arg(long)]
        no_submit: bool,
        /// Only push, don't create/update PRs
        #[arg(long)]
        no_pr: bool,
    },
```

**Step 2: Dispatch command in main match**

```rust
        Commands::Cascade { no_submit, no_pr } => commands::cascade::run(no_submit, no_pr),
```

### Task 4: Update README command list

**Files:**
- Modify: `README.md`

**Step 1: Add cascade entry**

Add to the command table near restack/submit:

```
| `stax cascade` | Restack from bottom, upstack restack, and submit updates |
```

### Task 5: Run tests

**Step 1: Run targeted test**

Run: `RUSTC_WRAPPER= cargo test test_cascade_no_submit_keeps_original_branch -v`
Expected: PASS

**Step 2: (Optional) Full test suite**

Run: `RUSTC_WRAPPER= cargo test`
Expected: PASS

### Task 6: Commit

```bash
git add tests/integration_tests.rs src/commands/cascade.rs src/commands/mod.rs src/main.rs README.md

git commit -m "feat: add cascade command to restack and submit stack"
```
