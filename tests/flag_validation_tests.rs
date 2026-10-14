use crate::common;
use common::{OutputAssertions, TestRepo};

#[test]
fn create_insert_and_below_are_mutually_exclusive() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["create", "flag-a", "--insert", "--below"]);
    output
        .assert_failure()
        .assert_stderr_contains("'--insert' cannot be used with '--below'");
}

#[test]
fn create_below_rejects_from() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["create", "flag-b", "--below", "--from", "main"]);
    output
        .assert_failure()
        .assert_stderr_contains("'--below' cannot be used with '--from <FROM>'");
}

#[test]
fn standup_plain_text_requires_ai() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["standup", "--plain-text"]);
    output
        .assert_failure()
        .assert_stderr_contains("--plain-text only applies when used with --ai");
}

#[test]
fn standup_style_requires_ai() {
    let repo = TestRepo::new();

    // "spoken" is a real StandupSummaryStyle variant (src/cli/args.rs); using it
    // ensures the test exercises the `--style` requires `--ai` guard rather than
    // clap rejecting an invalid enum value.
    let output = repo.run_stax(&["standup", "--style", "spoken"]);
    output
        .assert_failure()
        .assert_stderr_contains("required arguments were not provided")
        .assert_stderr_contains("--ai");
}

#[test]
fn resolve_rejects_zero_max_rounds() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["resolve", "--max-rounds", "0"]);
    output
        .assert_failure()
        .assert_stderr_contains("--max-rounds must be at least 1");
}

#[test]
fn checkout_rejects_explicit_branch_with_trunk_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["checkout", "main", "--trunk"]);
    output
        .assert_failure()
        .assert_stderr_contains("Cannot combine explicit branch with --trunk/--parent/--child");
}

#[test]
fn checkout_rejects_explicit_branch_with_parent_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["checkout", "main", "--parent"]);
    output
        .assert_failure()
        .assert_stderr_contains("Cannot combine explicit branch with --trunk/--parent/--child");
}

#[test]
fn changelog_find_rejects_multiple_queries() {
    let repo = TestRepo::new();

    // After `--`, both tokens are treated as literal positionals, so
    // `--find=q1` lands in the `from` slot and `q2` in `to`, reaching the
    // "accepts only one search query" guard in normalize_find_args.
    let output = repo.run_stax(&["changelog", "--", "--find=q1", "q2"]);
    output
        .assert_failure()
        .assert_stderr_contains("`stax changelog --find <query>` accepts only one search query");
}

#[test]
fn changelog_find_rejects_empty_query() {
    let repo = TestRepo::new();

    // Explicit from/to (HEAD HEAD) bypasses tag auto-resolution so the empty
    // --find guard in run_find_query is reached directly.
    let output = repo.run_stax(&["changelog", "HEAD", "HEAD", "--find", ""]);
    output
        .assert_failure()
        .assert_stderr_contains("`stax changelog --find <query>` requires a non-empty query");
}

#[test]
fn worktree_create_rejects_name_and_pick() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["worktree", "create", "flag-c", "--pick"]);
    output
        .assert_failure()
        .assert_stderr_contains("Use either a name or --pick, not both.");
}

#[test]
fn lane_tmux_session_requires_explicit_lane_name() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["lane", "--tmux-session", "flag-session"]);
    output
        .assert_failure()
        .assert_stderr_contains("--tmux-session requires an explicit lane name");
}

#[test]
fn merge_queue_and_ignore_failed_ci_are_mutually_exclusive() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--queue", "--ignore-failed-ci"]);
    output
        .assert_failure()
        .assert_stderr_contains("cannot be used with");
}
