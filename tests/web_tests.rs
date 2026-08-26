//! Integration tests for the `st web` command and web server API.

use crate::common;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

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

        // Reference-faithful three-column workspace chrome
        assert!(
            body.contains("status-bar"),
            "workspace should include status bar"
        );
        assert!(
            body.contains(r#"class="stage""#),
            "workspace should include stage grid (class=\"stage\")"
        );
        assert!(
            body.contains("topbar-actions"),
            "workspace should include topbar action group"
        );
        assert!(
            body.contains("branch-cards"),
            "workspace should include branch cards"
        );
        assert!(
            body.contains("review-header"),
            "workspace should include review header"
        );
        assert!(
            body.contains("quick-actions"),
            "workspace should include quick actions"
        );
        assert!(
            body.contains(r#"data-lane-count="1""#)
                && body.contains(r#"style="--stack-rail-w:240px""#),
            "a linear stack should keep the reference 240px width"
        );
        assert!(
            body.contains(r#"class="review-tab active""#),
            "workspace should render Changes as the active initial tab"
        );

        // Changes is the only active tab — no Commits or Stack preview tab elements
        assert!(
            !body.contains(r#"<li>Commits</li>"#) && !body.contains(r#"<li>Stack preview</li>"#),
            "workspace should not render inactive Commits or Stack preview tabs"
        );

        // Theme options must be present
        assert!(
            body.contains(r#"value="system""#),
            "System theme option should be present"
        );
        assert!(
            body.contains(r#"value="light""#),
            "Light theme option should be present"
        );
        assert!(
            body.contains(r#"value="dark""#),
            "Dark theme option should be present"
        );
        assert!(
            body.contains(r#"id="workspace-csrf""#),
            "workspace should expose one scoped CSRF input for HTMX controls"
        );
        assert!(
            body.contains(r##"hx-include="#workspace-csrf""##),
            "HTMX controls should include only the scoped workspace CSRF input"
        );
        assert!(
            !body.contains(r#"hx-include="[name='csrf']""#),
            "broad CSRF selectors submit duplicate fields and break form extraction"
        );

        // Keep repo alive until assertions complete.
        drop(repo);
    });
}

#[test]
fn web_stack_fragment_grows_for_multiple_topology_lanes() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(init_out.status.success());

        let first = repo.run_stax(&["bc", "feat/a"]);
        assert!(
            first.status.success(),
            "first branch create failed: {}",
            common::TestRepo::stderr(&first)
        );
        repo.create_file("a.txt", "a\n");
        repo.commit("add a");

        let checkout = repo.git(&["checkout", "main"]);
        assert!(
            checkout.status.success(),
            "main checkout failed: {}",
            common::TestRepo::stderr(&checkout)
        );

        let second = repo.run_stax(&["bc", "feat/b"]);
        assert!(
            second.status.success(),
            "second branch create failed: {}",
            common::TestRepo::stderr(&second)
        );
        repo.create_file("b.txt", "b\n");
        repo.commit("add b");

        let server = stax::web::start_test_server(repo.path().to_path_buf())
            .await
            .expect("server should start");
        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");
        let token = parts.get(4).copied().unwrap_or("unknown");

        let fragment = reqwest::Client::new()
            .get(format!("http://{host}/s/{token}/stack"))
            .send()
            .await
            .unwrap();
        assert_eq!(fragment.status().as_u16(), 200);
        let body = fragment.text().await.unwrap();
        assert!(
            body.contains(r#"data-lane-count="2""#)
                && body.contains(r#"style="--stack-rail-w:260px""#),
            "two topology lanes should grow the HTMX stack fragment to 260px: {body}"
        );
    });
}

#[test]
fn web_diff_shows_file_nav_and_gutter_for_committed_change() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(
            init_out.status.success(),
            "stax init failed: {}",
            common::TestRepo::stderr(&init_out)
        );

        // Create a feature branch (bc creates and checks out)
        let create_out = repo.run_stax(&["bc", "feat/real-change"]);
        assert!(
            create_out.status.success(),
            "branch create failed: {}",
            common::TestRepo::stderr(&create_out)
        );

        // Add a file and commit on the feature branch
        repo.create_file("added.txt", "line one\nline two\nline three\n");
        repo.commit("add added.txt");

        // The server auto-selects the current branch (feat/real-change)
        let repo_path = repo.path().to_path_buf();
        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");
        let token = parts.get(4).copied().unwrap_or("unknown");

        let client = reqwest::Client::new();
        let diff_url = format!("http://{host}/s/{token}/diff");
        let diff_resp = client.get(&diff_url).send().await.unwrap();
        assert_eq!(
            diff_resp.status().as_u16(),
            200,
            "diff endpoint should return 200"
        );
        let diff_body = diff_resp.text().await.unwrap();

        assert!(
            diff_body.contains("file-nav"),
            "diff should include file-nav: {diff_body}"
        );
        assert!(
            diff_body.contains("diff-gutter"),
            "diff should include diff-gutter: {diff_body}"
        );
        assert!(
            diff_body.contains("data-file-name"),
            "diff should include data-file-name: {diff_body}"
        );
        assert!(
            diff_body.contains("review-header"),
            "diff should include review-header OOB: {diff_body}"
        );
        assert!(
            diff_body.contains("hx-swap-oob"),
            "diff should include OOB swap for review-header: {diff_body}"
        );

        // Active Changes tab must carry the exact class "review-tab active".
        assert!(
            diff_body.contains(r#"class="review-tab active""#),
            "diff review-header should include class=\"review-tab active\": {diff_body}"
        );

        // Commit count must show the exact singular form "1 commit" (test made
        // exactly one commit; plural "1 commits" must not appear).
        assert!(
            diff_body.contains("1 commit") && !diff_body.contains("1 commits"),
            "diff review-header should show '1 commit' (singular): {diff_body}"
        );

        // File navigator must use ordinal anchors (integer data-diff-file).
        assert!(
            diff_body.contains(r#"data-diff-file="0""#),
            "file navigator must use ordinal anchor 0: {diff_body}"
        );

        drop(repo);
    });
}

#[test]
fn web_diff_empty_state_when_branch_matches_parent() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(init_out.status.success());

        let create_out = repo.run_stax(&["bc", "feat/a"]);
        assert!(
            create_out.status.success(),
            "branch create failed: {}",
            common::TestRepo::stderr(&create_out)
        );

        let repo_path = repo.path().to_path_buf();
        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");

        let parts: Vec<&str> = server.base_url.split('/').collect();
        let host = parts.get(2).copied().unwrap_or("127.0.0.1");
        let token = parts.get(4).copied().unwrap_or("unknown");

        let client = reqwest::Client::new();
        let diff_url = format!("http://{host}/s/{token}/diff");
        let diff_resp = client.get(&diff_url).send().await.unwrap();
        assert_eq!(diff_resp.status().as_u16(), 200);
        let diff_body = diff_resp.text().await.unwrap();
        assert!(
            diff_body.contains("No changes vs parent"),
            "empty diff should show explicit empty state: {diff_body}"
        );
    });
}

#[test]
fn web_server_bind_falls_back_when_requested_port_is_busy() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(init_out.status.success());

        let busy_listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("busy-port guard should bind");
        let busy_port = busy_listener
            .local_addr()
            .expect("busy-port guard should have an address")
            .port();

        let shared = stax::web::session::make_shared(stax::web::session::WebSession::new(
            repo.path().to_path_buf(),
            stax::web::session::generate_token(),
            stax::web::session::generate_token(),
        ));
        let bound = stax::web::server::bind(busy_port, shared)
            .await
            .expect("busy requested port should fall back");

        assert_eq!(bound.requested_port, busy_port);
        assert!(bound.fell_back, "busy requested port should be reported");
        assert_ne!(bound.addr.port(), busy_port);
        assert_ne!(bound.addr.port(), 0);

        let response = reqwest::Client::new()
            .get(&bound.url)
            .send()
            .await
            .expect("fallback server should be reachable");
        assert_eq!(response.status().as_u16(), 200);

        bound.join_handle.abort();
        drop(busy_listener);
    });
}

#[test]
fn web_server_bind_reports_no_fallback_for_ephemeral_port() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let shared = stax::web::session::make_shared(stax::web::session::WebSession::new(
            repo.path().to_path_buf(),
            stax::web::session::generate_token(),
            stax::web::session::generate_token(),
        ));

        let bound = stax::web::server::bind(0, shared)
            .await
            .expect("ephemeral bind should succeed");

        assert_eq!(bound.requested_port, 0);
        assert!(!bound.fell_back);
        assert_ne!(bound.addr.port(), 0);

        bound.join_handle.abort();
    });
}

#[test]
fn web_command_reports_busy_port_fallback_and_startup_progress() {
    let repo = common::TestRepo::new();
    let init_out = repo.run_stax(&["init", "--trunk", "main"]);
    assert!(init_out.status.success());

    let busy_listener =
        std::net::TcpListener::bind(("127.0.0.1", 0)).expect("busy-port guard should bind");
    let busy_port = busy_listener
        .local_addr()
        .expect("busy-port guard should have an address")
        .port();

    let mut child = web_command(&repo.path())
        .args(["web", "--port", &busy_port.to_string(), "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("st web should start");
    let stdout = child.stdout.take().expect("stdout should be piped");
    let (line_tx, line_rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line_tx.send(line.unwrap_or_default()).is_err() {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut output = String::new();
    loop {
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            let _ = child.kill();
            let _ = child.wait();
            let _ = reader.join();
            panic!("timed out waiting for startup output:\n{output}");
        };

        match line_rx.recv_timeout(remaining) {
            Ok(line) => {
                output.push_str(&line);
                output.push('\n');
                if line.contains("Press Ctrl-C to stop.") {
                    break;
                }
            }
            Err(error) => {
                let _ = child.kill();
                let status = child.wait().ok();
                let _ = reader.join();
                panic!(
                    "startup output ended before readiness ({error}; status {status:?}):\n{output}"
                );
            }
        }
    }

    child.kill().expect("web server should stop");
    child.wait().expect("web server should be reaped");
    reader.join().expect("stdout reader should finish");
    drop(busy_listener);

    assert!(output.contains("Opening repository..."));
    assert!(output.contains("Port busy, using free port"));
    assert!(output.contains(&format!(":{busy_port} → :")));
    assert!(output.contains("Server started"));
    assert!(output.contains("Browser"));
    assert!(output.contains("skipped (--no-open)"));
    assert!(output.contains("Workspace  http://127.0.0.1:"));
    assert!(output.contains("Press Ctrl-C to stop."));
}

#[test]
fn web_select_branch_emits_pane_refresh_trigger() {
    async_test!(async {
        ensure_crypto_provider();
        let repo = common::TestRepo::new();
        let init_out = repo.run_stax(&["init", "--trunk", "main"]);
        assert!(init_out.status.success());

        let repo_path = repo.path().to_path_buf();
        let server = stax::web::start_test_server(repo_path)
            .await
            .expect("server should start");
        let client = reqwest::Client::new();

        let body = client
            .get(&server.base_url)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();

        assert!(
            body.contains("stax:branch-selected from:body"),
            "changes + inspector panes should re-fetch on branch selection"
        );
        assert_eq!(
            body.matches("stax:branch-selected from:body").count(),
            2,
            "both #changes-pane and #inspector-pane should listen"
        );
        assert!(
            body.contains(r#"hx-sync="this:replace""#),
            "hx-sync prevents a stale 30s poll from overwriting a fresh selection"
        );

        let csrf = body
            .split(r#"id="workspace-csrf""#)
            .nth(1)
            .and_then(|s| s.split(r#"value=""#).nth(1))
            .and_then(|s| s.split('"').next())
            .expect("csrf token in workspace HTML")
            .to_string();

        let body = format!("branch=main&csrf={csrf}");
        let resp = client
            .post(format!("{}select", server.base_url))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        assert_eq!(
            resp.headers()
                .get("hx-trigger")
                .and_then(|v| v.to_str().ok()),
            Some("stax:branch-selected"),
            "/select must tell the changes + inspector panes to refresh"
        );
    });
}
