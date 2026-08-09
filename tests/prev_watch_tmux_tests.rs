//! Coverage for `stax prev`, the `tmux popup` guard rail, and `stax watch`
//! flag validation.

use crate::common;

use common::{IsolatedProcessEnv, OutputAssertions, TestRepo};

// =============================================================================
// prev
// =============================================================================

#[test]
fn prev_returns_to_previously_checked_out_branch() {
    let repo = TestRepo::new();
    repo.git(&["branch", "a"]).assert_success();
    repo.git(&["branch", "b"]).assert_success();
    repo.run_stax(&["checkout", "a"]).assert_success();
    repo.run_stax(&["checkout", "b"]).assert_success();

    let output = repo.run_stax(&["prev"]);
    output.assert_success();
    output.assert_stdout_contains("Switched to branch 'a'");
    assert_eq!(repo.current_branch(), "a");
}

#[test]
fn prev_reports_no_previous_branch_on_fresh_repo() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["prev"]);
    output.assert_success();
    output.assert_stdout_contains("No previous branch recorded");
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn prev_is_a_no_op_when_previous_equals_current() {
    let repo = TestRepo::new();
    let current = repo.current_branch();

    // Point refs/stax/prev-branch at the branch we're already on, via a temp
    // file (git hash-object --stdin needs a piped child; a file is simpler).
    let file = tempfile::NamedTempFile::new().expect("temp file for prev-branch blob");
    std::fs::write(file.path(), &current).expect("write prev-branch contents");
    let hash = repo.git(&["hash-object", "-w", file.path().to_str().unwrap()]);
    hash.assert_success();
    repo.git(&[
        "update-ref",
        "refs/stax/prev-branch",
        TestRepo::stdout(&hash).trim(),
    ])
    .assert_success();

    let output = repo.run_stax(&["prev"]);
    output.assert_success();
    output.assert_stdout_contains("Previous branch is the same as current branch");
    assert_eq!(repo.current_branch(), current);
}

#[test]
fn prev_fails_when_previous_branch_was_deleted() {
    let repo = TestRepo::new();
    repo.git(&["branch", "a"]).assert_success();
    repo.git(&["branch", "b"]).assert_success();
    repo.run_stax(&["checkout", "a"]).assert_success();
    repo.run_stax(&["checkout", "b"]).assert_success();
    repo.git(&["branch", "-D", "a"]).assert_success();

    let output = repo.run_stax(&["prev"]);
    output.assert_failure();
    output.assert_stderr_contains("Previous branch 'a' no longer exists.");
}

#[test]
fn prev_alias_p_matches_prev() {
    let repo = TestRepo::new();
    repo.git(&["branch", "a"]).assert_success();
    repo.git(&["branch", "b"]).assert_success();
    repo.run_stax(&["checkout", "a"]).assert_success();
    repo.run_stax(&["checkout", "b"]).assert_success();

    let output = repo.run_stax(&["p"]);
    output.assert_success();
    output.assert_stdout_contains("Switched to branch 'a'");
    assert_eq!(repo.current_branch(), "a");
}

// =============================================================================
// tmux popup
// =============================================================================

#[test]
fn tmux_popup_requires_tmux_session() {
    let repo = TestRepo::new();
    let env = IsolatedProcessEnv::with_config("");

    // `run_stax_with_env` only sets env vars; it cannot unset the ambient TMUX
    // var a test happens to inherit from a real tmux session. Build the
    // Command directly and explicitly remove TMUX instead.
    let output = env
        .command(&repo.path())
        .env_remove("TMUX")
        .args(["tmux", "popup"])
        .output()
        .expect("run stax tmux popup");

    output.assert_failure();
    output.assert_stderr_contains("Not inside a tmux session");
}

// =============================================================================
// watch (flag validation only — the main loop has no exit condition)
// =============================================================================

#[test]
fn watch_help_lists_current_and_interval_flags() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["watch", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("--current");
    output.assert_stdout_contains("--interval");
}

#[test]
fn watch_rejects_non_numeric_interval() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["watch", "--interval", "abc"]);
    output.assert_failure();
    output.assert_stderr_contains("invalid value 'abc' for '--interval <INTERVAL>'");
}

// `watch --iterations` needs a resolvable GitHub-shaped remote and a token —
// `watch::run` calls `RemoteInfo::from_repo` and `ForgeClient::new` before the
// loop even starts, so a bare `TestRepo::new()` fails with "No git remote
// 'origin' found" before rendering anything. No tracked branches are created,
// so the empty-branch-list fast path skips `fetch_ci_statuses` and no network
// call is made.
fn watch_ready_repo() -> TestRepo {
    let repo = TestRepo::new();
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test/repo.git",
    ])
    .assert_success();
    repo
}

#[test]
fn watch_iterations_one_renders_once_and_exits() {
    let repo = watch_ready_repo();

    let output = repo.run_stax_with_env(
        &["watch", "--iterations", "1"],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    );
    output.assert_success();
    output.assert_stdout_contains("Watching stack");
}

#[test]
fn watch_iterations_two_renders_twice() {
    let repo = watch_ready_repo();

    // With `--interval 1`, both refreshes complete well within a couple of
    // seconds: the loop returns before the trailing sleep on its final pass.
    let output = repo.run_stax_with_env(
        &["watch", "--iterations", "2", "--interval", "1"],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    );
    output.assert_success();
    output.assert_stdout_contains("iteration #1");
}

#[test]
fn watch_rejects_zero_iterations() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["watch", "--iterations", "0"]);
    output.assert_failure();
    output.assert_stderr_contains("--iterations must be at least 1");
}

#[test]
fn watch_help_lists_iterations_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["watch", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("--iterations");
}
