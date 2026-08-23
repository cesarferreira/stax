//! Integration tests for `stax branch track --all-prs` command

use crate::common;
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
        format!("[remote]\napi_base_url = \"{api_base_url}\"\n"),
    )
    .expect("Failed to write config");
}

/// Test that --all-prs flag is recognized by the CLI
#[test]
fn test_track_all_prs_flag_recognized() {
    let repo = TestRepo::new_with_remote();

    // Running --all-prs should not fail with "unrecognized flag"
    let output = repo.run_stax(&["branch", "track", "--all-prs"]);
    let stderr = TestRepo::stderr(&output);

    // The command may fail due to missing GitHub token, but the flag should be recognized
    assert!(
        !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
        "Flag --all-prs should be recognized, got: {}",
        stderr
    );
}

/// Test that --all-prs and --parent flags conflict
#[test]
fn test_track_all_prs_conflicts_with_parent() {
    let repo = TestRepo::new_with_remote();

    let output = repo.run_stax(&["branch", "track", "--all-prs", "--parent", "main"]);

    // Should fail because --all-prs conflicts with --parent
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "Expected conflict error, got: {}",
        stderr
    );
}

/// Test that --all-prs fails gracefully without GitHub token or proper remote
#[test]
fn test_track_all_prs_no_token() {
    let repo = TestRepo::new_with_remote();

    let output = repo.run_stax(&["branch", "track", "--all-prs"]);

    // Should fail - the command requires GitHub integration
    // It may fail due to:
    // 1. Missing token (if remote URL is valid GitHub URL)
    // 2. Invalid remote URL format (test uses local path as remote)
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    let stdout = TestRepo::stdout(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should have some error, either about token, auth, or remote URL
    assert!(
        combined.contains("token")
            || combined.contains("auth")
            || combined.contains("Token")
            || combined.contains("remote")
            || combined.contains("URL"),
        "Expected error about GitHub integration, got stdout: {}\nstderr: {}",
        stdout,
        stderr
    );
}

/// A successful import must create local branches which work with plain
/// `git pull`, and preserve the dependency between a root PR and its child.
#[tokio::test]
async fn track_all_prs_fetches_stacked_branches_sets_upstreams_and_parents() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());

    repo.git(&["checkout", "-b", "root-pr"]).assert_success();
    repo.create_file("root.txt", "root\n");
    repo.commit("Root PR");
    repo.git(&["push", "origin", "root-pr"]).assert_success();

    repo.git(&["checkout", "-b", "child-pr"]).assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child PR");
    repo.git(&["push", "origin", "child-pr"]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["branch", "-D", "root-pr"]).assert_success();
    repo.git(&["branch", "-D", "child-pr"]).assert_success();

    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "login": "test-user",
            "id": 1,
            "node_id": "MDQ6VXNlcjE=",
            "avatar_url": "https://example.test/avatar",
            "gravatar_id": "",
            "url": "https://api.github.test/users/test-user",
            "html_url": "https://github.test/test-user",
            "followers_url": "https://api.github.test/users/test-user/followers",
            "following_url": "https://api.github.test/users/test-user/following{/other_user}",
            "gists_url": "https://api.github.test/users/test-user/gists{/gist_id}",
            "starred_url": "https://api.github.test/users/test-user/starred{/owner}{/repo}",
            "subscriptions_url": "https://api.github.test/users/test-user/subscriptions",
            "organizations_url": "https://api.github.test/users/test-user/orgs",
            "repos_url": "https://api.github.test/users/test-user/repos",
            "events_url": "https://api.github.test/users/test-user/events{/privacy}",
            "received_events_url": "https://api.github.test/users/test-user/received_events",
            "type": "User",
            "site_admin": false,
            "name": null,
            "company": null,
            "blog": null,
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
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search/issues"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "total_count": 2,
            "incomplete_results": false,
            "items": [
                {
                    "number": 101,
                    "title": "Root PR",
                    "html_url": "https://github.test/test-owner/test-repo/pull/101",
                    "created_at": "2026-01-01T00:00:00Z",
                    "closed_at": null
                },
                {
                    "number": 102,
                    "title": "Child PR",
                    "html_url": "https://github.test/test-owner/test-repo/pull/102",
                    "created_at": "2026-01-01T00:00:00Z",
                    "closed_at": null
                }
            ]
        })))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/101"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 101,
            "url": "https://api.github.com/repos/test-owner/test-repo/pulls/101",
            "locked": false,
            "number": 101,
            "head": { "ref": "root-pr", "sha": "1111111111111111111111111111111111111111" },
            "base": { "ref": "main", "sha": "0000000000000000000000000000000000000000" },
            "draft": false
        })))
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/102"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 102,
            "url": "https://api.github.com/repos/test-owner/test-repo/pulls/102",
            "locked": false,
            "number": 102,
            "head": { "ref": "child-pr", "sha": "2222222222222222222222222222222222222222" },
            "base": { "ref": "root-pr", "sha": "1111111111111111111111111111111111111111" },
            "draft": true
        })))
        .mount(&mock_server)
        .await;

    repo.run_stax_with_env(
        &["branch", "track", "--all-prs"],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    )
    .assert_success()
    .assert_stdout_contains("Tracked 2 branch(es), fetched 2")
    .assert_stdout_contains("Set upstream to 'origin' on 2 newly fetched branch(es)");

    for branch in ["root-pr", "child-pr"] {
        repo.git(&["rev-parse", "--verify", branch])
            .assert_success();
        let remote = repo.git(&["config", "--get", &format!("branch.{branch}.remote")]);
        assert_eq!(TestRepo::stdout(&remote).trim(), "origin");
        let merge = repo.git(&["config", "--get", &format!("branch.{branch}.merge")]);
        assert_eq!(
            TestRepo::stdout(&merge).trim(),
            format!("refs/heads/{branch}")
        );
    }

    let status = repo.get_status_json();
    let branches = status["branches"]
        .as_array()
        .expect("Expected branches array");
    let parent_of = |branch: &str| {
        branches
            .iter()
            .find(|entry| entry["name"].as_str() == Some(branch))
            .and_then(|entry| entry["parent"].as_str())
            .unwrap_or_else(|| panic!("{branch} should be tracked"))
    };
    assert_eq!(parent_of("root-pr"), "main");
    assert_eq!(parent_of("child-pr"), "root-pr");
}

/// Test help text includes --all-prs
#[test]
fn test_track_help_includes_all_prs() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "track", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--all-prs"),
        "Help should mention --all-prs flag, got: {}",
        stdout
    );
    assert!(
        stdout.contains("open PRs"),
        "Help should describe what --all-prs does, got: {}",
        stdout
    );
}

/// Test that existing track command still works without --all-prs
#[test]
fn test_track_single_branch_still_works() {
    let repo = TestRepo::new();

    // Create an untracked branch with git directly
    repo.git(&["checkout", "-b", "untracked-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Track it with stax
    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    output.assert_success();

    // Verify it's now tracked
    let json = repo.get_status_json();
    let branches = json["branches"]
        .as_array()
        .expect("Expected branches array");
    let tracked = branches
        .iter()
        .any(|b| b["name"].as_str() == Some("untracked-feature"));
    assert!(
        tracked,
        "Branch should be tracked after running track command"
    );

    // Verify parent is correct
    let feature = branches
        .iter()
        .find(|b| b["name"].as_str() == Some("untracked-feature"))
        .expect("Branch not found");
    assert_eq!(
        feature["parent"].as_str(),
        Some("main"),
        "Parent should be main"
    );
}

/// Test that already tracked branches are handled correctly
#[test]
fn test_track_already_tracked_branch() {
    let repo = TestRepo::new();

    // Create a tracked branch with stax
    repo.create_stack(&["tracked-feature"]);

    // Go back to that branch
    repo.run_stax(&[
        "checkout",
        &repo.find_branch_containing("tracked-feature").unwrap(),
    ]);

    // Try to track it again (should fail gracefully)
    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("already tracked") || stdout.contains("reparent"),
        "Should indicate branch is already tracked, got: {}",
        stdout
    );
}

#[test]
fn submit_force_with_lease_uses_post_fetch_remote_oid() {
    let repo = TestRepo::new_with_remote();
    repo.set_trunk("main");
    repo.configure_github_like_submit_remote();
    let branch = repo.create_stack(&["lease-check"]).remove(0);
    repo.git(&["push", "-u", "origin", &branch])
        .assert_success();
    let remote_oid_after_fetch = repo.get_commit_sha(&format!("origin/{branch}"));

    repo.create_file("lease-check-update.txt", "updated\n");
    repo.commit("Update lease-check");

    let trace_dir = tempfile::tempdir().unwrap();
    let trace_path = trace_dir.path().join("git-trace.log");
    let output = repo.run_stax_with_env(
        &["submit", "--no-pr", "--no-prompt", "--yes"],
        &[("GIT_TRACE", trace_path.to_str().unwrap())],
    );
    output.assert_success();

    let trace = std::fs::read_to_string(&trace_path).unwrap();
    let expected = format!("--force-with-lease=refs/heads/{branch}:{remote_oid_after_fetch}");
    assert!(
        trace.contains(&expected),
        "submit must push with an explicit post-fetch lease `{expected}`; trace was:\n{trace}"
    );
}
