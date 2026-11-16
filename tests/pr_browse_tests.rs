//! Tests for `stax pr browse`. The interactive browser itself is covered by unit
//! tests in `src/github/pr_browse.rs` and `src/tui/pr_browse/`; these cover the CLI
//! surface that doesn't need a terminal or the network.

use crate::common;

use common::{OutputAssertions, TestRepo};

#[test]
fn pr_browse_help_describes_the_target() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "browse", "--help"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);

    assert!(stdout.contains("[TARGET]"), "missing TARGET: {stdout}");
    assert!(stdout.contains("owner/repo#12"), "missing syntax: {stdout}");
}

#[test]
fn pr_browse_is_listed_under_pr() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "--help"]);
    output.assert_success();
    assert!(TestRepo::stdout(&output).contains("browse"));
}

#[test]
fn pr_browse_without_a_terminal_points_to_pr_list() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "browse", "12"]);

    output.assert_failure();
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("interactive terminal") && stderr.contains("stax pr list"),
        "expected a terminal-required hint, got: {stderr}"
    );
}

#[test]
fn pr_browse_rejects_a_malformed_target_before_checking_the_terminal() {
    let repo = TestRepo::new();

    for target in [
        "not-a-pr",
        "owner#3",
        "https://evil.example/o/r/pull/1",
        "https://github.com/o/r/issues/1",
    ] {
        let output = repo.run_stax(&["pr", "browse", target]);
        output.assert_failure();
        let stderr = TestRepo::stderr(&output);
        assert!(
            stderr.contains("isn't a pull request"),
            "{target}: expected a target error, got: {stderr}"
        );
    }
}

// ---------------------------------------------------------------------------
// Non-interactive commands: show, diff, comment, approve
// ---------------------------------------------------------------------------

use crate::common::IsolatedProcessEnv;
use serde_json::{Value, json};
use std::io::Write;
use std::process::Stdio;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn detail_page() -> Value {
    // A raw string: a macro this deep exceeds the recursion limit.
    serde_json::from_str(
        r##"{
        "squashMergeAllowed": true, "mergeCommitAllowed": false, "rebaseMergeAllowed": true,
        "viewerPermission": "WRITE",
        "pullRequest": {
            "number": 12, "title": "Add the thing", "body": "Body text", "state": "OPEN",
            "isDraft": false, "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z", "baseRefName": "main",
            "headRefName": "feature-x", "headRefOid": "abc123", "additions": 3, "deletions": 1,
            "mergeable": "MERGEABLE", "mergeStateStatus": "CLEAN",
            "author": {"login": "ada"},
            "labels": {"nodes": [{"name": "bug"}]},
            "reviewRequests": {"nodes": []},
            "latestReviews": {"nodes": [{"state": "APPROVED", "author": {"login": "carol"}}]},
            "reviews": {"nodes": []},
            "comments": {"nodes": [{
                "body": "please add a test", "createdAt": "2026-01-01T05:00:00Z",
                "author": {"login": "bob"}
            }]},
            "reviewThreads": {"nodes": [{"comments": {"nodes": [{
                "body": "nit: rename", "path": "src/a.rs", "createdAt": "2026-01-01T06:00:00Z",
                "author": {"login": "bob"}
            }]}}]},
            "commits": {"nodes": []},
            "head": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": {"nodes": [{
                "__typename": "CheckRun", "name": "test", "status": "COMPLETED",
                "conclusion": "FAILURE", "detailsUrl": "https://ci/1",
                "checkSuite": {"workflowRun": {"workflow": {"name": "CI"}}}
            }]}}}}]}
        }
    }"##,
    )
    .unwrap()
}

fn search_hit() -> Value {
    json!({ "nodes": [{
        "number": 12, "title": "Add the thing", "isDraft": false,
        "updatedAt": "2026-01-02T00:00:00Z", "author": {"login": "ada"},
        "repository": {"nameWithOwner": "test-owner/test-repo"}
    }]})
}

fn checkout_with_remote() -> TestRepo {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-x"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ])
    .assert_success();
    repo
}

fn env_for(server: &MockServer) -> IsolatedProcessEnv {
    IsolatedProcessEnv::with_config(&format!("[remote]\napi_base_url = \"{}\"\n", server.uri()))
}

async fn mock_graphql(server: &MockServer) {
    // One answer serves both the branch search and the detail read.
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": { "search": search_hit(), "repository": detail_page() }
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn pr_show_json_prints_the_conversation_for_a_named_pr() {
    let repo = checkout_with_remote();
    let server = MockServer::start().await;
    mock_graphql(&server).await;

    let output = env_for(&server)
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "show", "12", "--json"])
        .output()
        .expect("run stax pr show");
    output.assert_success();

    let value: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(value["repo"], "test-owner/test-repo");
    assert_eq!(value["number"], 12);
    assert_eq!(value["state"], "open");
    assert_eq!(value["head_sha"], "abc123");
    assert_eq!(value["checks"][0]["status"], "failed");
    assert_eq!(value["reviewers"][0]["verdict"], "approved");
    let timeline = value["timeline"].as_array().unwrap();
    let comment = timeline
        .iter()
        .find(|e| e["type"] == "comment")
        .expect("a comment");
    assert_eq!(comment["actor"], "bob");
    assert_eq!(comment["body"], "please add a test");
    let inline = timeline
        .iter()
        .find(|e| e["type"] == "inline_comment")
        .expect("a line comment");
    assert_eq!(inline["path"], "src/a.rs");
}

#[tokio::test]
async fn pr_show_defaults_to_the_current_branch_and_prints_plain_text() {
    let repo = checkout_with_remote();
    let branch = repo.current_branch();
    let server = MockServer::start().await;
    mock_graphql(&server).await;
    // The branch's PR is found by owner and branch, as `stax pr` finds it.
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "url": "https://api.github.com/repos/test-owner/test-repo/pulls/12",
            "id": 12, "number": 12, "state": "open", "draft": false, "title": "Add the thing",
            "html_url": "https://github.com/test-owner/test-repo/pull/12",
            "head": { "ref": branch, "sha": "abc123", "label": format!("test-owner:{branch}") },
            "base": { "ref": "main", "sha": "bbbb" }
        }])))
        .mount(&server)
        .await;

    let output = env_for(&server)
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "show"])
        .output()
        .expect("run stax pr show");
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.starts_with("Add the thing (test-owner/test-repo#12)"),
        "{stdout}"
    );
    assert!(stdout.contains("    please add a test"), "{stdout}");
    assert!(stdout.contains("failed: CI / test"), "{stdout}");
    assert!(!stdout.contains('\u{1b}'), "no terminal control codes");

    let requests = server.received_requests().await.unwrap();
    let lookup = requests
        .iter()
        .find(|r| r.method.as_str() == "GET" && r.url.path().ends_with("/pulls"))
        .expect("a branch lookup");
    let head = lookup
        .url
        .query_pairs()
        .find(|(k, _)| k == "head")
        .map(|(_, v)| v.into_owned())
        .expect("a head filter");
    assert_eq!(head, format!("test-owner:{branch}"));
}

#[tokio::test]
async fn pr_diff_prints_the_unified_diff() {
    let repo = checkout_with_remote();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/12"))
        .respond_with(ResponseTemplate::new(200).set_body_string("diff --git a/a b/a\n+x\n"))
        .mount(&server)
        .await;

    let output = env_for(&server)
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "diff", "12"])
        .output()
        .expect("run stax pr diff");
    output.assert_success();
    assert_eq!(TestRepo::stdout(&output), "diff --git a/a b/a\n+x\n");
}

fn comment_response() -> Value {
    json!({
        "id": 1, "node_id": "IC_1",
        "url": "https://api.github.com/repos/test-owner/test-repo/issues/comments/1",
        "html_url": "https://github.com/test-owner/test-repo/pull/12#issuecomment-1",
        "issue_url": "https://api.github.com/repos/test-owner/test-repo/issues/12",
        "body": "ok",
        "user": {
            "login": "me", "id": 1, "node_id": "U", "avatar_url": "https://a/1",
            "gravatar_id": "", "url": "https://a/u", "html_url": "https://a/u",
            "followers_url": "https://a/f", "following_url": "https://a/f",
            "gists_url": "https://a/g", "starred_url": "https://a/s",
            "subscriptions_url": "https://a/s", "organizations_url": "https://a/o",
            "repos_url": "https://a/r", "events_url": "https://a/e",
            "received_events_url": "https://a/r", "type": "User", "site_admin": false
        },
        "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z"
    })
}

#[tokio::test]
async fn pr_comment_posts_the_message_argument() {
    let repo = checkout_with_remote();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/issues/12/comments"))
        .and(body_partial_json(json!({ "body": "hello agent" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(comment_response()))
        .expect(1)
        .mount(&server)
        .await;

    let output = env_for(&server)
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "comment", "12", "-m", "hello agent"])
        .output()
        .expect("run stax pr comment");
    output.assert_success();
    assert!(TestRepo::stdout(&output).contains("Commented on test-owner/test-repo#12"));
}

#[tokio::test]
async fn pr_comment_reads_a_multiline_message_from_stdin() {
    let repo = checkout_with_remote();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/issues/12/comments"))
        .and(body_partial_json(json!({ "body": "line one\nline two" })))
        .respond_with(ResponseTemplate::new(201).set_body_json(comment_response()))
        .expect(1)
        .mount(&server)
        .await;

    // Kept alive until the child exits: dropping it removes the config the child reads.
    let env = env_for(&server);
    let mut child = env
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "comment", "12", "-m", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stax pr comment");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"line one\nline two\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    output.assert_success();
}

#[tokio::test]
async fn pr_approve_posts_an_approving_review() {
    let repo = checkout_with_remote();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/pulls/12/reviews"))
        .and(body_partial_json(json!({ "event": "APPROVE" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&server)
        .await;

    let output = env_for(&server)
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .args(["pr", "approve", "12"])
        .output()
        .expect("run stax pr approve");
    output.assert_success();
    assert!(TestRepo::stdout(&output).contains("Approved test-owner/test-repo#12"));
}

#[test]
fn pr_comment_without_text_fails_before_any_request() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["pr", "comment", "owner/repo#12"]);
    output.assert_failure();
    assert!(
        TestRepo::stderr(&output).contains("-m -"),
        "{}",
        TestRepo::stderr(&output)
    );

    for text in ["   ", "-"] {
        // `-` reads standard input, which `run_stax` leaves empty.
        let output = repo.run_stax(&["pr", "comment", "owner/repo#12", "-m", text]);
        output.assert_failure();
        assert!(
            TestRepo::stderr(&output).contains("comment is empty"),
            "{text:?}: {}",
            TestRepo::stderr(&output)
        );
    }
}

/// Agent harnesses leave standard input open and never close it. Without `-m -`
/// the command must fail at once instead of waiting for input.
#[test]
fn pr_comment_does_not_wait_on_an_open_stdin() {
    let repo = TestRepo::new();
    let env = IsolatedProcessEnv::with_config("");
    let mut child = env
        .command(&repo.path())
        .args(["pr", "comment", "owner/repo#12"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn stax pr comment");
    let _open_stdin = child.stdin.take();

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("pr comment blocked reading standard input");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    };
    assert!(!status.success());
}

#[test]
fn pr_commands_reject_a_malformed_target() {
    let repo = TestRepo::new();

    for args in [
        vec!["pr", "show", "not-a-pr"],
        vec!["pr", "diff", "https://evil.example/o/r/pull/1"],
        vec!["pr", "comment", "owner#3", "-m", "hi"],
        vec!["pr", "approve", "a/b/c#3"],
    ] {
        let output = repo.run_stax(&args);
        output.assert_failure();
        assert!(
            TestRepo::stderr(&output).contains("isn't a pull request"),
            "{args:?}: {}",
            TestRepo::stderr(&output)
        );
    }
}
