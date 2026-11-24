//! Integration tests for `st pull` (sync --get --all-stacks --restack, then restack every stack).

use crate::common::{OutputAssertions, TestRepo};
use crate::sync_get_tests::{push_all, two_pushed_stacks, update_remote_branch_in_clone};

fn is_ancestor(repo: &TestRepo, ancestor: &str, descendant: &str) -> bool {
    repo.git(&["merge-base", "--is-ancestor", ancestor, descendant])
        .status
        .success()
}

fn refs_snapshot(repo: &TestRepo) -> String {
    TestRepo::stdout(&repo.git(&[
        "for-each-ref",
        "--format=%(refname) %(objectname)",
        "refs/heads",
        "refs/remotes",
    ]))
}

fn file_on_branch(repo: &TestRepo, branch: &str, file: &str) -> bool {
    repo.git(&["cat-file", "-e", &format!("{branch}:{file}")])
        .status
        .success()
}

fn advance_trunk(repo: &TestRepo) {
    repo.simulate_remote_commit("trunk-advance.txt", "trunk\n", "Advance trunk");
}

#[test]
fn pull_restacks_every_stack_from_a_stack_branch() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-a", "pl-a1"], &["pl-b"]);
    advance_trunk(&repo);
    repo.git(&["checkout", &a[1]]).assert_success();

    repo.run_stax(&["pull"]).assert_success();

    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
    assert!(is_ancestor(&repo, &a[0], &a[1]));
    assert_eq!(repo.current_branch(), a[1]);
}

#[test]
fn pull_restacks_every_stack_from_trunk() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-ta", "pl-ta1"], &["pl-tb"]);
    advance_trunk(&repo);

    repo.run_stax(&["pull"]).assert_success();

    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn pull_reconciles_then_restacks() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-ra", "pl-ra1"], &["pl-rb"]);
    update_remote_branch_in_clone(&repo, &a[0], "other-a.txt", "a");
    update_remote_branch_in_clone(&repo, &b[0], "other-b.txt", "b");
    advance_trunk(&repo);

    repo.run_stax(&["pull"]).assert_success();

    assert!(file_on_branch(&repo, &a[0], "other-a.txt"));
    assert!(file_on_branch(&repo, &a[1], "other-a.txt"));
    assert!(is_ancestor(&repo, &a[0], &a[1]));
    assert!(file_on_branch(&repo, &b[0], "other-b.txt"));
    assert!(is_ancestor(&repo, "main", &b[0]));
    assert!(is_ancestor(&repo, "main", &a[0]));
}

fn diverge_second_stack(repo: &TestRepo, b: &str) -> String {
    let remote = update_remote_branch_in_clone(repo, b, "other-machine.txt", "other");
    repo.git(&["checkout", b]).assert_success();
    repo.create_file("local-only.txt", "local");
    repo.commit("Local only");
    repo.git(&["checkout", "main"]).assert_success();
    remote
}

#[test]
fn pull_force_lets_remote_win() {
    let repo = TestRepo::new_with_remote();
    let (_a, b) = two_pushed_stacks(&repo, &["pl-fa"], &["pl-fb"]);
    let remote = diverge_second_stack(&repo, &b[0]);

    repo.run_stax(&["pull", "--force"]).assert_success();

    assert_eq!(repo.get_commit_sha(&b[0]), remote);
    assert!(!file_on_branch(&repo, &b[0], "local-only.txt"));
}

#[test]
fn pull_without_force_rebases_diverged_branch() {
    let repo = TestRepo::new_with_remote();
    let (_a, b) = two_pushed_stacks(&repo, &["pl-na"], &["pl-nb"]);
    let remote = diverge_second_stack(&repo, &b[0]);

    repo.run_stax(&["pull"]).assert_success();

    assert!(is_ancestor(&repo, &remote, &b[0]));
    assert!(file_on_branch(&repo, &b[0], "local-only.txt"));
}

#[test]
fn pull_auto_stash_pop_restacks_dirty_linked_worktree() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-sa", "pl-sa1"], &["pl-sb"]);
    advance_trunk(&repo);

    let wt_root = tempfile::TempDir::new().expect("temp dir");
    let linked = wt_root.path().join("pull-wt");
    repo.git(&["worktree", "add", linked.to_str().unwrap(), &a[0]])
        .assert_success();
    let scratch = linked.join("scratch.txt");
    std::fs::write(&scratch, "local scratch\n").expect("write dirty file");

    repo.run_stax(&["pull", "--force", "--auto-stash-pop"])
        .assert_success();

    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
    assert_eq!(
        std::fs::read_to_string(&scratch).expect("scratch kept"),
        "local scratch\n"
    );
}

#[test]
fn pull_rejects_dirty_tree_without_auto_stash_pop() {
    let repo = TestRepo::new_with_remote();
    let (_a, b) = two_pushed_stacks(&repo, &["pl-da"], &["pl-db"]);
    update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    advance_trunk(&repo);
    repo.create_file("dirty.txt", "x");
    let before = refs_snapshot(&repo);

    let out = repo.run_stax(&["pull"]);
    out.assert_failure();
    out.assert_stderr_contains("--auto-stash-pop");
    out.assert_stderr_contains("`st pull`");
    assert_eq!(refs_snapshot(&repo), before);
}

#[test]
fn pull_stops_at_conflict_in_second_stack_and_rerun_finishes() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();

    let a = repo.create_stack(&["pl-ca"]);
    repo.git(&["checkout", "main"]).assert_success();
    let b = repo.create_stack(&["pl-cb"]);
    repo.create_file("conflict.txt", "branch version\n");
    repo.commit("Prepare pull conflict");
    repo.git(&["checkout", "main"]).assert_success();
    let c = repo.create_stack(&["pl-cc"]);
    let c_before = repo.get_commit_sha(&c[0]);

    repo.simulate_remote_commit("conflict.txt", "trunk version\n", "Advance trunk");
    repo.git(&["checkout", &a[0]]).assert_success();

    let out = repo.run_stax(&["pull"]);
    out.assert_failure();
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains(&format!("Stopped at '{}'", b[0])),
        "{stdout}"
    );
    assert!(stdout.contains(&format!("Done: {}", a[0])), "{stdout}");
    assert!(stdout.contains(&format!("Remaining: {}", c[0])), "{stdout}");
    assert!(stdout.contains("re-run `st pull`"), "{stdout}");
    assert!(repo.has_rebase_in_progress(), "{stdout}");
    assert_eq!(repo.get_commit_sha(&c[0]), c_before);
    let a_after = repo.get_commit_sha(&a[0]);

    repo.resolve_conflicts_ours();
    repo.run_stax(&["continue"]).assert_success();
    repo.run_stax(&["pull"]).assert_success();

    for branch in a.iter().chain(b.iter()).chain(c.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
    assert_eq!(repo.get_commit_sha(&a[0]), a_after);
}

#[test]
fn pull_rejects_unsupported_flags() {
    let repo = TestRepo::new_with_remote();
    two_pushed_stacks(&repo, &["pl-ua"], &["pl-ub"]);
    let before = refs_snapshot(&repo);

    for flag in ["--json", "--get", "--all-stacks", "--continue"] {
        repo.run_stax(&["pull", flag]).assert_failure();
    }
    assert_eq!(refs_snapshot(&repo), before);
}

#[test]
fn pull_no_restack_only_reconciles() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-oa"], &["pl-ob"]);
    let remote_b = update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    advance_trunk(&repo);

    repo.run_stax(&["pull", "--no-restack"]).assert_success();

    assert_eq!(repo.get_commit_sha(&b[0]), remote_b);
    assert!(!is_ancestor(&repo, "main", &a[0]));

    repo.create_file("dirty.txt", "x");
    let out = repo.run_stax(&["pull", "--no-restack"]);
    assert!(
        !TestRepo::stderr(&out).contains("`st pull` restacks every stack"),
        "{}",
        TestRepo::stderr(&out)
    );
    out.assert_failure();
    assert!(
        TestRepo::stderr(&out).contains("--auto-stash-pop"),
        "{}",
        TestRepo::stderr(&out)
    );
    assert!(!TestRepo::stderr(&out).contains("not a terminal"));
}

fn stash_count(repo: &TestRepo) -> usize {
    TestRepo::stdout(&repo.git(&["stash", "list"]))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count()
}

#[test]
fn pull_no_restack_dirty_tree_without_terminal_fails_cleanly() {
    let repo = TestRepo::new_with_remote();
    let (_a, b) = two_pushed_stacks(&repo, &["pl-na"], &["pl-nb"]);
    update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    advance_trunk(&repo);
    repo.create_file("dirty.txt", "x");
    let before = refs_snapshot(&repo);
    let stashes = stash_count(&repo);

    let out = repo.run_stax(&["pull", "--no-restack"]);

    out.assert_failure();
    let stderr = TestRepo::stderr(&out);
    assert!(stderr.contains("Working tree is dirty"), "{stderr}");
    assert!(stderr.contains("--auto-stash-pop"), "{stderr}");
    assert!(!stderr.contains("re-run with --stash "), "{stderr}");
    assert!(!stderr.contains("not a terminal"), "{stderr}");
    assert_eq!(refs_snapshot(&repo), before);
    assert_eq!(stash_count(&repo), stashes);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("dirty.txt")).unwrap(),
        "x"
    );
}

#[test]
fn pull_no_restack_auto_stash_pop_stashes_dirty_tree_without_terminal() {
    let repo = TestRepo::new_with_remote();
    let (_a, b) = two_pushed_stacks(&repo, &["pl-sa"], &["pl-sb"]);
    let remote_b = update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    repo.create_file("dirty.txt", "keep me");
    let stashes = stash_count(&repo);

    let out = repo.run_stax(&["pull", "--no-restack", "--auto-stash-pop"]);

    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(stdout.contains("Stashed working tree changes."), "{stdout}");
    assert!(stdout.contains("Restored stashed changes."), "{stdout}");
    assert_eq!(stash_count(&repo), stashes);
    assert_eq!(repo.get_commit_sha(&b[0]), remote_b);
    assert_eq!(
        std::fs::read_to_string(repo.path().join("dirty.txt")).unwrap(),
        "keep me"
    );
}

#[test]
fn pull_dry_run_makes_no_changes() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-pa"], &["pl-pb"]);
    update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    advance_trunk(&repo);
    repo.create_file("dirty.txt", "x");
    let head = repo.head_sha();
    let before = refs_snapshot(&repo);

    for flag in ["--dry-run", "--plan"] {
        let out = repo.run_stax(&["pull", flag]);
        out.assert_success();
        let stdout = TestRepo::stdout(&out);
        assert!(
            stdout.contains("Get (all tracked branches vs remote)"),
            "{stdout}"
        );
        assert!(stdout.contains("Restack (every stack)"), "{stdout}");
        assert!(stdout.contains(&a[0]) && stdout.contains(&b[0]), "{stdout}");
        assert_eq!(repo.head_sha(), head);
        assert_eq!(refs_snapshot(&repo), before);
    }
}

#[test]
fn pull_skips_frozen_branch() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-za"], &["pl-zb"]);
    repo.run_stax(&["freeze", &b[0]]).assert_success();
    let frozen_before = repo.get_commit_sha(&b[0]);
    advance_trunk(&repo);

    repo.run_stax(&["pull"]).assert_success();

    assert_eq!(repo.get_commit_sha(&b[0]), frozen_before);
    assert!(is_ancestor(&repo, "main", &a[0]));
}

#[test]
fn pull_with_no_tracked_stacks_still_syncs_trunk() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    advance_trunk(&repo);

    repo.run_stax(&["pull"]).assert_success();

    assert_eq!(
        repo.get_commit_sha("main"),
        repo.get_commit_sha("origin/main")
    );
    assert!(file_on_branch(&repo, "main", "trunk-advance.txt"));
}

fn merged_fixture() -> (TestRepo, Vec<String>) {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]).assert_success();
    let stack = repo.create_stack(&["pl-ma"]);
    repo.git(&["checkout", "main"]).assert_success();
    push_all(&repo, &stack);
    repo.merge_branch_on_remote(&stack[0]);
    (repo, stack)
}

#[test]
fn pull_no_delete_keeps_merged_branch_and_default_deletes_it() {
    let (repo, stack) = merged_fixture();
    repo.run_stax(&["pull", "--force", "--no-delete"])
        .assert_success();
    assert!(repo.list_branches().contains(&stack[0]));

    let (repo, stack) = merged_fixture();
    repo.run_stax(&["pull", "--force"]).assert_success();
    assert!(!repo.list_branches().contains(&stack[0]));
}

fn assert_no_sync_plan_prompt(text: &str) {
    assert!(
        !text.contains("How should sync proceed"),
        "pull must not show the sync plan prompt:\n{text}"
    );
}

#[test]
fn pull_non_interactive_never_shows_sync_plan_prompt() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-na"], &["pl-nb"]);
    advance_trunk(&repo);

    let out = repo.run_stax(&["pull"]);
    out.assert_success();
    assert_no_sync_plan_prompt(&TestRepo::stdout(&out));
    assert_no_sync_plan_prompt(&TestRepo::stderr(&out));
    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
}

#[test]
fn pull_on_a_tty_never_shows_sync_plan_prompt() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-ta"], &["pl-tb"]);
    advance_trunk(&repo);

    let out = crate::common::run_stax_in_script(&repo.path(), &["pull"], "sleep 3");
    let combined = format!("{}{}", TestRepo::stdout(&out), TestRepo::stderr(&out));
    assert_no_sync_plan_prompt(&combined);
    out.assert_success();
    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
}

fn needs_restack_fixture(repo: &TestRepo, prefix: &str) -> (Vec<String>, Vec<String>) {
    let a0 = format!("{prefix}-a0");
    let a1 = format!("{prefix}-a1");
    let b0 = format!("{prefix}-b0");
    let (a, b) = two_pushed_stacks(repo, &[a0.as_str(), a1.as_str()], &[b0.as_str()]);
    repo.git(&["checkout", &a[0]]).assert_success();
    repo.create_file("local-a0.txt", "local\n");
    repo.commit("Local commit on a0");
    repo.git(&["checkout", &a[1]]).assert_success();
    (a, b)
}

fn assert_needs_restack_fixture_restacked(repo: &TestRepo, a: &[String], b: &[String]) {
    assert!(is_ancestor(repo, &a[0], &a[1]), "a1 not restacked on a0");
    assert!(file_on_branch(repo, &a[1], "local-a0.txt"));
    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(repo, "main", branch), "{branch} not on main");
    }
    assert_eq!(repo.current_branch(), a[1]);
}

#[test]
fn pull_non_tty_with_current_stack_needing_restack_skips_plan_prompt() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = needs_restack_fixture(&repo, "pl-nr");
    advance_trunk(&repo);

    let out = repo.run_stax(&["pull"]);
    out.assert_success();
    assert_no_sync_plan_prompt(&TestRepo::stdout(&out));
    assert_no_sync_plan_prompt(&TestRepo::stderr(&out));
    assert_needs_restack_fixture_restacked(&repo, &a, &b);
}

const CANCEL_IF_PLAN_PROMPTS: &str = "if STAX_TUI_WAIT_ATTEMPTS=40 wait_for_tui_text \"How should sync proceed?\"; then printf '\\033[B\\033[B\\n'; fi";

#[test]
fn pull_on_a_tty_with_current_stack_needing_restack_skips_plan_prompt() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = needs_restack_fixture(&repo, "pl-tr");
    advance_trunk(&repo);

    let home = repo.clean_home();
    let out = crate::common::run_stax_in_script_with_env(
        &repo.path(),
        &["pull"],
        CANCEL_IF_PLAN_PROMPTS,
        &[("HOME", &home)],
    );
    // The input script's own "timed out waiting" notice goes to stderr and names the prompt.
    assert_no_sync_plan_prompt(&TestRepo::stdout(&out));
    out.assert_success();
    assert_needs_restack_fixture_restacked(&repo, &a, &b);
}

#[test]
fn pull_auto_stash_pop_restores_dirty_current_worktree_without_force() {
    let repo = TestRepo::new_with_remote();
    let (a, b) = two_pushed_stacks(&repo, &["pl-ca"], &["pl-cb"]);
    update_remote_branch_in_clone(&repo, &b[0], "other.txt", "x");
    advance_trunk(&repo);
    repo.git(&["checkout", &a[0]]).assert_success();
    std::fs::write(repo.path().join(format!("{}.txt", a[0])), "dirty\n").expect("dirty tracked");

    let out = repo.run_stax(&["pull", "--auto-stash-pop"]);
    out.assert_success();
    assert_no_sync_plan_prompt(&TestRepo::stderr(&out));
    for branch in a.iter().chain(b.iter()) {
        assert!(is_ancestor(&repo, "main", branch), "{branch} not on main");
    }
    assert!(file_on_branch(&repo, &b[0], "other.txt"));
    assert_eq!(
        std::fs::read_to_string(repo.path().join(format!("{}.txt", a[0]))).unwrap(),
        "dirty\n"
    );
}
