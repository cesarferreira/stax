mod common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;

/// `st inbox --json` should always emit a well-formed JSON array.
#[test]
fn inbox_json_is_array_on_clean_repo() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["inbox", "--json"]);
    output.assert_success();

    let value: Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("inbox JSON should parse");
    assert!(value.is_array(), "inbox JSON should be an array");
}

/// A healthy stack with no PRs and nothing to do should produce an empty inbox.
#[test]
fn inbox_empty_when_nothing_needs_attention() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-a", "feature-b"]);

    let output = repo.run_stax(&["inbox", "--json"]);
    output.assert_success();

    let items: Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("inbox JSON should parse");
    assert_eq!(
        items.as_array().map(|a| a.len()),
        Some(0),
        "branches with no PR and no pending work should be filtered out: {}",
        TestRepo::stdout(&output)
    );
}

/// A branch that needs a restack must surface as "needs you / restack" without
/// any network access.
#[test]
fn inbox_flags_restack_as_needs_you() {
    let repo = TestRepo::new();

    // Build a parent/child pair, then delete the parent so the child is
    // orphaned and gets reparented to trunk + marked needs_restack.
    repo.run_stax(&["bc", "parent-branch"]).assert_success();
    let parent = repo.current_branch();
    repo.run_stax(&["bc", "child-branch"]).assert_success();
    let child = repo.current_branch();
    repo.run_stax(&["t"]).assert_success();
    repo.git(&["branch", "-D", &parent]).assert_success();

    let output = repo.run_stax(&["inbox", "--json"]);
    output.assert_success();

    let items: Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("inbox JSON should parse");
    let arr = items.as_array().expect("inbox JSON should be an array");

    let entry = arr
        .iter()
        .find(|i| i["branch"].as_str() == Some(child.as_str()))
        .unwrap_or_else(|| panic!("child branch missing from inbox: {}", TestRepo::stdout(&output)));

    assert_eq!(entry["bucket"], "needs_you");
    assert_eq!(entry["next_action"], "restack");
    assert_eq!(entry["needs_restack"], true);
}

/// The default (non-JSON) view should run cleanly and announce inbox zero when
/// there is nothing to do.
#[test]
fn inbox_text_view_reports_inbox_zero() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["inbox"]);
    output
        .assert_success()
        .assert_stdout_contains("Inbox zero");
}
