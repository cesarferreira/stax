//! Tests for the `stax board` command.

use crate::common;

use common::{OutputAssertions, TestRepo};

fn configure_remote(repo: &TestRepo, url: &str) {
    let output = repo.git(&["remote", "add", "origin", url]);
    assert!(
        output.status.success(),
        "Failed to add origin: {}",
        TestRepo::stderr(&output)
    );
}

#[test]
fn board_help_lists_flags() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["board", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);

    assert!(stdout.contains("--limit"), "missing --limit: {stdout}");
    assert!(stdout.contains("--tab"), "missing --tab: {stdout}");
    assert!(
        stdout.contains("--interval"),
        "missing --interval: {stdout}"
    );
    assert!(stdout.contains("--plain"), "missing --plain: {stdout}");
}

#[test]
fn board_home_alias_resolves() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["home", "--help"]);
    output.assert_success();
}

#[test]
fn board_plain_without_github_auth_fails_cleanly() {
    let repo = TestRepo::new();
    configure_remote(&repo, "https://github.com/test-owner/test-repo.git");

    let output = repo.run_stax(&["board", "--plain"]);

    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.to_lowercase().contains("auth"),
        "expected an auth-related error, got: {stderr}"
    );
}

#[test]
fn board_rejects_non_github_remote() {
    let repo = TestRepo::new();
    configure_remote(&repo, "https://gitlab.com/test-owner/test-repo.git");

    let output = repo.run_stax(&["board", "--plain"]);

    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("GitHub only"),
        "expected a GitHub-only error, got: {stderr}"
    );
}

#[test]
fn board_is_allowed_during_rebase() {
    let repo = TestRepo::new();
    configure_remote(&repo, "https://github.com/test-owner/test-repo.git");
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(repo.has_rebase_in_progress(), "expected rebase in progress");

    let output = repo.run_stax(&["board", "--plain"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));

    assert!(
        !combined.contains("A rebase is in progress. Resolve"),
        "board should not be blocked by the rebase guard, got:\n{combined}"
    );

    repo.abort_rebase();
}
