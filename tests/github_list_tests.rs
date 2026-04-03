mod common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn write_test_config(home: &Path, api_base_url: &str) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("Failed to create config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{}\"\n", api_base_url),
    )
    .expect("Failed to write config");
}

fn configure_github_remote(repo: &TestRepo) {
    let output = repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test/repo.git",
    ]);
    assert!(
        output.status.success(),
        "Failed to add origin: {}",
        TestRepo::stderr(&output)
    );
}

fn setup_repo(home: &Path, api_base_url: &str) -> TestRepo {
    let repo = TestRepo::new();
    configure_github_remote(&repo);
    write_test_config(home, api_base_url);
    repo
}

fn env_with_auth<'a>(home: &'a TempDir) -> [(&'a str, &'a str); 2] {
    [
        ("HOME", home.path().to_str().unwrap()),
        ("STAX_GITHUB_TOKEN", "mock-token"),
    ]
}

#[tokio::test]
async fn test_pr_list_human_output() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .and(query_param("state", "open"))
        .and(query_param("sort", "created"))
        .and(query_param("direction", "desc"))
        .and(query_param("per_page", "30"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 114,
                "title": "worktrees enhanced",
                "html_url": "https://github.com/test/repo/pull/114",
                "user": { "login": "cesarferreira" },
                "head": { "ref": "cesar/worktrees-enhanced" },
                "base": { "ref": "main" },
                "state": "open",
                "draft": false,
                "created_at": "2026-03-15T10:00:00Z"
            },
            {
                "number": 45,
                "title": "feat: add upstream remote support for fork workflows",
                "html_url": "https://github.com/test/repo/pull/45",
                "user": { "login": "rawnam" },
                "head": { "ref": "rawnam/02-12/feat-upstream-sync-and-pr-support-for-fork-workflows" },
                "base": { "ref": "main" },
                "state": "open",
                "draft": true,
                "created_at": "2026-03-14T09:00:00Z"
            }
        ])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_success()
        .assert_stdout_contains("test/repo")
        .assert_stdout_contains("2 open pull requests")
        .assert_stdout_contains("STATE")
        .assert_stdout_contains("#114")
        .assert_stdout_contains("draft")
        .assert_stdout_contains("worktrees enhanced");
}

#[tokio::test]
async fn test_pr_list_json_output() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 112,
                "title": "fix: harden stax log tree traversal against stack overflow",
                "html_url": "https://github.com/test/repo/pull/112",
                "user": { "login": "cesarferreira" },
                "head": { "ref": "fix/log-iterative-tree-traversal" },
                "base": { "ref": "main" },
                "state": "open",
                "draft": false,
                "created_at": "2026-03-15T11:00:00Z"
            }
        ])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list", "--json"], &env_with_auth(&home));
    output.assert_success();

    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).unwrap();
    let items = json.as_array().expect("Expected PR array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["number"], 112);
    assert_eq!(items[0]["author"], "cesarferreira");
    assert_eq!(items[0]["head_branch"], "fix/log-iterative-tree-traversal");
    assert_eq!(items[0]["base_branch"], "main");
    assert_eq!(items[0]["state"], "open");
    assert_eq!(items[0]["is_draft"], false);
    assert!(items[0]["created_at"]
        .as_str()
        .unwrap()
        .contains("2026-03-15"));
}

#[tokio::test]
async fn test_issue_list_human_output() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/issues"))
        .and(query_param("state", "open"))
        .and(query_param("sort", "updated"))
        .and(query_param("direction", "desc"))
        .and(query_param("per_page", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 113,
                "title": "Handle browser launcher failures in open/pr/submit instead of swallowing them",
                "html_url": "https://github.com/test/repo/issues/113",
                "user": { "login": "cesarferreira" },
                "labels": [],
                "updated_at": "2026-03-15T12:00:00Z"
            },
            {
                "number": 77,
                "title": "Gitlab Support",
                "html_url": "https://github.com/test/repo/issues/77",
                "user": { "login": "geoHeil" },
                "labels": [{ "name": "help wanted" }],
                "updated_at": "2026-03-14T11:00:00Z"
            }
        ])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["issue", "list"], &env_with_auth(&home));
    output
        .assert_success()
        .assert_stdout_contains("test/repo")
        .assert_stdout_contains("2 open issues")
        .assert_stdout_contains("LABELS")
        .assert_stdout_contains("#77")
        .assert_stdout_contains("help wanted");
}

#[tokio::test]
async fn test_issue_list_json_output() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "number": 65,
                "title": "Allow specifying CLI parameters through environment variables",
                "html_url": "https://github.com/test/repo/issues/65",
                "user": { "login": "cesarferreira" },
                "labels": [{ "name": "cli" }, { "name": "enhancement" }],
                "updated_at": "2026-03-15T08:30:00Z"
            }
        ])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["issue", "list", "--json"], &env_with_auth(&home));
    output.assert_success();

    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).unwrap();
    let items = json.as_array().expect("Expected issue array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["number"], 65);
    assert_eq!(items[0]["author"], "cesarferreira");
    assert_eq!(items[0]["labels"][0], "cli");
    assert_eq!(items[0]["labels"][1], "enhancement");
    assert!(items[0]["updated_at"]
        .as_str()
        .unwrap()
        .contains("2026-03-15"));
}

#[tokio::test]
async fn test_pr_list_empty_state() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_success()
        .assert_stdout_contains("0 open pull requests")
        .assert_stdout_contains("No open pull requests.");
}

#[tokio::test]
async fn test_issue_list_empty_state() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["issue", "list"], &env_with_auth(&home));
    output
        .assert_success()
        .assert_stdout_contains("0 open issues")
        .assert_stdout_contains("No open issues.");
}

#[test]
fn test_pr_list_requires_auth() {
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), "https://api.github.invalid");

    let output =
        repo.run_stax_with_env(&["pr", "list"], &[("HOME", home.path().to_str().unwrap())]);
    output.assert_failure();

    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        combined.contains("GitHub auth not configured"),
        "Expected auth error, got:\n{}",
        combined
    );
}

#[test]
fn test_issue_list_requires_remote() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["issue", "list"]);
    output.assert_failure();

    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        combined.contains("No git remote 'origin' found"),
        "Expected remote error, got:\n{}",
        combined
    );
}
