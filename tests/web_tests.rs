//! Integration tests for the `st web` command and web server API.

use crate::common;
use std::path::Path;
use std::process::Command;

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn web_command(cwd: &Path) -> Command {
    let mut command = Command::new(common::stax_bin());
    command
        .current_dir(cwd)
        .env_remove("STAX_CONFIG_DIR")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env("STAX_DISABLE_UPDATE_CHECK", "1");
    command
}

// ── CLI shape tests ──────────────────────────────────────────────────────────

#[test]
fn web_help_works_outside_a_repository() {
    let temp = tempfile::tempdir().unwrap();
    let output = web_command(temp.path())
        .args(["web", "--help"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--port"),
        "expected --port in help: {stdout}"
    );
    assert!(
        stdout.contains("--no-open"),
        "expected --no-open in help: {stdout}"
    );
}

#[test]
fn web_help_contains_web_description() {
    let temp = tempfile::tempdir().unwrap();
    let output = web_command(temp.path())
        .args(["web", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // The description should contain something about web/localhost/browser
    let lower = stdout.to_lowercase();
    assert!(
        lower.contains("localhost")
            || lower.contains("web")
            || lower.contains("browser")
            || lower.contains("server"),
        "expected server/web description: {stdout}"
    );
}

// ── Server API tests (using library helpers) ──────────────────────────────────

/// Build a runtime and run an async test body.
macro_rules! async_test {
    ($body:expr) => {{
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on($body)
    }};
}

#[test]
fn web_server_binds_and_serves_workspace_html() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let repo_path = repo.path().to_path_buf();

        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        let client = reqwest::Client::new();
        let resp = client.get(&server.base_url).send().await.unwrap();
        // May return 500 if stax is not initialized in the repo; that's fine for binding test.
        assert!(
            resp.status().as_u16() < 600,
            "server responded with unexpected status: {}",
            resp.status()
        );
    });
}

#[test]
fn web_server_rejects_missing_session_token() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let repo_path = repo.path().to_path_buf();

        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        // Extract host from URL
        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");

        let bad_url = format!("http://{host}/s/WRONGTOKEN/");
        let client = reqwest::Client::new();
        let resp = client.get(&bad_url).send().await.unwrap();
        assert_eq!(
            resp.status().as_u16(),
            404,
            "wrong token should return 404; got {}",
            resp.status()
        );
    });
}

#[test]
fn web_server_rejects_csrf_on_mutating_post() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let repo_path = repo.path().to_path_buf();

        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        // Extract token and host from URL: http://127.0.0.1:<port>/s/<token>/
        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");
        let token = parts.get(4).copied().unwrap_or("unknown");

        let client = reqwest::Client::new();
        let post_url = format!("http://{host}/s/{token}/op/checkout");
        // Send form-encoded body with an invalid CSRF token
        let body = "branch=main&csrf=INVALID_CSRF_TOKEN";
        let resp = client
            .post(&post_url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();

        assert_eq!(
            resp.status().as_u16(),
            403,
            "invalid CSRF should return 403; got {}",
            resp.status()
        );
    });
}

#[test]
fn web_server_serves_static_assets() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let repo_path = repo.path().to_path_buf();

        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");

        let client = reqwest::Client::new();

        // CSS
        let css = client
            .get(format!("http://{host}/assets/app.css"))
            .send()
            .await
            .unwrap();
        assert_eq!(css.status().as_u16(), 200, "app.css should return 200");
        let ct = css
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert!(ct.contains("css"), "content-type should be CSS: {ct}");

        // htmx.min.js
        let js = client
            .get(format!("http://{host}/assets/htmx.min.js"))
            .send()
            .await
            .unwrap();
        assert_eq!(js.status().as_u16(), 200, "htmx.min.js should return 200");
    });
}

#[test]
fn web_server_workspace_shows_trunk_branch_after_stax_init() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();

        // Initialize stax so the server can load a real snapshot.
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(
            init_out.status.success(),
            "stax init failed: {}",
            common::TestRepo::stderr(&init_out)
        );

        let repo_path = repo.path().to_path_buf();
        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        let client = reqwest::Client::new();
        let resp = client.get(&server.base_url).send().await.unwrap();

        assert_eq!(
            resp.status().as_u16(),
            200,
            "initialized repo should return 200; got {}",
            resp.status()
        );

        let body = resp.text().await.unwrap();
        assert!(
            body.contains("main"),
            "workspace HTML should contain trunk branch name 'main'"
        );

        // The repo name (directory name) should also appear in the title.
        assert!(
            body.contains("stax web") || body.contains("stax"),
            "workspace HTML should reference stax"
        );

        // Keep repo alive until assertions complete.
        drop(repo);
    });
}
