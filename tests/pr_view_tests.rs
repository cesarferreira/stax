//! Tests for `stax pr view`. The interactive browser itself is covered by unit
//! tests in `src/github/pr_view.rs` and `src/tui/pr_view/`; these cover the CLI
//! surface that doesn't need a terminal or the network.

use crate::common;

use common::{OutputAssertions, TestRepo};

#[test]
fn pr_view_help_describes_the_target() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "view", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);

    assert!(stdout.contains("[TARGET]"), "missing TARGET: {stdout}");
    assert!(stdout.contains("owner/repo#12"), "missing syntax: {stdout}");
}

#[test]
fn pr_view_is_listed_under_pr() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "--help"]);
    output.assert_success();
    assert!(TestRepo::stdout(&output).contains("view"));
}

#[test]
fn pr_view_without_a_terminal_points_to_pr_list() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "view", "12"]);

    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("interactive terminal") && stderr.contains("stax pr list"),
        "expected a terminal-required hint, got: {stderr}"
    );
}

#[test]
fn pr_view_rejects_a_malformed_target_before_checking_the_terminal() {
    let repo = TestRepo::new();

    for target in [
        "not-a-pr",
        "owner#3",
        "https://evil.example/o/r/pull/1",
        "https://github.com/o/r/issues/1",
    ] {
        let output = repo.run_stax(&["pr", "view", target]);
        output.assert_failure();
        let stderr = TestRepo::stderr(&output);
        assert!(
            stderr.contains("isn't a pull request"),
            "{target}: expected a target error, got: {stderr}"
        );
    }
}
