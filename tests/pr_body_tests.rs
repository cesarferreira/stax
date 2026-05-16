//! PR body command integration tests.

mod common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use wiremock::matchers::{method, path};
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

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, pr_number: u64) {
    let metadata = serde_json::json!({
        "parentBranchName": "main",
        "parentBranchRevision": repo.get_commit_sha("main"),
        "prInfo": {
            "number": pr_number,
            "state": "OPEN"
        }
    });

    let metadata_file = tempfile::NamedTempFile::new().expect("metadata temp file");
    fs::write(metadata_file.path(), metadata.to_string()).expect("write metadata temp file");
    let hash = repo.git(&[
        "hash-object",
        "-w",
        metadata_file.path().to_str().expect("metadata path"),
    ]);
    assert!(
        hash.status.success(),
        "git hash-object failed: {}",
        TestRepo::stderr(&hash)
    );
    let blob = TestRepo::stdout(&hash);
    let update = repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{}", branch),
        blob.trim(),
    ]);
    assert!(
        update.status.success(),
        "git update-ref failed: {}",
        TestRepo::stderr(&update)
    );
}

fn pr_fixture(number: u64, branch: &str, body: &str) -> serde_json::Value {
    serde_json::json!({
        "url": format!("https://api.github.com/repos/test/repo/pulls/{}", number),
        "id": number,
        "number": number,
        "state": "open",
        "draft": false,
        "title": "Test PR",
        "body": body,
        "html_url": format!("https://github.com/test/repo/pull/{}", number),
        "head": { "ref": branch, "sha": "aaaa", "label": format!("test:{}", branch) },
        "base": { "ref": "main", "sha": "bbbb" }
    })
}

fn setup_pr_body_repo(api_base_url: &str, pr_number: u64) -> (TestRepo, String, String) {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), api_base_url);

    let remote = repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test/repo.git",
    ]);
    assert!(
        remote.status.success(),
        "failed to add remote: {}",
        TestRepo::stderr(&remote)
    );

    repo.create_stack(&["feature-body"]);
    let branch = repo.current_branch();
    write_branch_pr_metadata(&repo, &branch, pr_number);

    (repo, home, branch)
}

#[tokio::test]
async fn pr_body_prints_current_pr_description() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let (repo, _home, branch) = setup_pr_body_repo(&mock_server.uri(), 42);

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_fixture(
            42,
            &branch,
            "Summary line\n\nDetails line",
        )))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(&["pr", "body"], &[("STAX_GITHUB_TOKEN", "mock-token")]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Summary line"), "stdout was: {stdout}");
    assert!(stdout.contains("Details line"), "stdout was: {stdout}");
}

#[cfg(unix)]
#[tokio::test]
async fn pr_body_edit_updates_current_pr_description() {
    use std::os::unix::fs::PermissionsExt;

    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let (repo, home, branch) = setup_pr_body_repo(&mock_server.uri(), 43);

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls/43"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_fixture(43, &branch, "Old body")))
        .mount(&mock_server)
        .await;

    Mock::given(method("PATCH"))
        .and(path("/repos/test/repo/pulls/43"))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_fixture(
            43,
            &branch,
            "Updated body\n",
        )))
        .mount(&mock_server)
        .await;

    let editor = Path::new(&home).join("editor.sh");
    fs::write(&editor, "#!/bin/sh\nprintf 'Updated body\\n' > \"$1\"\n")
        .expect("write editor script");
    let mut permissions = fs::metadata(&editor)
        .expect("editor metadata")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&editor, permissions).expect("chmod editor");

    let output = repo.run_stax_with_env(
        &["pr", "body", "--edit"],
        &[
            ("STAX_GITHUB_TOKEN", "mock-token"),
            ("EDITOR", editor.to_str().expect("editor path")),
        ],
    );
    output.assert_success();
    assert!(
        TestRepo::stdout(&output).contains("Updated PR #43 body"),
        "stdout was: {}",
        TestRepo::stdout(&output)
    );

    let requests = mock_server.received_requests().await.unwrap();
    assert!(requests.iter().any(|request| {
        request.method.as_str() == "PATCH"
            && request.url.path() == "/repos/test/repo/pulls/43"
            && String::from_utf8_lossy(&request.body).contains("Updated body\\n")
    }));
}

#[test]
fn pr_body_no_pr_fails() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-body"]);

    let output = repo.run_stax(&["pr", "body"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No PR") || stderr.contains("submit"),
        "expected missing PR message, got: {stderr}"
    );
}
