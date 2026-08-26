use crate::common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use tempfile::TempDir;
use wiremock::matchers::{method, path, path_regex};
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

fn env_with_auth(home: &TempDir) -> [(&str, &str); 2] {
    [
        ("HOME", home.path().to_str().unwrap()),
        ("STAX_GITHUB_TOKEN", "mock-token"),
    ]
}

#[tokio::test]
async fn pr_list_surfaces_rate_limit_error() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "message": "API rate limit exceeded for xxx.xxx.xxx.xxx (but here's the good news: Authenticated requests get a higher rate limit)."
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("API rate limit exceeded");
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stderr.contains("token is expired or lacks access"),
        "Rate-limit (403) errors should not get the auth hint, got: {}",
        stderr
    );
}

#[tokio::test]
async fn pr_list_surfaces_missing_repository() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "message": "Not Found",
            "documentation_url": "https://docs.github.com/rest"
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Failed to list pull requests")
        .assert_stderr_contains("Not Found")
        .assert_stderr_contains("token is expired or lacks access")
        .assert_stderr_contains("stax auth --from-gh");
}

#[tokio::test]
async fn issue_list_surfaces_missing_repository() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/issues"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "message": "Not Found",
            "documentation_url": "https://docs.github.com/rest"
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["issue", "list"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Failed to list issues")
        .assert_stderr_contains("Not Found")
        .assert_stderr_contains("token is expired or lacks access")
        .assert_stderr_contains("stax auth --from-gh");
}

#[tokio::test]
async fn pr_list_surfaces_server_conflict() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls"))
        .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
            "message": "Conflict: repository is empty."
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Conflict: repository is empty.");
}

#[tokio::test]
async fn ci_surfaces_expired_token_instead_of_no_ci() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repos/test/repo/commits/[0-9a-f]+/check-runs$",
        ))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
            "message": "Bad credentials"
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["ci"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Failed to fetch CI checks")
        .assert_stderr_contains("token is expired or lacks access");
    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("no CI"),
        "Auth failures must not be reported as 'no CI', got stdout: {}",
        stdout
    );
}

#[tokio::test]
async fn ci_surfaces_missing_checks_permission_on_statuses_endpoint() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repos/test/repo/commits/[0-9a-f]+/check-runs$",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "total_count": 0, "check_runs": [] })),
        )
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/test/repo/commits/[0-9a-f]+/statuses$"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "message": "Resource not accessible by personal access token"
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["ci"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Failed to fetch commit statuses");
}

#[tokio::test]
async fn ci_reports_no_ci_when_repo_has_no_checks() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), &mock_server.uri());

    Mock::given(method("GET"))
        .and(path_regex(
            r"^/repos/test/repo/commits/[0-9a-f]+/check-runs$",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({ "total_count": 0, "check_runs": [] })),
        )
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(r"^/repos/test/repo/commits/[0-9a-f]+/statuses$"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["ci"], &env_with_auth(&home));
    output.assert_success().assert_stdout_contains("no CI");
}

#[tokio::test]
async fn pr_list_surfaces_unreachable_api() {
    ensure_crypto_provider();
    let home = TempDir::new().unwrap();
    let repo = setup_repo(home.path(), "http://127.0.0.1:1");

    let output = repo.run_stax_with_env(&["pr", "list"], &env_with_auth(&home));
    output
        .assert_failure()
        .assert_stderr_contains("Failed to list pull requests")
        .assert_stderr_contains("Connection refused");
}
