//! PR open command integration tests.

use crate::common::{IsolatedProcessEnv, OutputAssertions, TestRepo};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn pr_open_network_fallback_does_not_panic_without_a_reactor() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-open"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ])
    .assert_success();

    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let env = IsolatedProcessEnv::with_config(&format!(
        "[remote]\napi_base_url = \"{}\"\n",
        server.uri()
    ));
    let output = env
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .arg("pr")
        .output()
        .expect("run stax pr");

    assert!(
        !output.status.success(),
        "st pr should report the missing PR"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        !stderr.contains("there is no reactor running"),
        "st pr must not panic while creating the forge client:\n{stderr}"
    );
    assert!(
        stderr.contains("No PR found for branch"),
        "expected a missing-PR error after the forge lookup, got:\n{stderr}"
    );
}

#[tokio::test]
async fn pr_open_preserves_lookup_authentication_failure() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-open"]);
    repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ])
    .assert_success();

    let server = MockServer::start().await;
    Mock::given(wiremock::matchers::method("GET"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let env = IsolatedProcessEnv::with_config(&format!(
        "[remote]\napi_base_url = \"{}\"\n",
        server.uri()
    ));
    let output = env
        .command(&repo.path())
        .env("STAX_GITHUB_TOKEN", "test-token")
        .arg("pr")
        .output()
        .expect("run stax pr");

    assert!(
        !output.status.success(),
        "st pr should report the lookup failure"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("Could not resolve the pull request"),
        "expected the structured lookup failure, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("No PR found for branch"),
        "lookup failure must not be reported as a missing PR:\n{stderr}"
    );
}
