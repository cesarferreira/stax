//! Shared GitHub / WireMock fixtures for integration tests.
//!
//! Individual suites used to hand-roll near-identical helpers; keeping them
//! here lets a new suite mount the standard fixtures in one call.

use std::fs;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Write `~/.config/stax/config.toml` under `home` with the given API base URL
/// and `[submit] stack_links = "off"`. `extra_toml` is appended as-is (empty
/// string for none).
pub(crate) fn write_stax_config(home: &Path, api_base_url: &str, extra_toml: &str) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("failed to create test config dir");
    let body = format!(
        "[remote]\napi_base_url = \"{api_base_url}\"\n\n[submit]\nstack_links = \"off\"\n{extra_toml}"
    );
    fs::write(config_dir.join("config.toml"), body).expect("failed to write test config");
}

/// Mock `GET /user` to return the given login as the authenticated user.
pub(crate) async fn mount_current_user(mock_server: &MockServer, login: &str) {
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": login,
            "id": 1,
            "node_id": "u1",
            "avatar_url": "https://example.com/avatar",
            "gravatar_id": "",
            "url": format!("https://api.github.com/users/{login}"),
            "html_url": format!("https://github.com/{login}"),
            "followers_url": format!("https://api.github.com/users/{login}/followers"),
            "following_url": format!("https://api.github.com/users/{login}/following"),
            "gists_url": format!("https://api.github.com/users/{login}/gists"),
            "starred_url": format!("https://api.github.com/users/{login}/starred"),
            "subscriptions_url": format!("https://api.github.com/users/{login}/subscriptions"),
            "organizations_url": format!("https://api.github.com/users/{login}/orgs"),
            "repos_url": format!("https://api.github.com/users/{login}/repos"),
            "events_url": format!("https://api.github.com/users/{login}/events"),
            "received_events_url": format!("https://api.github.com/users/{login}/received_events"),
            "type": "User",
            "site_admin": false
        })))
        .mount(mock_server)
        .await;
}

/// Mount the four mocks stax needs to create a PR and refresh it once:
/// list-open, create, list-issue-comments, get-by-number. `number`, `head`
/// (branch), and `base` populate the returned PR body. `owner_head_label`
/// controls the `head.label` field, which callers assert against for fork
/// submits (`contributor:branch`) or same-owner submits (`test-owner:branch`).
#[allow(dead_code)]
pub(crate) async fn mount_pr_create_and_refresh(
    mock_server: &MockServer,
    owner: &str,
    repo: &str,
    number: u64,
    head: &str,
    base: &str,
    owner_head_label: &str,
) {
    let pr_body = serde_json::json!({
        "url": format!("https://api.github.com/repos/{owner}/{repo}/pulls/{number}"),
        "id": number,
        "number": number,
        "state": "open",
        "title": head,
        "body": "",
        "draft": false,
        "head": { "ref": head, "sha": "aaaa", "label": format!("{owner_head_label}:{head}") },
        "base": { "ref": base, "sha": "bbbb" },
        "html_url": format!("https://github.com/{owner}/{repo}/pull/{number}")
    });

    Mock::given(method("GET"))
        .and(path(format!("/repos/{owner}/{repo}/pulls")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path(format!("/repos/{owner}/{repo}/pulls")))
        .respond_with(ResponseTemplate::new(201).set_body_json(pr_body.clone()))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/{owner}/{repo}/issues/{number}/comments"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!("/repos/{owner}/{repo}/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_body))
        .mount(mock_server)
        .await;
}
