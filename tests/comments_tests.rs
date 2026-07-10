//! Comments command integration tests
//!
//! Tests for the `comments` command that shows PR comments.
//! Note: Full functionality requires GitHub API access, so we test
//! pre-condition validation that exits before API calls.

use crate::common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_test_config(home: &Path, api_base_url: &str) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("config directory");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{api_base_url}\"\n"),
    )
    .expect("test config");
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, parent: &str, pr_number: u64) {
    let metadata = serde_json::json!({
        "parentBranchName": parent,
        "parentBranchRevision": repo.get_commit_sha(parent),
        "prInfo": { "number": pr_number, "state": "OPEN" }
    });
    let file = tempfile::NamedTempFile::new().expect("metadata file");
    fs::write(file.path(), metadata.to_string()).expect("metadata contents");
    let hash = repo.git(&["hash-object", "-w", file.path().to_str().unwrap()]);
    hash.assert_success();
    repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{branch}"),
        TestRepo::stdout(&hash).trim(),
    ])
    .assert_success();
}

// =============================================================================
// Help Tests
// =============================================================================

#[test]
fn test_comments_help() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["comments", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("comments"));
    assert!(stdout.contains("PR"));
}

// =============================================================================
// Error Case Tests (validation before API calls)
// =============================================================================

#[test]
fn test_comments_on_trunk_fails() {
    let repo = TestRepo::new();
    // Stay on main (trunk)

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    // On trunk, the command may fail with "not tracked", "trunk", or "No PR" message
    // depending on how the validation logic proceeds
    assert!(
        stderr.contains("not tracked") || stderr.contains("trunk") || stderr.contains("No PR"),
        "Expected message about untracked/trunk branch or missing PR, got: {}",
        stderr
    );
}

#[test]
fn test_comments_untracked_branch_fails() {
    let repo = TestRepo::new();

    // Create an untracked branch directly with git
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("not tracked") || stderr.contains("track"),
        "Expected message about untracked branch, got: {}",
        stderr
    );
}

#[test]
fn test_comments_no_pr_fails() {
    let repo = TestRepo::new();

    // Create a tracked branch but without a PR
    repo.create_stack(&["feature-a"]);
    assert!(repo.current_branch_contains("feature-a"));

    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No PR") || stderr.contains("submit") || stderr.contains("PR"),
        "Expected message about missing PR, got: {}",
        stderr
    );
}

// =============================================================================
// Integration with Stack
// =============================================================================

#[test]
fn test_comments_from_deep_stack_no_pr() {
    let repo = TestRepo::new();

    // Create a deeper stack
    repo.create_stack(&["feature-a", "feature-b", "feature-c"]);
    assert!(repo.current_branch_contains("feature-c"));

    // Should still fail because no PR exists
    let output = repo.run_stax(&["comments"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No PR") || stderr.contains("submit"),
        "Expected message about missing PR, got: {}",
        stderr
    );
}

#[tokio::test]
async fn reviews_alias_lists_a_stack_inbox_as_json() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &server.uri());
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test/repo.git",
    ])
    .assert_success();
    let branches = repo.create_stack(&["review-parent", "review-child"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 41);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 42);

    for (number, body) in [(41, "Please add a test"), (42, "Looks good")] {
        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/issues/{number}/comments")))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!([{
                    "id": number,
                    "body": body,
                    "user": { "login": "reviewer" },
                    "created_at": "2026-07-10T12:00:00Z"
                }])),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/pulls/{number}/comments")))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;
    }

    let output = repo.run_stax_with_env(
        &["reviews", "--stack", "--json"],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    );
    output.assert_success();
    let inbox: serde_json::Value = serde_json::from_slice(&output.stdout).expect("inbox JSON");
    assert_eq!(inbox["scope"], "stack");
    assert_eq!(inbox["total_comments"], 2);
    assert_eq!(inbox["pull_requests"][0]["branch"], branches[0]);
    assert_eq!(
        inbox["pull_requests"][0]["comments"][0]["body"],
        "Please add a test"
    );
    assert_eq!(inbox["pull_requests"][1]["branch"], branches[1]);
}
