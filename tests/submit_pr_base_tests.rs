//! Regression tests for `stax submit`'s PR base handling once a PR is
//! registered with GitHub's native Stacked PRs feature.
//!
//! Once a PR is linked into a native GitHub Stack (via `gh stack link`,
//! triggered automatically by submit — see `gh_stack_tests.rs`), GitHub's
//! REST API rejects *any* `PATCH .../pulls/{n}` call that includes `base`,
//! even a no-op re-send of the PR's current base. Submit used to call
//! `update_pr_base` whenever a branch merely needed a push, regardless of
//! whether the base had actually changed, which broke every subsequent
//! submit for a native-linked stack. See `is_native_stack_base_locked_error`
//! and `needs_base_update` in `src/commands/submit.rs`.

use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_test_config(home: &Path, api_base_url: &str) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("failed to create test config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "[remote]\napi_base_url = \"{api_base_url}\"\n\n\
             [submit]\nstack_links = \"off\"\nnative_stack = \"off\"\n"
        ),
    )
    .expect("failed to write test config");
}

fn pr_fixture(number: u64, branch: &str, base: &str) -> serde_json::Value {
    serde_json::json!({
        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
        "id": number,
        "number": number,
        "state": "open",
        "draft": false,
        "title": format!("PR {number}"),
        "body": "",
        "head": { "ref": branch, "sha": "aaaa", "label": format!("test-owner:{branch}") },
        "base": { "ref": base, "sha": "bbbb" },
        "html_url": format!("https://github.com/test-owner/test-repo/pull/{number}")
    })
}

/// Mounts the GET endpoints submit always hits for an existing PR: the PR
/// itself and its issue comments (checked even with `stack_links = "off"`).
async fn mock_existing_pr_reads(mock_server: &MockServer, number: u64, branch: &str, base: &str) {
    Mock::given(method("GET"))
        .and(path(format!("/repos/test-owner/test-repo/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(pr_fixture(number, branch, base)))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test-owner/test-repo/issues/{number}/comments"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, parent: &str, pr_number: u64) {
    let parent_revision = {
        let output = repo.git(&["rev-parse", parent]);
        output.assert_success();
        TestRepo::stdout(&output).trim().to_string()
    };
    let metadata = serde_json::json!({
        "parentBranchName": parent,
        "parentBranchRevision": parent_revision,
        "prInfo": {
            "number": pr_number,
            "state": "OPEN",
            "isDraft": false
        }
    });

    let metadata_file = tempfile::NamedTempFile::new().expect("metadata temp file");
    fs::write(metadata_file.path(), metadata.to_string()).expect("write metadata temp file");
    let hash = repo.git(&[
        "hash-object",
        "-w",
        metadata_file.path().to_str().expect("metadata path"),
    ]);
    hash.assert_success();
    let blob = TestRepo::stdout(&hash);
    repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{branch}"),
        blob.trim(),
    ])
    .assert_success();
}

/// Pushing new commits to an already-open PR whose base hasn't changed must
/// not re-send `base` in the update PATCH — GitHub rejects that once the PR
/// is part of a native Stack, and even off a native stack it is a pointless
/// no-op call.
#[tokio::test]
async fn submit_does_not_repatch_base_when_only_pushing_new_commits() {
    let mock_server = MockServer::start().await;
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());
    repo.configure_github_like_submit_remote();

    repo.create_stack(&["base-stable"]);
    let branch = repo.current_branch();
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();

    write_branch_pr_metadata(&repo, &branch, "main", 501);
    mock_existing_pr_reads(&mock_server, 501, &branch, "main").await;

    // New local commit, not yet pushed: `needs_push` becomes true while the
    // base ("main") stays exactly what GitHub already reports.
    repo.create_file("base-stable-extra.txt", "more work\n");
    repo.commit("More work on base-stable");

    // Deliberately no PATCH mock: if stax still called `update_pr_base` here,
    // wiremock would answer 404 and the unfixed code would abort the whole
    // submit with "Failed to update PR base".
    let output = repo.run_stax_with_env(
        &["submit", "--yes", "--no-prompt", "--no-template"],
        &[("STAX_GITHUB_TOKEN", "test-token")],
    );
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));

    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        !requests.iter().any(|r| r.method.as_str() == "PATCH"
            && r.url.path() == "/repos/test-owner/test-repo/pulls/501"),
        "submit should not PATCH an unchanged PR base: {requests:#?}"
    );
}

/// When the base genuinely does need to change but GitHub rejects it because
/// the PR is registered in a native Stack, submit must report a soft note
/// and keep going rather than aborting the whole run.
#[tokio::test]
async fn submit_treats_native_stack_base_lock_as_non_fatal() {
    let mock_server = MockServer::start().await;
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());
    repo.configure_github_like_submit_remote();

    repo.create_stack(&["base-locked"]);
    let branch = repo.current_branch();

    // Local metadata says the parent is "main", but GitHub still reports a
    // stale base for the PR — a genuine mismatch, not a push-only no-op.
    write_branch_pr_metadata(&repo, &branch, "main", 502);
    mock_existing_pr_reads(&mock_server, 502, &branch, "stale-base").await;

    Mock::given(method("PATCH"))
        .and(path("/repos/test-owner/test-repo/pulls/502"))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "message": "Validation Failed",
            "documentation_url": "https://docs.github.com/rest/pulls/pulls#update-a-pull-request",
            "errors": [{
                "code": "invalid",
                "field": "base",
                "message": "Cannot change the base branch because the pull request is part of a stack.",
                "resource": "PullRequest"
            }]
        })))
        .mount(&mock_server)
        .await;

    let output = repo.run_stax_with_env(
        &["submit", "--yes", "--no-prompt", "--no-template"],
        &[("STAX_GITHUB_TOKEN", "test-token")],
    );
    assert!(
        output.status.success(),
        "submit should not abort on a native-stack base lock: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("skipped base update") && stdout.contains("native Stack"),
        "expected a soft note about the locked base, got: {stdout}"
    );
}
