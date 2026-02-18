//! Tests for `stax auth` command flags and behavior.

mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_auth_help_includes_from_gh_flag() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["auth", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--from-gh"),
        "Expected --from-gh in help output, got: {}",
        stdout
    );
}

#[test]
fn test_auth_token_conflicts_with_from_gh() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["auth", "--token", "abc123", "--from-gh"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_auth_status_command() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["auth", "status"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("GitHub Auth Status"),
        "Expected auth status header, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Resolution order"),
        "Expected resolution order in status output, got: {}",
        stdout
    );
}
