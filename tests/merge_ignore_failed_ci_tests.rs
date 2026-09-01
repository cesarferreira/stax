//! Integration tests for `stax merge --ignore-failed-ci`.

use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::PathBuf;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REMOTE_URL: &str = "https://github.com/test-owner/test-repo.git";

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn setup_github_repo(repo: &TestRepo, api_base_url: &str) {
    let config_dir = PathBuf::from(repo.clean_home())
        .join(".config")
        .join("stax");
    fs::create_dir_all(&config_dir).expect("config directory");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{api_base_url}\"\n"),
    )
    .expect("test config");
    repo.git(&["remote", "add", "origin", REMOTE_URL])
        .assert_success();
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

fn auth_env() -> [(&'static str, &'static str); 1] {
    [("STAX_GITHUB_TOKEN", "mock-token")]
}

/// Build the GraphQL `pullRequest` body returned by `get_pr_merge_status`.
fn merge_status_graphql(
    number: u64,
    sha: &str,
    rollup_state: &str,
    mergeable: &str,
    is_draft: bool,
    review_decision: &str,
) -> serde_json::Value {
    serde_json::json!({
        "data": { "repository": { "pullRequest": {
            "number": number,
            "title": "Test PR",
            "state": "OPEN",
            "updatedAt": "2026-09-01T10:00:00Z",
            "isDraft": is_draft,
            "mergeable": mergeable,
            "reviewDecision": review_decision,
            "headRefOid": sha,
            "statusCheckRollup": { "state": rollup_state, "contexts": { "nodes": [] } },
            "reviews": { "nodes": [
                { "state": "APPROVED", "author": { "login": "reviewer" } }
            ] }
        } } }
    })
}

/// Mount the GraphQL merge-status mock for PR `number`.
async fn mount_merge_status(
    server: &MockServer,
    number: u64,
    sha: &str,
    rollup_state: &str,
    mergeable: &str,
    is_draft: bool,
    review_decision: &str,
) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(format!(
            "pullRequest(number: {number})"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(merge_status_graphql(
                number,
                sha,
                rollup_state,
                mergeable,
                is_draft,
                review_decision,
            )),
        )
        .mount(server)
        .await;
}

/// Mount `GET /repos/.../pulls/N` — backs `is_pr_merged` (checks `merged_at`).
async fn mount_pr_get(server: &MockServer, number: u64, sha: &str) {
    let pr_body = serde_json::json!({
        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
        "id": number,
        "number": number,
        "state": "open",
        "title": "Test PR",
        "body": "",
        "draft": false,
        "merged_at": null,
        "head": { "ref": "feat", "sha": sha, "label": "test-owner:feat" },
        "base": { "ref": "main", "sha": "0000" },
        "html_url": format!("https://github.com/test-owner/test-repo/pull/{number}")
    });
    Mock::given(method("GET"))
        .and(path(format!("/repos/test-owner/test-repo/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_body))
        .mount(server)
        .await;
}

/// Mount `PUT /repos/.../pulls/N/merge` → 200 merged.
async fn mount_merge_endpoint(server: &MockServer, number: u64) {
    Mock::given(method("PUT"))
        .and(path(format!(
            "/repos/test-owner/test-repo/pulls/{number}/merge"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "sha": "cafe",
            "merged": true,
            "message": "Pull Request successfully merged"
        })))
        .mount(server)
        .await;
}

#[tokio::test]
async fn merge_without_override_stops_on_failed_ci() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["feat"]);
    let sha = repo.get_commit_sha(&branches[0]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 1);

    mount_merge_status(&server, 1, &sha, "FAILURE", "MERGEABLE", false, "APPROVED").await;
    mount_pr_get(&server, 1, &sha).await;

    let output = repo.run_stax_with_env(
        &["merge", "--no-wait", "--yes", "--no-delete", "--no-sync"],
        &auth_env(),
    );

    let stdout = TestRepo::stdout(&output);
    let combined = format!("{}{}", stdout, TestRepo::stderr(&output));

    assert!(
        combined.contains("CI failed"),
        "expected 'CI failed' in output; got: {combined}"
    );
    assert!(
        combined.contains("--ignore-failed-ci"),
        "expected --ignore-failed-ci hint in output; got: {combined}"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let had_merge = requests.iter().any(|r| {
        r.method == wiremock::http::Method::PUT
            && r.url
                .path()
                .contains("/repos/test-owner/test-repo/pulls/1/merge")
    });
    assert!(
        !had_merge,
        "merge endpoint must NOT be called when CI failed without override"
    );
}

#[tokio::test]
async fn merge_with_override_merges_failed_ci_pr() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["feat"]);
    let sha = repo.get_commit_sha(&branches[0]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 1);

    mount_merge_status(&server, 1, &sha, "FAILURE", "MERGEABLE", false, "APPROVED").await;
    mount_pr_get(&server, 1, &sha).await;
    mount_merge_endpoint(&server, 1).await;

    let output = repo.run_stax_with_env(
        &[
            "merge",
            "--no-wait",
            "--yes",
            "--no-delete",
            "--no-sync",
            "--ignore-failed-ci",
        ],
        &auth_env(),
    );

    let stdout = TestRepo::stdout(&output);
    let combined = format!("{}{}", stdout, TestRepo::stderr(&output));

    assert!(
        combined.contains("warning:"),
        "expected warning about CI failure override; got: {combined}"
    );
    assert!(
        combined.contains("--ignore-failed-ci"),
        "expected --ignore-failed-ci in warning text; got: {combined}"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let had_merge = requests.iter().any(|r| {
        r.method == wiremock::http::Method::PUT
            && r.url
                .path()
                .contains("/repos/test-owner/test-repo/pulls/1/merge")
    });
    assert!(
        had_merge,
        "merge endpoint MUST be called with --ignore-failed-ci"
    );
}

#[tokio::test]
async fn merge_with_override_still_blocks_draft() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["feat"]);
    let sha = repo.get_commit_sha(&branches[0]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 1);

    mount_merge_status(&server, 1, &sha, "FAILURE", "MERGEABLE", true, "APPROVED").await;
    mount_pr_get(&server, 1, &sha).await;

    let output = repo.run_stax_with_env(
        &[
            "merge",
            "--no-wait",
            "--yes",
            "--no-delete",
            "--no-sync",
            "--ignore-failed-ci",
        ],
        &auth_env(),
    );

    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));

    assert!(
        combined.contains("Draft"),
        "expected Draft block message; got: {combined}"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let had_merge = requests.iter().any(|r| {
        r.method == wiremock::http::Method::PUT
            && r.url
                .path()
                .contains("/repos/test-owner/test-repo/pulls/1/merge")
    });
    assert!(
        !had_merge,
        "merge endpoint must NOT be called for a draft PR"
    );
}

#[tokio::test]
async fn merge_with_override_still_blocks_changes_requested() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["feat"]);
    let sha = repo.get_commit_sha(&branches[0]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 1);

    mount_merge_status(
        &server,
        1,
        &sha,
        "FAILURE",
        "MERGEABLE",
        false,
        "CHANGES_REQUESTED",
    )
    .await;
    mount_pr_get(&server, 1, &sha).await;

    let output = repo.run_stax_with_env(
        &[
            "merge",
            "--no-wait",
            "--yes",
            "--no-delete",
            "--no-sync",
            "--ignore-failed-ci",
        ],
        &auth_env(),
    );

    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));

    assert!(
        combined.contains("Changes requested"),
        "expected 'Changes requested' block; got: {combined}"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let had_merge = requests.iter().any(|r| {
        r.method == wiremock::http::Method::PUT
            && r.url
                .path()
                .contains("/repos/test-owner/test-repo/pulls/1/merge")
    });
    assert!(
        !had_merge,
        "merge endpoint must NOT be called when changes requested"
    );
}

#[tokio::test]
async fn merge_with_override_still_blocks_conflicts() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["feat"]);
    let sha = repo.get_commit_sha(&branches[0]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 1);

    mount_merge_status(
        &server,
        1,
        &sha,
        "FAILURE",
        "CONFLICTING",
        false,
        "APPROVED",
    )
    .await;
    mount_pr_get(&server, 1, &sha).await;

    let output = repo.run_stax_with_env(
        &[
            "merge",
            "--no-wait",
            "--yes",
            "--no-delete",
            "--no-sync",
            "--ignore-failed-ci",
        ],
        &auth_env(),
    );

    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));

    assert!(
        combined.contains("Has conflicts"),
        "expected 'Has conflicts' block (not 'CI failed'); got: {combined}"
    );
    assert!(
        !combined.contains("CI failed"),
        "must not report 'CI failed' when conflict is the actual blocker; got: {combined}"
    );

    let requests = server.received_requests().await.unwrap_or_default();
    let had_merge = requests.iter().any(|r| {
        r.method == wiremock::http::Method::PUT
            && r.url
                .path()
                .contains("/repos/test-owner/test-repo/pulls/1/merge")
    });
    assert!(
        !had_merge,
        "merge endpoint must NOT be called for a conflicting PR"
    );
}
