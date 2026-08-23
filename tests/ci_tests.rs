//! Tests for the `stax ci` command
//!
//! CI output is rendered from forge responses, so these tests drive the real
//! binary against a `wiremock` GitHub API instead of the live network. That
//! keeps assertions on the actual command contract (rendered text, JSON schema,
//! requested SHAs, scope selection) rather than on environmental failure modes.

use crate::common;
use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REMOTE_URL: &str = "https://github.com/test-owner/test-repo.git";

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// Point the repo's stax config at the mock API and give it a GitHub remote.
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

fn auth_env() -> [(&'static str, &'static str); 1] {
    [("STAX_GITHUB_TOKEN", "mock-token")]
}

fn check_run(name: &str, status: &str, conclusion: Option<&str>) -> Value {
    serde_json::json!({
        "id": 1,
        "name": name,
        "status": status,
        "conclusion": conclusion,
        "html_url": null,
        "started_at": "2026-01-01T00:00:00Z",
        "completed_at": "2026-01-01T00:01:00Z"
    })
}

/// Mount the two endpoints `fetch_checks` always hits for a SHA.
async fn mount_checks(server: &MockServer, sha: &str, runs: Vec<Value>) {
    let check_runs_path = format!("/repos/test-owner/test-repo/commits/{sha}/check-runs");
    Mock::given(method("GET"))
        .and(path(check_runs_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": runs.len(),
            "check_runs": runs,
        })))
        .mount(server)
        .await;

    let statuses_path = format!("/repos/test-owner/test-repo/commits/{sha}/statuses");
    Mock::given(method("GET"))
        .and(path(statuses_path))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(server)
        .await;
}

async fn requested_paths(server: &MockServer) -> Vec<String> {
    server
        .received_requests()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|request| request.url.path().to_string())
        .collect()
}

/// Create a tracked branch with one commit and return its name and SHA.
fn tracked_branch(repo: &TestRepo, name: &str) -> (String, String) {
    repo.run_stax(&["bc", name]).assert_success();
    let branch = repo.current_branch();
    repo.create_file(&format!("{name}.txt"), "content");
    repo.commit(&format!("Add {name}"));
    let sha = repo.get_commit_sha(&branch);
    (branch, sha)
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
// Rendering
// =============================================================================

#[tokio::test]
async fn ci_renders_passing_checks_for_the_current_branch_sha() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(
        &server,
        &sha,
        vec![
            check_run("build", "completed", Some("success")),
            check_run("lint", "completed", Some("success")),
        ],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains(&branch)
        .assert_stdout_contains(&sha[..7])
        .assert_stdout_contains("build")
        .assert_stdout_contains("lint")
        .assert_stdout_contains("passed")
        // Both checks span 2026-01-01T00:00:00Z..00:01:00Z.
        .assert_stdout_contains("1m");

    let paths = requested_paths(&server).await;
    assert!(
        paths.contains(&format!(
            "/repos/test-owner/test-repo/commits/{sha}/check-runs"
        )),
        "expected check-runs to be requested for the branch SHA, got: {paths:?}"
    );
}

#[tokio::test]
async fn ci_renders_failing_checks() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (_branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(
        &server,
        &sha,
        vec![
            check_run("build", "completed", Some("success")),
            check_run("integration", "completed", Some("failure")),
        ],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains("integration")
        .assert_stdout_contains("failed");
}

#[tokio::test]
async fn ci_reports_no_ci_when_the_forge_has_no_checks() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(&server, &sha, Vec::new()).await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains(&branch)
        .assert_stdout_contains("no CI");
}

#[tokio::test]
async fn ci_shows_pr_number_and_uses_the_pr_rollup_status() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");
    write_branch_pr_metadata(&repo, &branch, "main", 42);

    // The rollup says pending even though the check run already completed;
    // `stax ci` must prefer the PR-level rollup for the overall status.
    mount_checks(
        &server,
        &sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": { "repository": { "pullRequest": {
                "number": 42,
                "title": "Add the feature",
                "state": "OPEN",
                "updatedAt": "2026-01-01T00:00:00Z",
                "isDraft": false,
                "mergeable": "MERGEABLE",
                "reviewDecision": "APPROVED",
                "headRefOid": sha,
                "statusCheckRollup": { "state": "PENDING" },
                "reviews": { "nodes": [] }
            } } }
        })))
        .mount(&server)
        .await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains("PR #42")
        .assert_stdout_contains("pending");
}

// =============================================================================
// JSON contract
// =============================================================================

#[tokio::test]
async fn ci_json_output_matches_the_branch_status_contract() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(
        &server,
        &sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--json"], &auth_env());
    output.assert_success();

    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    let statuses = json.as_array().expect("array of branch statuses");
    assert_eq!(statuses.len(), 1);

    let status = &statuses[0];
    assert_eq!(status["branch"], branch);
    assert_eq!(status["sha"], sha);
    assert_eq!(status["sha_short"].as_str(), Some(&sha[..7]));
    assert_eq!(status["overall_status"], "success");
    assert_eq!(status["pr_number"], Value::Null);

    let checks = status["check_runs"].as_array().expect("check runs array");
    assert_eq!(checks.len(), 1);
    assert_eq!(checks[0]["name"], "build");
    assert_eq!(checks[0]["status"], "completed");
    assert_eq!(checks[0]["conclusion"], "success");
    assert_eq!(checks[0]["started_at"], "2026-01-01T00:00:00Z");
    assert_eq!(checks[0]["completed_at"], "2026-01-01T00:01:00Z");
    assert_eq!(checks[0]["elapsed_secs"], 60);
}

#[tokio::test]
async fn ci_json_output_reports_failure_status() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (_branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(
        &server,
        &sha,
        vec![check_run("build", "completed", Some("failure"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--json"], &auth_env());
    output.assert_success();

    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json[0]["overall_status"], "failure");
    assert_eq!(json[0]["check_runs"][0]["conclusion"], "failure");
}

// =============================================================================
// Scope selection
// =============================================================================

#[tokio::test]
async fn ci_default_scope_queries_only_the_current_branch() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (_parent, parent_sha) = tracked_branch(&repo, "parent");
    let (child, child_sha) = tracked_branch(&repo, "child");

    mount_checks(
        &server,
        &child_sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output.assert_success().assert_stdout_contains(&child);

    let paths = requested_paths(&server).await;
    assert!(
        !paths.iter().any(|p| p.contains(&parent_sha)),
        "default scope should not query the parent branch, got: {paths:?}"
    );
}

#[tokio::test]
async fn ci_stack_scope_covers_every_branch_in_the_current_stack() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (parent, parent_sha) = tracked_branch(&repo, "parent");
    let (child, child_sha) = tracked_branch(&repo, "child");

    mount_checks(
        &server,
        &parent_sha,
        vec![check_run("parent-build", "completed", Some("success"))],
    )
    .await;
    mount_checks(
        &server,
        &child_sha,
        vec![check_run("child-build", "completed", Some("failure"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--stack"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains("2 branches")
        .assert_stdout_contains(&parent)
        .assert_stdout_contains(&child)
        .assert_stdout_contains("1 passing")
        .assert_stdout_contains("1 failing");
}

#[tokio::test]
async fn ci_all_scope_covers_every_tracked_branch() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (first, first_sha) = tracked_branch(&repo, "first");
    repo.run_stax(&["checkout", "main"]).assert_success();
    let (second, second_sha) = tracked_branch(&repo, "second");

    mount_checks(
        &server,
        &first_sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;
    mount_checks(
        &server,
        &second_sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--all"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains("2 branches")
        .assert_stdout_contains(&first)
        .assert_stdout_contains(&second);
}

#[test]
fn ci_all_reports_when_no_branches_are_tracked() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["ci", "--all"]);
    output
        .assert_success()
        .assert_stdout_contains("No tracked branches found.");
}

// =============================================================================
// Watch mode
// =============================================================================

#[tokio::test]
async fn ci_watch_exits_immediately_when_checks_already_passed() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (_branch, sha) = tracked_branch(&repo, "feature");

    mount_checks(
        &server,
        &sha,
        vec![check_run("build", "completed", Some("success"))],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--watch", "--no-alert"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains("CI already finished — all checks passed");
}

#[tokio::test]
async fn ci_watch_strict_exits_when_a_check_has_failed() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");

    // A still-running check would normally keep watch mode polling; --strict
    // must bail out on the failure instead.
    mount_checks(
        &server,
        &sha,
        vec![
            check_run("build", "completed", Some("failure")),
            serde_json::json!({
                "id": 2,
                "name": "integration",
                "status": "in_progress",
                "conclusion": null,
                "html_url": null,
                "started_at": "2026-01-01T00:00:00Z",
                "completed_at": null
            }),
        ],
    )
    .await;

    let output = repo.run_stax_with_env(&["ci", "--watch", "--strict", "--no-alert"], &auth_env());
    output
        .assert_success()
        .assert_stdout_contains(&format!("CI already finished — failed on {branch}"));
}

// =============================================================================
// Error paths
// =============================================================================

#[tokio::test]
async fn ci_surfaces_forge_failures_with_branch_context() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    let repo = TestRepo::new();
    setup_github_repo(&repo, &server.uri());
    let (branch, sha) = tracked_branch(&repo, "feature");

    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test-owner/test-repo/commits/{sha}/check-runs"
        )))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(serde_json::json!({ "message": "Bad credentials" })),
        )
        .mount(&server)
        .await;

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_failure()
        .assert_stderr_contains(&format!("Failed to fetch CI checks for branch '{branch}'"));
}

#[test]
fn ci_requires_forge_auth() {
    let repo = TestRepo::new();
    setup_github_repo(&repo, "https://api.github.invalid");
    tracked_branch(&repo, "feature");

    let output = repo.run_stax(&["ci"]);
    output
        .assert_failure()
        .assert_stderr_contains("GitHub auth not configured");
}

#[test]
fn ci_requires_a_configured_remote() {
    let repo = TestRepo::new();
    tracked_branch(&repo, "feature");

    let output = repo.run_stax_with_env(&["ci"], &auth_env());
    output
        .assert_failure()
        .assert_stderr_contains("Could not determine remote info");
}

// =============================================================================
// Help
// =============================================================================

#[test]
fn test_ci_refresh_help_explains_live_fetch_behavior() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["ci", "--help"]);
    output
        .assert_success()
        .assert_stdout_contains("CI is always fetched live");
}
