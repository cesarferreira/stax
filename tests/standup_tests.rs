//! End-to-end contracts for `st standup` machine-readable output.

use crate::common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn configure_github_remote(repo: &TestRepo) {
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test/repo.git",
    ])
    .assert_success();
}

fn write_test_config(home: &Path, api_base_url: &str) {
    let config_dir = home.join(".config/stax");
    fs::create_dir_all(&config_dir).expect("create Stax config directory");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{api_base_url}\"\n"),
    )
    .expect("write Stax config");
}

fn setup_repo(home: &TempDir, api_base_url: &str) -> TestRepo {
    let repo = TestRepo::new();
    configure_github_remote(&repo);
    write_test_config(home.path(), api_base_url);
    repo
}

fn env_with_auth(home: &TempDir) -> [(&str, &str); 2] {
    [
        ("HOME", home.path().to_str().expect("UTF-8 home path")),
        ("STAX_GITHUB_TOKEN", "mock-token"),
    ]
}

async fn mount_populated_activity(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": "test-user",
            "id": 1,
            "node_id": "MDQ6VXNlcjE=",
            "avatar_url": "https://avatars.example.test/u/1",
            "gravatar_id": "",
            "url": "https://api.github.com/users/test-user",
            "html_url": "https://github.com/test-user",
            "followers_url": "https://api.github.com/users/test-user/followers",
            "following_url": "https://api.github.com/users/test-user/following{/other_user}",
            "gists_url": "https://api.github.com/users/test-user/gists{/gist_id}",
            "starred_url": "https://api.github.com/users/test-user/starred{/owner}{/repo}",
            "subscriptions_url": "https://api.github.com/users/test-user/subscriptions",
            "organizations_url": "https://api.github.com/users/test-user/orgs",
            "repos_url": "https://api.github.com/users/test-user/repos",
            "events_url": "https://api.github.com/users/test-user/events{/privacy}",
            "received_events_url": "https://api.github.com/users/test-user/received_events",
            "type": "User",
            "site_admin": false,
            "name": null,
            "company": null,
            "blog": "",
            "location": null,
            "email": null,
            "hireable": null,
            "bio": null,
            "twitter_username": null,
            "public_repos": 1,
            "public_gists": 0,
            "followers": 0,
            "following": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z"
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": 1,
            "incomplete_results": false,
            "items": [{
                "number": 42,
                "title": "Mocked standup activity",
                "html_url": "https://github.com/test/repo/pull/42",
                "created_at": "2026-08-08T12:00:00Z",
                "closed_at": "2026-08-08T13:00:00Z"
            }]
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls/42/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "state": "APPROVED",
                "submitted_at": "2026-08-08T14:00:00Z",
                "user": { "login": "reviewer" }
            }
        ])))
        .mount(server)
        .await;
}

#[tokio::test]
async fn standup_json_is_machine_readable_and_contains_mocked_forge_activity() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    let stack = repo.create_stack(&["standup-json"]);

    let output = repo.run_stax_with_env(
        &["standup", "--json", "--hours", "48"],
        &env_with_auth(&home),
    );
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    let requests = server
        .received_requests()
        .await
        .expect("inspect mock requests");
    let json: Value = serde_json::from_str(&stdout).expect("stdout must be a single JSON document");
    assert_eq!(json["period_hours"], 48);
    assert_eq!(json["current_branch"], stack[0]);
    assert_eq!(
        json["merged_prs"][0]["number"], 42,
        "expected mocked forge activity in output: {stdout}; requests: {requests:?}"
    );
    assert_eq!(json["opened_prs"][0]["title"], "Mocked standup activity");
    assert_eq!(json["reviews_received"][0]["reviewer"], "reviewer");
    assert!(json["needs_attention"].is_object());
    assert!(
        !stdout.contains("Collecting standup context"),
        "JSON stdout must not contain progress output: {stdout}"
    );
}

#[tokio::test]
async fn standup_json_limits_pushes_to_current_stack_unless_all_is_requested() {
    let repo = TestRepo::new();
    let stack_a = repo.create_stack(&["standup-a"]);
    repo.git(&["checkout", "main"]).assert_success();
    let stack_b = repo.create_stack(&["standup-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let current_output = repo.run_stax(&["standup", "--json"]);
    current_output.assert_success();
    let current: Value =
        serde_json::from_str(&TestRepo::stdout(&current_output)).expect("current JSON");
    let current_pushes = current["recent_pushes"].as_array().expect("push array");
    assert!(
        current_pushes
            .iter()
            .any(|push| push["branch"] == stack_a[0])
    );
    assert!(
        current_pushes
            .iter()
            .all(|push| push["branch"] != stack_b[0])
    );

    let all_output = repo.run_stax(&["standup", "--json", "--all"]);
    all_output.assert_success();
    let all: Value = serde_json::from_str(&TestRepo::stdout(&all_output)).expect("all JSON");
    let all_pushes = all["recent_pushes"].as_array().expect("push array");
    assert!(all_pushes.iter().any(|push| push["branch"] == stack_a[0]));
    assert!(all_pushes.iter().any(|push| push["branch"] == stack_b[0]));
}

#[tokio::test]
async fn standup_json_gracefully_degrades_when_forge_activity_is_unavailable() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    repo.create_stack(&["standup-unavailable"]);

    let output = repo.run_stax_with_env(&["standup", "--json"], &env_with_auth(&home));
    output.assert_success();

    let json: Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON after forge error");
    assert_eq!(json["merged_prs"], serde_json::json!([]));
    assert_eq!(json["opened_prs"], serde_json::json!([]));
    assert_eq!(json["reviews_received"], serde_json::json!([]));
    assert_eq!(json["reviews_given"], serde_json::json!([]));
    assert!(json["recent_pushes"].as_array().is_some());
}
