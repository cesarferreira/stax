mod common;
use common::{OutputAssertions, TestRepo};

#[test]
fn test_rerequest_review_flag_accepted() {
    // Verify --rerequest-review flag is accepted by the CLI parser
    let repo = TestRepo::new();
    let output = repo.run_stax(&["submit", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("--rerequest-review");
}

#[test]
fn test_rerequest_review_flag_in_branch_submit() {
    // Verify flag is available in branch submit too
    let repo = TestRepo::new();
    let output = repo.run_stax(&["branch", "submit", "--help"]);
    output.assert_success();
    output.assert_stdout_contains("--rerequest-review");
}
