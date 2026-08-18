//! Tests for the temporary-restack preparation phase of `stax submit`.
//!
//! Background: when a branch in the stack needs a restack, submit does not
//! refuse to publish — it prepares a *temporary* rebased ref per branch so the
//! pushed content is what the PR should contain. That preparation used to be:
//!
//!   * completely silent (no progress output at all), and
//!   * one full `git worktree add` + `git rebase` + `git worktree remove`
//!     per branch, including for empty branches that need no rebase at all.
//!
//! In a large repository with a 4-5 branch stack that is several minutes of
//! dead air after "Fetching from origin... done". These tests pin the fixed
//! contract: visible per-branch progress, no worktree for empty branches, and
//! byte-identical rebase results.

use crate::common;

use common::TestRepo;

/// Build `main -> base -> empty -> tip`, then move `main` forward so the whole
/// stack needs a restack. `empty` deliberately carries no commits of its own.
///
/// Returns (base, empty, tip) branch names.
fn stale_stack_with_empty_branch(repo: &TestRepo) -> (String, String, String) {
    let bc = repo.run_stax(&["bc", "base"]);
    assert!(bc.status.success(), "bc base: {}", TestRepo::stderr(&bc));
    repo.create_file("base.txt", "base");
    repo.commit("Commit for base");
    let base = repo.current_branch();

    // Empty branch: created on top of `base` but never committed to.
    let bc = repo.run_stax(&["bc", "empty"]);
    assert!(bc.status.success(), "bc empty: {}", TestRepo::stderr(&bc));
    let empty = repo.current_branch();

    let bc = repo.run_stax(&["bc", "tip"]);
    assert!(bc.status.success(), "bc tip: {}", TestRepo::stderr(&bc));
    repo.create_file("tip.txt", "tip");
    repo.commit("Commit for tip");
    let tip = repo.current_branch();

    // Move trunk forward so `base` (and therefore the whole stack) is stale.
    let co = repo.git(&["checkout", "main"]);
    assert!(
        co.status.success(),
        "checkout main: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("trunk.txt", "moved");
    repo.commit("Trunk moves forward");

    let co = repo.git(&["checkout", &tip]);
    assert!(
        co.status.success(),
        "checkout tip: {}",
        TestRepo::stderr(&co)
    );

    (base, empty, tip)
}

/// Run a stack submit on the PR-creating (legacy) submit path.
///
/// `--no-pr` alone routes to the newer application backend, which does not do
/// temporary-restack preparation. `--no-template` keeps us on the same code
/// path a real `stax ss` takes while `--no-pr` keeps the test offline.
fn submit_stack(repo: &TestRepo) -> String {
    submit_stack_with(
        repo,
        &["ss", "--no-pr", "--no-template", "--no-prompt", "--yes"],
    )
}

fn submit_stack_with(repo: &TestRepo, args: &[&str]) -> String {
    let out = repo.run_stax(args);
    let stdout = TestRepo::stdout(&out);
    let stderr = TestRepo::stderr(&out);
    assert!(
        out.status.success(),
        "submit failed:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    format!("{stdout}\n{stderr}")
}

/// Thesis part 1: the preparation phase must not be silent.
///
/// Before the fix the only output was a single summary line printed *after*
/// every worktree had been created, rebased and torn down — i.e. after the
/// entire stall. There must be per-branch progress while the work happens.
#[test]
fn temp_restack_preparation_reports_per_branch_progress() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let (base, _empty, tip) = stale_stack_with_empty_branch(&repo);

    let combined = submit_stack(&repo);

    assert!(
        combined.contains("Preparing restack"),
        "expected per-branch restack progress output, got:\n{combined}"
    );
    for branch in [&base, &tip] {
        assert!(
            combined.contains(branch.as_str()),
            "expected branch '{branch}' named in preparation progress, got:\n{combined}"
        );
    }
}

/// Thesis part 2: an empty branch has the same tip commit as its parent by
/// definition, so its rebased tip is exactly the parent's rebased tip. It must
/// reuse the parent's prepared ref instead of paying for its own worktree
/// checkout + rebase + teardown.
#[test]
fn empty_branches_do_not_get_their_own_temp_worktree() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let (_base, _empty, _tip) = stale_stack_with_empty_branch(&repo);

    let combined = submit_stack(&repo);

    // Only `base` and `tip` genuinely need a rebase; `empty` must not.
    assert!(
        combined.contains("Prepared 2 temporary restack refs"),
        "expected exactly 2 prepared refs (empty branch skipped), got:\n{combined}"
    );
}

/// Thesis part 3: the rebases must all share one worktree instead of paying
/// for a `git worktree add` + `rm -rf` per branch, which is what made a 4-5
/// branch stack stall for minutes in a large repository.
#[test]
fn temp_restack_reuses_a_single_worktree_for_every_branch() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let (_base, _empty, _tip) = stale_stack_with_empty_branch(&repo);

    let combined = submit_stack_with(
        &repo,
        &[
            "ss",
            "--no-pr",
            "--no-template",
            "--no-prompt",
            "--yes",
            "--verbose",
        ],
    );

    assert!(
        combined.contains("2 rebase(s) across 1 worktree(s)"),
        "expected both rebases to share one worktree, got:\n{combined}"
    );
}

/// Regression guard for the worktree-reuse optimisation: reusing a single
/// worktree across branches must produce exactly the same published commits as
/// one-worktree-per-branch did.
///
/// `empty` must land on the remote at the same commit as `base` (that is what
/// "empty" means), and `tip` must sit one commit above it with both the trunk
/// commit and the base commit in its history.
#[test]
fn temp_restack_publishes_correctly_rebased_commits() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let (base, empty, tip) = stale_stack_with_empty_branch(&repo);

    submit_stack(&repo);

    let remote_sha = |branch: &str| repo.get_commit_sha(&format!("refs/remotes/origin/{branch}"));

    let trunk_sha = repo.get_commit_sha("main");
    let base_sha = remote_sha(&base);
    let empty_sha = remote_sha(&empty);
    let tip_sha = remote_sha(&tip);

    assert_eq!(
        base_sha, empty_sha,
        "empty branch must be published at the same commit as its parent"
    );
    assert_ne!(base_sha, tip_sha, "tip must be published above its parent");

    // The rebase actually happened: the published base contains the new trunk
    // commit, and the published tip contains the published base.
    let contains = |ancestor: &str, descendant: &str| {
        repo.git(&["merge-base", "--is-ancestor", ancestor, descendant])
            .status
            .success()
    };
    assert!(
        contains(&trunk_sha, &base_sha),
        "published base must be rebased onto the moved trunk"
    );
    assert!(
        contains(&base_sha, &tip_sha),
        "published tip must be rebased onto the published base"
    );
}

/// The reused worktree must not survive a failed rebase.
///
/// With one worktree per branch a conflict left at most that branch's worktree
/// behind; now a single worktree is held across the whole phase, so its cleanup
/// on the error path is worth pinning rather than assuming.
#[test]
fn failed_temp_restack_leaves_no_worktree_behind() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    // `base` edits a file that trunk then edits differently, so replaying
    // `base` onto the moved trunk conflicts.
    let bc = repo.run_stax(&["bc", "base"]);
    assert!(bc.status.success(), "bc base: {}", TestRepo::stderr(&bc));
    repo.create_file("shared.txt", "from base");
    repo.commit("Base edits shared");

    let co = repo.git(&["checkout", "main"]);
    assert!(
        co.status.success(),
        "checkout main: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("shared.txt", "from trunk");
    repo.commit("Trunk edits shared");

    let co = repo.git(&["checkout", "base"]);
    assert!(
        co.status.success(),
        "checkout base: {}",
        TestRepo::stderr(&co)
    );

    let worktrees_before = repo.git(&["worktree", "list"]);
    let before = String::from_utf8_lossy(&worktrees_before.stdout)
        .lines()
        .count();

    let out = repo.run_stax(&["ss", "--no-pr", "--no-template", "--no-prompt", "--yes"]);
    let combined = format!("{}\n{}", TestRepo::stdout(&out), TestRepo::stderr(&out));

    assert!(
        !out.status.success(),
        "submit should fail when the temporary restack conflicts, got:\n{combined}"
    );
    assert!(
        combined.contains("stax restack"),
        "failure should point at `stax restack`, got:\n{combined}"
    );

    let worktrees_after = repo.git(&["worktree", "list"]);
    let after_raw = String::from_utf8_lossy(&worktrees_after.stdout).to_string();
    assert_eq!(
        after_raw.lines().count(),
        before,
        "temporary worktree leaked after a failed restack:\n{after_raw}"
    );
}
