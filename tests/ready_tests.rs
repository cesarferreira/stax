//! Tests for the `stax ready` command

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

fn merge_status_body(number: u64, title: &str, state: &str, sha: &str) -> serde_json::Value {
    serde_json::json!({
        "data": { "repository": { "pullRequest": {
            "number": number,
            "title": title,
            "state": state,
            "updatedAt": "2026-08-25T10:00:00Z",
            "isDraft": false,
            "mergeable": "MERGEABLE",
            "reviewDecision": "APPROVED",
            "headRefOid": sha,
            "statusCheckRollup": { "state": "SUCCESS", "contexts": { "nodes": [] } },
            "reviews": { "nodes": [
                { "state": "APPROVED", "author": { "login": "reviewer" } }
            ] }
        } } }
    })
}

async fn mount_merge_status(server: &MockServer, number: u64, title: &str, state: &str, sha: &str) {
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(body_string_contains(format!(
            "pullRequest(number: {number})"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(merge_status_body(number, title, state, sha)),
        )
        .mount(server)
        .await;
}

async fn mount_empty_checks(server: &MockServer, sha: &str) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test-owner/test-repo/commits/{sha}/check-runs"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": 0,
            "check_runs": []
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test-owner/test-repo/commits/{sha}/statuses"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(server)
        .await;
}

fn auth_env() -> [(&'static str, &'static str); 1] {
    [("STAX_GITHUB_TOKEN", "mock-token")]
}

/// Help text exposes the interactive readiness view and all expected flags.
#[test]
fn test_ready_help_mentions_readiness_view() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--help"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("PR readiness")
            || stdout.contains("readiness")
            || stdout.contains("tracked branches"),
        "help should mention PR readiness; got: {stdout}"
    );
    assert!(stdout.contains("--all"), "missing --all");
    assert!(stdout.contains("--current"), "missing --current");
    assert!(stdout.contains("--stack"), "missing --stack");
    assert!(stdout.contains("--json"), "missing --json");
    assert!(stdout.contains("--plain"), "missing --plain");
    assert!(stdout.contains("--interval"), "missing --interval");
}

/// `--plain` with tracked branches must reach the static readiness path and fail
/// with the readiness auth guard, not enter the CI watch loop.
#[test]
fn test_ready_plain_reaches_static_readiness() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.create_stack(&["feat-a", "feat-b"]);
    let output = repo.run_stax(&["ready", "--plain"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{stdout}{stderr}");

    assert!(
        !combined.contains("\x1B[2J"),
        "output must not contain ANSI clear-screen escape in plain mode"
    );

    assert!(
        !output.status.success(),
        "expected non-zero exit when forge token is absent"
    );
    assert!(
        stderr.contains("live PR readiness cannot be fetched")
            || stderr.contains("token")
            || stderr.contains("forge")
            || stderr.contains("auth"),
        "expected readiness auth error message; got: {stderr}"
    );
}

/// `--json` must exit non-zero with a configuration error (missing remote,
/// auth, or similar). This verifies the JSON path short-circuits before the
/// TUI and hits the readiness schema guard.
#[test]
fn test_ready_json_no_config_exits_nonzero() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--json"]);
    assert!(
        !output.status.success(),
        "expected non-zero exit when forge is not configured"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("auth")
            || stderr.contains("token")
            || stderr.contains("configured")
            || stderr.contains("remote")
            || stderr.contains("Remote")
            || stderr.contains("live PR readiness cannot be fetched"),
        "expected a configuration error; got: {stderr}"
    );
}

/// `--all` and `--current` are declared `conflicts_with` each other; clap
/// should reject this combination with exit code 2.
#[test]
fn test_ready_all_and_current_conflict() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["ready", "--all", "--current"]);
    let code = output.status.code().unwrap_or(0);
    assert_eq!(code, 2, "expected clap conflict error (exit 2)");
}

/// `st pr list --ready --help` still advertises `--ready`.
#[test]
fn test_pr_list_ready_help_present() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["pr", "list", "--help"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("--ready"), "pr list --help missing --ready");
}

/// `st ready --current` and `st ready --stack` with tracked branches reach
/// the static readiness path — clap accepts them and they fail at auth, not at
/// parse (exit 2 would indicate clap rejected the flags).
#[test]
fn test_ready_current_and_stack_flags_reach_readiness() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.create_stack(&["feat-a", "feat-b"]);

    let output_current = repo.run_stax(&["ready", "--current", "--plain"]);
    let output_stack = repo.run_stax(&["ready", "--stack", "--plain"]);

    assert_ne!(
        output_current.status.code(),
        Some(2),
        "--current should be accepted by clap and reach readiness path"
    );
    assert_ne!(
        output_stack.status.code(),
        Some(2),
        "--stack should be accepted by clap and reach readiness path"
    );

    let stderr_current = TestRepo::stderr(&output_current);
    let stderr_stack = TestRepo::stderr(&output_stack);
    assert!(
        stderr_current.contains("live PR readiness cannot be fetched")
            || stderr_current.contains("token")
            || stderr_current.contains("forge")
            || stderr_current.contains("auth"),
        "--current must reach readiness auth guard"
    );
    assert!(
        stderr_stack.contains("live PR readiness cannot be fetched")
            || stderr_stack.contains("token")
            || stderr_stack.contains("forge")
            || stderr_stack.contains("auth"),
        "--stack must reach readiness auth guard"
    );
}

#[tokio::test]
async fn test_ready_json_omits_merged_pr_and_skips_its_ci_requests() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["merged-feature", "open-feature"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 11);
    let merged_sha = repo.get_commit_sha(&branches[0]);
    let open_sha = repo.get_commit_sha(&branches[1]);

    mount_merge_status(&server, 10, "Already merged", "MERGED", &merged_sha).await;
    mount_merge_status(&server, 11, "Still open", "OPEN", &open_sha).await;
    mount_empty_checks(&server, &open_sha).await;

    let output = repo.run_stax_with_env(&["ready", "--json"], &auth_env());
    output.assert_success();
    let rows: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("readiness JSON");
    let rows = rows.as_array().expect("readiness rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["branch"], branches[1]);
    assert_eq!(rows[0]["pr_number"], 11);

    let requests = server.received_requests().await.unwrap_or_default();
    let paths = requests
        .iter()
        .map(|request| request.url.path())
        .collect::<Vec<_>>();
    assert!(
        paths.contains(
            &format!("/repos/test-owner/test-repo/commits/{open_sha}/check-runs").as_str()
        )
    );
    assert!(
        !paths
            .iter()
            .any(|request_path| request_path.contains(&merged_sha))
    );

    let metadata = repo.git(&["show", &format!("refs/branch-metadata/{}", branches[0])]);
    metadata.assert_success();
    let metadata: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&metadata)).expect("merged metadata");
    assert_eq!(metadata["prInfo"]["state"], "merged");
}

#[tokio::test]
async fn test_ready_json_keeps_closed_unmerged_pr_as_fix_candidate() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["closed-feature"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 20);
    let sha = repo.get_commit_sha(&branches[0]);

    mount_merge_status(&server, 20, "Closed without merge", "CLOSED", &sha).await;
    mount_empty_checks(&server, &sha).await;

    let output = repo.run_stax_with_env(&["ready", "--json"], &auth_env());
    output.assert_success();
    let rows: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("readiness JSON");
    assert_eq!(rows[0]["action"], "fix");
    assert_eq!(rows[0]["reason"], "closed");
}

#[tokio::test]
async fn test_ready_json_status_failure_is_not_treated_as_merged() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let branches = repo.create_stack(&["status-error"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 30);

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let output = repo.run_stax_with_env(&["ready", "--json"], &auth_env());
    assert!(!output.status.success());
    assert!(
        TestRepo::stderr(&output).contains("Failed to fetch live readiness for PR #30"),
        "unexpected error: {}",
        TestRepo::stderr(&output)
    );
}

#[tokio::test]
async fn test_ready_json_skips_branch_with_no_pr() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    repo.create_stack(&["no-pr-feature"]);

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let output = repo.run_stax_with_env(&["ready", "--json"], &auth_env());
    output.assert_success();
    let rows: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&output)).expect("readiness JSON");
    assert_eq!(rows, serde_json::json!([]));

    let requests = server.received_requests().await.unwrap_or_default();
    assert!(
        requests
            .iter()
            .all(|request| request.url.path() != "/graphql")
    );
}
