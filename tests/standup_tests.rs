//! End-to-end contracts for `st standup` machine-readable output.

use crate::common::{OutputAssertions, TestRepo};
use chrono::{Duration, Utc};
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

fn configure_remote(repo: &TestRepo, url: &str) {
    repo.git(&["remote", "add", "origin", url]).assert_success();
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

#[test]
fn standup_rejects_non_positive_hours() {
    let repo = TestRepo::new();

    for hours in ["--hours=-1", "--hours=0"] {
        let output = repo.run_stax(&["standup", "--json", hours]);
        output.assert_failure();

        let stderr = TestRepo::stderr(&output);
        assert!(
            stderr.contains("value must be greater than zero"),
            "expected positive-hours validation for {hours}, got: {stderr}"
        );
    }
}

fn env_with_auth(home: &TempDir) -> [(&str, &str); 2] {
    [
        ("HOME", home.path().to_str().expect("UTF-8 home path")),
        ("STAX_GITHUB_TOKEN", "mock-token"),
    ]
}

async fn mount_populated_activity(server: &MockServer) {
    let now = Utc::now();
    let created_at = (now - Duration::hours(6)).to_rfc3339();
    let closed_at = (now - Duration::hours(5)).to_rfc3339();
    let reviewed_at = (now - Duration::hours(4)).to_rfc3339();

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
                "created_at": created_at,
                "closed_at": closed_at
            }]
        })))
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test/repo/pulls/42/reviews"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "state": "APPROVED",
                "submitted_at": reviewed_at,
                "user": { "login": "reviewer" }
            }
        ])))
        .mount(server)
        .await;

    mount_review_contributions(
        server,
        serde_json::json!([
            review_contribution(77, "Reviewed teammate change", &reviewed_at, "test/repo"),
            review_contribution(
                78,
                "Too old",
                &(now - Duration::hours(72)).to_rfc3339(),
                "test/repo"
            ),
            review_contribution(79, "Other repository", &reviewed_at, "other/repo")
        ]),
        200,
    )
    .await;
}

fn review_contribution(number: u64, title: &str, occurred_at: &str, repository: &str) -> Value {
    serde_json::json!({
        "occurredAt": occurred_at,
        "user": { "login": "test-user" },
        "pullRequestReview": {
            "state": "APPROVED",
            "author": { "login": "test-user" },
            "pullRequest": {
                "number": number,
                "title": title,
                "repository": { "nameWithOwner": repository }
            }
        }
    })
}

async fn mount_review_contributions(server: &MockServer, nodes: Value, status: u16) {
    let response = if status == 200 {
        ResponseTemplate::new(status).set_body_json(serde_json::json!({
            "data": {
                "viewer": {
                    "contributionsCollection": {
                        "pullRequestReviewContributions": { "nodes": nodes }
                    }
                }
            }
        }))
    } else {
        ResponseTemplate::new(status)
            .set_body_json(serde_json::json!({ "message": "review query failed" }))
    };

    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(response)
        .with_priority(if status == 200 { 5 } else { 1 })
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
    assert_eq!(json["signals"]["reviews_given"]["status"], "available");
    assert_eq!(json["reviews_given"].as_array().unwrap().len(), 1);
    assert_eq!(json["reviews_given"][0]["pr_number"], 77);
    assert_eq!(json["reviews_given"][0]["reviewer"], "test-user");
    assert_eq!(json["signals"]["ci_failing"]["status"], "not_requested");
    assert!(json["needs_attention"].is_object());
    assert!(
        !stdout.contains("Collecting standup context"),
        "JSON stdout must not contain progress output: {stdout}"
    );
    let graphql_requests: Vec<_> = requests
        .iter()
        .filter(|request| request.method.as_str() == "POST" && request.url.path() == "/graphql")
        .collect();
    assert_eq!(
        graphql_requests.len(),
        1,
        "reviews must use one bounded query"
    );
    let body: Value = serde_json::from_slice(&graphql_requests[0].body).expect("GraphQL JSON body");
    let query = body["query"].as_str().expect("GraphQL query string");
    assert!(query.contains("pullRequestReviewContributions(first: 100"));
    assert!(query.contains("contributionsCollection(from:"));
    assert!(query.contains("to:"));
}

#[tokio::test]
async fn standup_human_prints_reviews_and_truthful_signal_notes() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    repo.create_stack(&["standup-human"]);

    let output = repo.run_stax_with_env(&["standup", "--hours", "48"], &env_with_auth(&home));
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("You approved PR #77"),
        "human review missing: {stdout}"
    );
    assert!(stdout.contains("Signals"), "signal note missing: {stdout}");
    assert!(
        stdout.contains("--ci"),
        "CI opt-in guidance missing: {stdout}"
    );
}

#[tokio::test]
async fn standup_marks_failed_review_query_unavailable_without_losing_local_activity() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    mount_review_contributions(&server, serde_json::json!([]), 500).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    let stack = repo.create_stack(&["standup-review-error"]);

    let output = repo.run_stax_with_env(&["standup", "--json"], &env_with_auth(&home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["reviews_given"], serde_json::json!([]));
    assert_eq!(json["signals"]["reviews_given"]["status"], "unavailable");
    assert!(json["signals"]["reviews_given"]["reason"].is_string());
    assert!(
        json["recent_pushes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["branch"] == stack[0])
    );
}

#[tokio::test]
async fn standup_human_does_not_claim_no_activity_when_reviews_are_unavailable() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": 0,
            "incomplete_results": false,
            "items": []
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    mount_review_contributions(&server, serde_json::json!([]), 500).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());

    let output = repo.run_stax_with_env(&["standup"], &env_with_auth(&home));
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Signals"),
        "signal warning missing: {stdout}"
    );
    assert!(
        !stdout.contains("No activity in the last"),
        "unavailable review data must not produce an unconditional empty claim: {stdout}"
    );
}

#[tokio::test]
async fn standup_skips_null_review_contributions_and_keeps_valid_reviews() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let reviewed_at = (Utc::now() - Duration::hours(2)).to_rfc3339();
    let mut unusable = review_contribution(80, "Missing identity", &reviewed_at, "test/repo");
    unusable["user"] = Value::Null;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "viewer": {
                    "contributionsCollection": {
                        "pullRequestReviewContributions": {
                            "nodes": [
                                null,
                                unusable,
                                review_contribution(81, "Valid review", &reviewed_at, "test/repo")
                            ]
                        }
                    }
                }
            }
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    repo.create_stack(&["standup-null-contributions"]);

    let output = repo.run_stax_with_env(&["standup", "--json"], &env_with_auth(&home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["signals"]["reviews_given"]["status"], "available");
    assert_eq!(json["reviews_given"].as_array().unwrap().len(), 1);
    assert_eq!(json["reviews_given"][0]["pr_number"], 81);
    assert_eq!(json["reviews_given"][0]["reviewer"], "test-user");
}

#[tokio::test]
async fn standup_marks_successful_empty_review_query_available() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {
                "viewer": {
                    "contributionsCollection": {
                        "pullRequestReviewContributions": { "nodes": [] }
                    }
                }
            }
        })))
        .with_priority(1)
        .mount(&server)
        .await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    repo.create_stack(&["standup-empty-reviews"]);

    let output = repo.run_stax_with_env(&["standup", "--json"], &env_with_auth(&home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["reviews_given"], serde_json::json!([]));
    assert_eq!(json["signals"]["reviews_given"]["status"], "available");
}

#[tokio::test]
async fn standup_without_ci_makes_no_ci_requests() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    repo.create_stack(&["standup-no-ci"]);

    let output = repo.run_stax_with_env(&["standup", "--json"], &env_with_auth(&home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["signals"]["ci_failing"]["status"], "not_requested");
    assert!(
        json["signals"]["ci_failing"]["reason"]
            .as_str()
            .unwrap()
            .contains("--ci")
    );

    let requests = server.received_requests().await.expect("mock requests");
    assert!(
        requests.iter().all(|request| {
            let path = request.url.path();
            !path.contains("/check-runs") && !path.contains("/statuses")
        }),
        "default standup unexpectedly queried CI: {requests:?}"
    );
}

#[tokio::test]
async fn standup_ci_reports_failure_and_api_unavailability() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    let stack = repo.create_stack(&["standup-ci"]);
    let sha_output = repo.git(&["rev-parse", &stack[0]]);
    let sha = String::from_utf8(sha_output.stdout)
        .expect("UTF-8 sha")
        .trim()
        .to_string();

    Mock::given(method("GET"))
        .and(path(format!("/repos/test/repo/commits/{sha}/check-runs")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": 1,
            "check_runs": [{
                "id": 1, "name": "build", "status": "completed", "conclusion": "failure",
                "started_at": null, "completed_at": null
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/repos/test/repo/commits/{sha}/statuses")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let output = repo.run_stax_with_env(&["standup", "--json", "--ci"], &env_with_auth(&home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["signals"]["ci_failing"]["status"], "available");
    assert_eq!(
        json["needs_attention"]["ci_failing"],
        serde_json::json!([stack[0].clone()])
    );

    let human = repo.run_stax_with_env(&["standup", "--ci"], &env_with_auth(&home));
    human.assert_success();
    assert!(
        TestRepo::stdout(&human).contains(&format!("CI failing on {}", stack[0])),
        "human standup must report failing CI"
    );

    let failed = MockServer::start().await;
    mount_populated_activity(&failed).await;
    let failed_home = TempDir::new().expect("temp home");
    let failed_repo = setup_repo(&failed_home, &failed.uri());
    let failed_stack = failed_repo.create_stack(&["standup-ci-error"]);
    let failed_sha_output = failed_repo.git(&["rev-parse", &failed_stack[0]]);
    let failed_sha = String::from_utf8(failed_sha_output.stdout)
        .expect("UTF-8 sha")
        .trim()
        .to_string();
    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test/repo/commits/{failed_sha}/check-runs"
        )))
        .respond_with(ResponseTemplate::new(500))
        .mount(&failed)
        .await;
    let output =
        failed_repo.run_stax_with_env(&["standup", "--json", "--ci"], &env_with_auth(&failed_home));
    output.assert_success();
    let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
    assert_eq!(json["needs_attention"]["ci_failing"], serde_json::json!([]));
    assert_eq!(json["signals"]["ci_failing"]["status"], "unavailable");
}

#[tokio::test]
async fn standup_reports_unsupported_authored_reviews_without_provider_scan() {
    ensure_crypto_provider();

    for (remote_url, token_name, user_body, expected_provider) in [
        (
            "https://gitlab.com/test/repo.git",
            "STAX_GITLAB_TOKEN",
            serde_json::json!({ "username": "test-user" }),
            "GitLab",
        ),
        (
            "https://gitea.com/test/repo.git",
            "STAX_GITEA_TOKEN",
            serde_json::json!({ "login": "test-user" }),
            "Gitea",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(user_body))
            .with_priority(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&server)
            .await;

        let home = TempDir::new().expect("temp home");
        write_test_config(home.path(), &server.uri());
        let repo = TestRepo::new();
        configure_remote(&repo, remote_url);
        repo.create_stack(&["standup-unsupported"]);
        let env = [
            ("HOME", home.path().to_str().expect("UTF-8 home path")),
            (token_name, "mock-token"),
        ];

        let output = repo.run_stax_with_env(&["standup", "--json"], &env);
        output.assert_success();
        let json: Value = serde_json::from_str(&TestRepo::stdout(&output)).expect("valid JSON");
        assert_eq!(json["reviews_given"], serde_json::json!([]));
        assert_eq!(json["signals"]["reviews_given"]["status"], "unsupported");
        assert!(
            json["signals"]["reviews_given"]["reason"]
                .as_str()
                .unwrap()
                .contains(expected_provider)
        );

        let requests = server.received_requests().await.expect("mock requests");
        assert!(
            requests.iter().all(|request| {
                let path = request.url.path();
                !path.contains("/approvals") && !path.contains("/reviews")
            }),
            "unsupported authored reviews must not scan MR/PR review endpoints: {requests:?}"
        );
    }
}

#[tokio::test]
async fn standup_ci_scope_follows_current_stack_and_all() {
    ensure_crypto_provider();
    let server = MockServer::start().await;
    mount_populated_activity(&server).await;
    let home = TempDir::new().expect("temp home");
    let repo = setup_repo(&home, &server.uri());
    let stack_a = repo.create_stack(&["standup-ci-a"]);
    repo.git(&["checkout", "main"]).assert_success();
    let stack_b = repo.create_stack(&["standup-ci-b"]);
    repo.git(&["checkout", &stack_a[0]]).assert_success();

    let mut branch_shas = Vec::new();
    for branch in [&stack_a[0], &stack_b[0]] {
        let output = repo.git(&["rev-parse", branch]);
        let sha = String::from_utf8(output.stdout)
            .expect("UTF-8 sha")
            .trim()
            .to_string();
        for endpoint in ["check-runs", "statuses"] {
            let body = if endpoint == "check-runs" {
                serde_json::json!({ "total_count": 0, "check_runs": [] })
            } else {
                serde_json::json!([])
            };
            Mock::given(method("GET"))
                .and(path(format!("/repos/test/repo/commits/{sha}/{endpoint}")))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        branch_shas.push((branch.to_string(), sha));
    }

    let current = repo.run_stax_with_env(&["standup", "--json", "--ci"], &env_with_auth(&home));
    current.assert_success();
    let requests = server.received_requests().await.expect("current requests");
    assert!(
        requests
            .iter()
            .any(|r| r.url.path().contains(&branch_shas[0].1))
    );
    assert!(
        requests
            .iter()
            .all(|r| !r.url.path().contains(&branch_shas[1].1))
    );

    let all = repo.run_stax_with_env(
        &["standup", "--json", "--all", "--ci"],
        &env_with_auth(&home),
    );
    all.assert_success();
    let requests = server.received_requests().await.expect("all requests");
    assert!(
        requests
            .iter()
            .any(|r| r.url.path().contains(&branch_shas[0].1))
    );
    assert!(
        requests
            .iter()
            .any(|r| r.url.path().contains(&branch_shas[1].1))
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
