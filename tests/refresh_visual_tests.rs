//! Visual-presentation checks for `st update`'s header.
//!
//! The header prints before sync/submit run, so we only assert on stdout
//! content and tolerate non-zero exit codes for the paths that would try to
//! push (no forge token is available in tests).

use crate::common::{OutputAssertions, TestRepo};

#[test]
fn update_header_no_submit_uses_skip_wording() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["update", "--no-submit", "--force"]);
    let stdout = TestRepo::stdout(&output);

    output.assert_success();
    assert!(
        stdout.contains("Updating stack"),
        "expected header title, got: {stdout}"
    );
    assert!(
        stdout.contains("Sync trunk"),
        "expected step 1 wording, got: {stdout}"
    );
    assert!(
        stdout.contains("Restack current stack onto updated parents"),
        "expected step 2 wording, got: {stdout}"
    );
    assert!(
        stdout.contains("Skip push and PR updates (--no-submit)"),
        "expected --no-submit step 3 wording, got: {stdout}"
    );
}

#[test]
fn update_header_default_mentions_push_and_prs() {
    let repo = TestRepo::new_with_remote();
    // No forge token in tests — submit will fail, but the header prints first.
    let output = repo.run_stax(&["update", "--force"]);
    let stdout = TestRepo::stdout(&output);

    assert!(
        stdout.contains("Updating stack"),
        "expected header title, got: {stdout}"
    );
    assert!(
        stdout.contains("Push branches and update PRs"),
        "expected default step 3 wording, got: {stdout}"
    );
    assert!(
        !stdout.contains("--no-submit"),
        "default header should not mention --no-submit, got: {stdout}"
    );
}

#[test]
fn update_header_no_pr_mentions_push_without_prs() {
    let repo = TestRepo::new_with_remote();
    let output = repo.run_stax(&["update", "--no-pr", "--force"]);
    let stdout = TestRepo::stdout(&output);

    assert!(
        stdout.contains("Updating stack"),
        "expected header title, got: {stdout}"
    );
    assert!(
        stdout.contains("Push branches without updating PRs"),
        "expected --no-pr step 3 wording, got: {stdout}"
    );
}
