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

fn submit_stack_verbose(repo: &TestRepo) -> String {
    submit_stack_with(
        repo,
        &[
            "ss",
            "--no-pr",
            "--no-template",
            "--no-prompt",
            "--yes",
            "--verbose",
        ],
    )
}

fn submit_stack_verbose_expect_failure(repo: &TestRepo) -> String {
    let out = repo.run_stax(&[
        "ss",
        "--no-pr",
        "--no-template",
        "--no-prompt",
        "--yes",
        "--verbose",
    ]);
    let combined = format!("{}\n{}", TestRepo::stdout(&out), TestRepo::stderr(&out));
    assert!(
        !out.status.success(),
        "submit unexpectedly succeeded:\n{combined}"
    );
    combined
}

fn worktree_registrations(repo: &TestRepo) -> String {
    let out = repo.git(&["worktree", "list", "--porcelain"]);
    assert!(
        out.status.success(),
        "worktree list failed: {}",
        TestRepo::stderr(&out)
    );
    TestRepo::stdout(&out)
}

fn assert_worktree_registrations_unchanged(repo: &TestRepo, before: &str) {
    let after = worktree_registrations(repo);
    assert_eq!(
        after, before,
        "temporary submit worktree leaked:\nbefore:\n{before}\nafter:\n{after}"
    );
}

fn assert_ancestor(repo: &TestRepo, ancestor: &str, descendant: &str, message: &str) {
    let out = repo.git(&["merge-base", "--is-ancestor", ancestor, descendant]);
    assert!(out.status.success(), "{message}");
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

/// Thesis part 3: a temporary restack only needs the resulting commit id, so
/// it must be replayed in the object database rather than paying for a
/// `git worktree add` + `rm -rf`, which is what made a 4-5 branch stack stall
/// for minutes in a large repository.
#[test]
fn temp_restack_replays_without_creating_a_worktree() {
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
        combined.contains("2 rebase(s): 2 replayed in-memory, 0 worktree(s) created"),
        "expected both rebases to replay without a worktree, got:\n{combined}"
    );
}

/// Signed commits must use Git's real rebase path: replaying in the object
/// database would otherwise publish an unsigned replacement commit.
#[test]
fn signed_temp_restack_uses_worktree_and_preserves_signing_failure() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let bc = repo.run_stax(&["bc", "signed"]);
    assert!(bc.status.success(), "bc signed: {}", TestRepo::stderr(&bc));
    repo.create_file("signed.txt", "signed");
    repo.commit("Commit requiring a signature when rebased");

    let co = repo.git(&["checkout", "main"]);
    assert!(
        co.status.success(),
        "checkout main: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("trunk.txt", "moved");
    repo.commit("Trunk moves forward");
    let co = repo.git(&["checkout", "signed"]);
    assert!(
        co.status.success(),
        "checkout signed: {}",
        TestRepo::stderr(&co)
    );

    let signer = common::stax_bin();
    let config = repo.git(&["config", "--local", "commit.gpgsign", "true"]);
    assert!(
        config.status.success(),
        "set commit.gpgsign: {}",
        TestRepo::stderr(&config)
    );
    let config = repo.git(&["config", "--local", "gpg.format", "openpgp"]);
    assert!(
        config.status.success(),
        "set gpg.format: {}",
        TestRepo::stderr(&config)
    );
    let config = repo.git(&[
        "config",
        "--local",
        "gpg.program",
        signer
            .to_str()
            .expect("compiled stax binary path must be UTF-8"),
    ]);
    assert!(
        config.status.success(),
        "set gpg.program: {}",
        TestRepo::stderr(&config)
    );

    let signing_enabled = repo.git(&["config", "--bool", "--get", "commit.gpgsign"]);
    assert!(
        signing_enabled.status.success(),
        "read commit.gpgsign: {}",
        TestRepo::stderr(&signing_enabled)
    );
    assert_eq!(
        TestRepo::stdout(&signing_enabled).trim(),
        "true",
        "fixture must enable repository-local commit signing"
    );
    let configured_signer = repo.git(&["config", "--get", "gpg.program"]);
    assert_eq!(
        TestRepo::stdout(&configured_signer).trim(),
        signer.to_str().unwrap(),
        "fixture must use the deterministic failing signer"
    );

    let worktrees_before = worktree_registrations(&repo);
    let combined = submit_stack_verbose_expect_failure(&repo);

    assert!(
        combined.contains("git rebase") && combined.contains("gpg failed to sign the data"),
        "submit must reach Git's signing rebase failure, got:\n{combined}"
    );
    assert_worktree_registrations_unchanged(&repo, &worktrees_before);
}

/// Merge commits need Git's own rebase semantics, which a linear object
/// database replay cannot preserve.
#[test]
fn merge_history_temp_restack_uses_one_worktree() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let bc = repo.run_stax(&["bc", "merge-history"]);
    assert!(
        bc.status.success(),
        "bc merge-history: {}",
        TestRepo::stderr(&bc)
    );
    let branch = repo.current_branch();

    let co = repo.git(&["checkout", "-b", "merge-source"]);
    assert!(
        co.status.success(),
        "checkout merge-source: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("merge-source.txt", "merge source");
    repo.commit("Commit on merge source");

    let co = repo.git(&["checkout", &branch]);
    assert!(
        co.status.success(),
        "checkout {branch}: {}",
        TestRepo::stderr(&co)
    );
    let merge = repo.git(&[
        "merge",
        "--no-ff",
        "merge-source",
        "-m",
        "Merge merge source",
    ]);
    assert!(
        merge.status.success(),
        "create merge commit: {}",
        TestRepo::stderr(&merge)
    );
    let merge_count = repo.git(&["rev-list", "--merges", "--count", "main..HEAD"]);
    assert!(
        merge_count.status.success(),
        "count merge commits: {}",
        TestRepo::stderr(&merge_count)
    );
    assert_eq!(
        TestRepo::stdout(&merge_count).trim(),
        "1",
        "fixture must contain exactly one merge in the replay range"
    );

    let co = repo.git(&["checkout", "main"]);
    assert!(
        co.status.success(),
        "checkout main: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("trunk.txt", "moved");
    repo.commit("Trunk moves forward");
    let trunk = repo.get_commit_sha("main");
    let co = repo.git(&["checkout", &branch]);
    assert!(
        co.status.success(),
        "checkout {branch}: {}",
        TestRepo::stderr(&co)
    );

    let worktrees_before = worktree_registrations(&repo);
    let combined = submit_stack_verbose(&repo);

    assert!(
        combined.contains("1 rebase(s): 0 replayed in-memory, 1 worktree(s) created"),
        "merge history must use one real worktree, got:\n{combined}"
    );
    let published = repo.get_commit_sha(&format!("refs/remotes/origin/{branch}"));
    assert_ancestor(
        &repo,
        &trunk,
        &published,
        "published merge-history branch must be rebased onto the moved trunk",
    );
    assert_worktree_registrations_unchanged(&repo, &worktrees_before);
}

/// A root commit has no parent to cherry-pick from, so it must also use the
/// worktree path and let Git turn it into a commit based on the moved trunk.
#[test]
fn root_history_temp_restack_uses_one_worktree() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let bc = repo.run_stax(&["bc", "root-history"]);
    assert!(
        bc.status.success(),
        "bc root-history: {}",
        TestRepo::stderr(&bc)
    );
    let branch = repo.current_branch();

    let co = repo.git(&["checkout", "--orphan", "isolated-root"]);
    assert!(
        co.status.success(),
        "checkout orphan: {}",
        TestRepo::stderr(&co)
    );
    let remove = repo.git(&["rm", "-rf", "."]);
    assert!(
        remove.status.success(),
        "clear orphan index: {}",
        TestRepo::stderr(&remove)
    );
    repo.create_file("root.txt", "root history");
    let add = repo.git(&["add", "root.txt"]);
    assert!(
        add.status.success(),
        "stage root commit: {}",
        TestRepo::stderr(&add)
    );
    let commit = repo.git(&["commit", "-m", "Parentless root commit"]);
    assert!(
        commit.status.success(),
        "create root commit: {}",
        TestRepo::stderr(&commit)
    );
    let move_branch = repo.git(&["branch", "-f", &branch, "HEAD"]);
    assert!(
        move_branch.status.success(),
        "replace {branch} history: {}",
        TestRepo::stderr(&move_branch)
    );
    let co = repo.git(&["checkout", &branch]);
    assert!(
        co.status.success(),
        "checkout {branch}: {}",
        TestRepo::stderr(&co)
    );
    let parents = repo.git(&["rev-list", "--parents", "-n", "1", "HEAD"]);
    assert!(
        parents.status.success(),
        "inspect root commit parents: {}",
        TestRepo::stderr(&parents)
    );
    assert_eq!(
        TestRepo::stdout(&parents).split_whitespace().count(),
        1,
        "fixture branch tip must be a parentless root commit"
    );

    let co = repo.git(&["checkout", "main"]);
    assert!(
        co.status.success(),
        "checkout main: {}",
        TestRepo::stderr(&co)
    );
    repo.create_file("trunk.txt", "moved");
    repo.commit("Trunk moves forward");
    let trunk = repo.get_commit_sha("main");
    let co = repo.git(&["checkout", &branch]);
    assert!(
        co.status.success(),
        "checkout {branch}: {}",
        TestRepo::stderr(&co)
    );

    let worktrees_before = worktree_registrations(&repo);
    let combined = submit_stack_verbose(&repo);

    assert!(
        combined.contains("1 rebase(s): 0 replayed in-memory, 1 worktree(s) created"),
        "root history must use one real worktree, got:\n{combined}"
    );
    let published = repo.get_commit_sha(&format!("refs/remotes/origin/{branch}"));
    assert_ancestor(
        &repo,
        &trunk,
        &published,
        "published root-history branch must be rebased onto the moved trunk",
    );
    assert_worktree_registrations_unchanged(&repo, &worktrees_before);
}

/// The object-database replay must bail (not guess) when the rebase conflicts,
/// handing over to the real worktree rebase so the user gets git's own output.
#[test]
fn conflicting_temp_restack_falls_back_to_a_real_worktree() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

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

    let out = repo.run_stax(&[
        "ss",
        "--no-pr",
        "--no-template",
        "--no-prompt",
        "--yes",
        "--verbose",
    ]);
    let combined = format!("{}\n{}", TestRepo::stdout(&out), TestRepo::stderr(&out));

    assert!(
        !out.status.success(),
        "conflicting restack should still fail, got:\n{combined}"
    );
    // The real rebase ran, which is where git's conflict output comes from.
    assert!(
        combined.contains("CONFLICT") || combined.contains("could not apply"),
        "expected git's own conflict output via the worktree fallback, got:\n{combined}"
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

/// The strongest available check on the object-database replay: the commit it
/// publishes must be byte-identical to what `git rebase --onto` produces, for
/// awkward commit messages as well as simple ones.
///
/// Equality of the full SHA covers tree, parent, author identity/date and the
/// message bytes all at once.
#[test]
fn replayed_commits_are_identical_to_a_real_rebase() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let bc = repo.run_stax(&["bc", "base"]);
    assert!(bc.status.success(), "bc base: {}", TestRepo::stderr(&bc));

    // Messages that exercise formatting the replay must not normalise away.
    repo.create_file("one.txt", "1");
    repo.git(&["add", "-A"]);
    // Message files live outside the working tree so they never become content.
    let msg_dir = std::env::temp_dir().join(format!("stax-msg-{}", std::process::id()));
    std::fs::create_dir_all(&msg_dir).unwrap();
    let msg_path = msg_dir.join("msg.txt");
    let msg = "Subject line\n\nBody paragraph one.\n\nBody paragraph two.\n\nCo-Authored-By: Someone <s@example.com>\n";
    std::fs::write(&msg_path, msg).unwrap();
    let c = repo.git(&["commit", "-F", msg_path.to_str().unwrap()]);
    assert!(c.status.success(), "commit: {}", TestRepo::stderr(&c));

    repo.create_file("two.txt", "2");
    repo.git(&["add", "-A"]);
    let msg2 = "Unicode ✅ émoji 🎉 and trailing spaces   \n\nsecond line\n";
    std::fs::write(&msg_path, msg2).unwrap();
    let c = repo.git(&["commit", "-F", msg_path.to_str().unwrap()]);
    assert!(c.status.success(), "commit2: {}", TestRepo::stderr(&c));

    let branch = repo.current_branch();
    let branch_tip = repo.get_commit_sha(&branch);
    let upstream = repo.get_commit_sha("main");

    // Move trunk so the branch needs a restack.
    repo.git(&["checkout", "main"]);
    repo.create_file("trunk.txt", "moved");
    repo.commit("Trunk moves forward");
    repo.git(&["checkout", &branch]);

    // Reference: what a real `git rebase --onto` produces.
    let wt = repo.path().join("refwt");
    let add = repo.git(&[
        "worktree",
        "add",
        "--detach",
        wt.to_str().unwrap(),
        &branch_tip,
    ]);
    assert!(
        add.status.success(),
        "worktree add: {}",
        TestRepo::stderr(&add)
    );
    let rebase = repo.git_in(&wt, &["rebase", "--onto", "main", &upstream]);
    assert!(
        rebase.status.success(),
        "reference rebase: {}",
        TestRepo::stderr(&rebase)
    );
    let reference = String::from_utf8_lossy(&repo.git_in(&wt, &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_string();
    repo.git(&["worktree", "remove", "--force", wt.to_str().unwrap()]);

    submit_stack(&repo);

    let published = repo.get_commit_sha(&format!("refs/remotes/origin/{branch}"));

    // Compare the two chains commit by commit rather than by tip SHA.
    //
    // Committer *timestamp* is "now" for both `git rebase` and the replay, and
    // it feeds the commit id — so the tip SHA, and equally the tip's parent
    // SHA, differ whenever the two runs straddle a second boundary. Walking the
    // chain and comparing the fields that carry meaning is both
    // timestamp-independent and stricter than a single tip comparison.
    let commits = |tip: &str| -> Vec<String> {
        String::from_utf8_lossy(
            &repo
                .git(&["rev-list", "--reverse", &format!("main..{tip}")])
                .stdout,
        )
        .split_whitespace()
        .map(str::to_string)
        .collect()
    };

    let published_chain = commits(&published);
    let reference_chain = commits(&reference);
    assert_eq!(
        published_chain.len(),
        reference_chain.len(),
        "replayed chain must have the same number of commits as `git rebase --onto`"
    );
    assert_eq!(
        published_chain.len(),
        2,
        "fixture should produce two commits above trunk"
    );

    let field = |commit: &str, format: &str| {
        String::from_utf8_lossy(
            &repo
                .git(&["log", "-1", &format!("--format={format}"), commit])
                .stdout,
        )
        .trim()
        .to_string()
    };
    let raw_message = |commit: &str| repo.git(&["log", "-1", "--format=%B", "-z", commit]).stdout;

    for (index, (got, want)) in published_chain
        .iter()
        .zip(reference_chain.iter())
        .enumerate()
    {
        for (format, label) in [
            ("%T", "tree"),
            ("%an", "author name"),
            ("%ae", "author email"),
            ("%aI", "author date"),
        ] {
            assert_eq!(
                field(got, format),
                field(want, format),
                "commit {index}: {label} must match `git rebase --onto` exactly"
            );
        }

        // Exact bytes, not the trimmed form: an extra trailing newline is
        // precisely the kind of drift that is easy to miss.
        assert_eq!(
            raw_message(got),
            raw_message(want),
            "commit {index}: message bytes must match `git rebase --onto` exactly"
        );
    }
}
