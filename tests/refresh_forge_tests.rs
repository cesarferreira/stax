//! `st refresh` against a mocked GitHub API: after cleanup deletes the bottom branch,
//! the submit phase must run from the next branch's stack and update its PR.

use crate::common::{OutputAssertions, TestRepo};
use crate::gh_stack_tests::{
    fake_gh_dir, mock_existing_pr, path_with_fake_gh, write_branch_pr_metadata, write_config,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// A GitHub pull request payload for a PR the forge reports as merged.
async fn mock_merged_pr(mock_server: &MockServer, number: u64, branch: &str, base: &str) {
    let body = serde_json::json!({
        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
        "id": number,
        "number": number,
        "state": "closed",
        "merged": true,
        "merged_at": "2026-10-01T00:00:00Z",
        "title": format!("PR {number}"),
        "body": "",
        "draft": false,
        "head": { "ref": branch, "sha": "aaaa", "label": format!("test-owner:{branch}") },
        "base": { "ref": base, "sha": "bbbb" },
        "html_url": format!("https://github.com/test-owner/test-repo/pull/{number}")
    });
    Mock::given(method("GET"))
        .and(path(format!("/repos/test-owner/test-repo/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(mock_server)
        .await;
}

struct Forge {
    repo: TestRepo,
    server: MockServer,
    home: String,
    branches: Vec<String>,
}

impl Forge {
    /// Branches `names` (bottom first) with PRs numbered 10, 20, 30, ... on a mocked
    /// GitHub. The bottom branch is squash-merged on the remote and checked out.
    async fn with_squash_merged_bottom(names: &[&str]) -> Self {
        let server = MockServer::start().await;
        let repo = TestRepo::new_with_remote();
        let home = repo.clean_home();
        write_config(&home, &server.uri());
        repo.configure_github_like_submit_remote();
        let branches = repo.create_stack(names);
        let mut push = vec!["push", "-u", "origin"];
        push.extend(branches.iter().map(String::as_str));
        repo.git(&push).assert_success();
        for (index, branch) in branches.iter().enumerate() {
            let number = 10 * (index as u64 + 1);
            let parent = if index == 0 {
                "main"
            } else {
                branches[index - 1].as_str()
            };
            write_branch_pr_metadata(&repo, branch, parent, number);
            mock_existing_pr(&server, number, branch, parent).await;
        }
        repo.squash_merge_branch_on_remote(&branches[0]);
        repo.git(&["checkout", &branches[0]]).assert_success();
        Self {
            repo,
            server,
            home,
            branches,
        }
    }

    fn refresh(&self, extra: &[&str]) -> std::process::Output {
        let fake = fake_gh_dir("#!/bin/sh\nexit 1\n");
        let path = path_with_fake_gh(fake.path());
        let mut args = vec![
            "refresh",
            "--force",
            "--yes",
            "--no-prompt",
            "--delete-merged",
        ];
        args.extend_from_slice(extra);
        self.repo.run_stax_with_env(
            &args,
            &[
                ("HOME", &self.home),
                ("STAX_GITHUB_TOKEN", "test-token"),
                ("PATH", &path),
            ],
        )
    }

    /// `(method, path, body)` of every request the mock received.
    async fn requests(&self) -> Vec<(String, String, String)> {
        self.server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .map(|r| {
                (
                    r.method.to_string(),
                    r.url.path().to_string(),
                    String::from_utf8_lossy(&r.body).to_string(),
                )
            })
            .collect()
    }
}

fn patches_to(requests: &[(String, String, String)], pr: u64) -> Vec<&str> {
    let target = format!("/repos/test-owner/test-repo/pulls/{pr}");
    requests
        .iter()
        .filter(|(method, path, _)| method == "PATCH" && *path == target)
        .map(|(_, _, body)| body.as_str())
        .collect()
}

#[tokio::test]
async fn refresh_updates_the_child_pr_after_the_bottom_branch_is_deleted() {
    let forge = Forge::with_squash_merged_bottom(&["rf1-bottom", "rf1-child"]).await;

    let output = forge.refresh(&[]);
    output.assert_success();
    output.assert_stdout_contains("Updating");

    assert_eq!(forge.repo.current_branch(), forge.branches[1]);
    assert!(!forge.repo.list_branches().contains(&forge.branches[0]));

    let requests = forge.requests().await;
    let child_patches = patches_to(&requests, 20);
    assert!(
        child_patches
            .iter()
            .any(|body| body.contains("\"base\":\"main\"")),
        "the child PR must be retargeted to main; PATCH bodies: {child_patches:?}"
    );
    assert!(
        child_patches
            .iter()
            .any(|body| body.contains("stax-stack-links") && body.contains("PR #20")),
        "the child PR's stack links must be refreshed from the child's stack; {child_patches:?}"
    );
    assert!(
        patches_to(&requests, 10).is_empty(),
        "the merged bottom PR must not be modified"
    );
    assert!(
        !requests
            .iter()
            .any(|(method, path, _)| method == "POST" && path.ends_with("/pulls")),
        "no new PR should be created"
    );
}

#[tokio::test]
async fn refresh_updates_every_remaining_pr_of_a_three_branch_stack() {
    let forge = Forge::with_squash_merged_bottom(&["rf2-bottom", "rf2-middle", "rf2-top"]).await;

    forge.refresh(&[]).assert_success();

    assert_eq!(forge.repo.current_branch(), forge.branches[1]);
    let requests = forge.requests().await;
    assert!(
        patches_to(&requests, 20)
            .iter()
            .any(|body| body.contains("\"base\":\"main\"")),
        "the middle PR is retargeted to main"
    );
    // Both remaining PRs get stack links that show the whole remaining stack.
    for pr in [20, 30] {
        assert!(
            patches_to(&requests, pr).iter().any(|body| {
                body.contains("stax-stack-links")
                    && body.contains("PR #20")
                    && body.contains("PR #30")
            }),
            "PR #{pr} should list the remaining stack (#20 and #30)"
        );
    }
    assert!(patches_to(&requests, 10).is_empty());
}

#[tokio::test]
async fn refresh_lands_on_the_child_when_the_forge_reports_the_bottom_pr_merged() {
    let forge = Forge::with_squash_merged_bottom(&["rf3-bottom", "rf3-child"]).await;
    // GitHub's answer for the bottom PR is "merged" (the realistic signal).
    mock_merged_pr(&forge.server, 10, &forge.branches[0], "main").await;

    forge.refresh(&[]).assert_success();

    assert_eq!(forge.repo.current_branch(), forge.branches[1]);
    assert!(!forge.repo.list_branches().contains(&forge.branches[0]));
    let requests = forge.requests().await;
    assert!(
        patches_to(&requests, 20)
            .iter()
            .any(|body| body.contains("\"base\":\"main\""))
    );
}

#[tokio::test]
async fn refresh_still_lands_on_the_child_when_the_forge_is_failing() {
    let forge = Forge::with_squash_merged_bottom(&["rf4-bottom", "rf4-child"]).await;
    // Every PR endpoint now errors.
    forge.server.reset().await;
    Mock::given(wiremock::matchers::any())
        .respond_with(ResponseTemplate::new(500))
        .mount(&forge.server)
        .await;

    let output = forge.refresh(&[]);
    let transcript = format!(
        "stdout:\n{}\nstderr:\n{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    // The git-level work happened regardless of the forge: never stranded on trunk
    // with a half-deleted stack.
    assert!(
        !forge.repo.list_branches().contains(&forge.branches[0]),
        "bottom deleted; {transcript}"
    );
    assert_eq!(
        forge.repo.current_branch(),
        forge.branches[1],
        "{transcript}"
    );
    assert!(!forge.repo.has_rebase_in_progress());
}

#[tokio::test]
async fn refresh_creates_the_missing_child_pr_against_trunk() {
    let server = MockServer::start().await;
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["rf5-bottom", "rf5-child"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    // Only the bottom branch has a PR; the child has none yet.
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    mock_existing_pr(&server, 10, &branches[0], "main").await;
    repo.squash_merge_branch_on_remote(&branches[0]);
    repo.git(&["checkout", &branches[0]]).assert_success();

    // No existing PR for the child's head; creating one returns PR #21.
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;
    let created = serde_json::json!({
        "url": "https://api.github.com/repos/test-owner/test-repo/pulls/21",
        "id": 21,
        "number": 21,
        "state": "open",
        "title": "Commit for rf5-child",
        "body": "",
        "draft": false,
        "head": { "ref": branches[1], "sha": "cccc", "label": format!("test-owner:{}", branches[1]) },
        "base": { "ref": "main", "sha": "bbbb" },
        "html_url": "https://github.com/test-owner/test-repo/pull/21"
    });
    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(created.clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/21"))
        .respond_with(ResponseTemplate::new(200).set_body_json(created.clone()))
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path("/repos/test-owner/test-repo/pulls/21"))
        .respond_with(ResponseTemplate::new(200).set_body_json(created))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/issues/21/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&server)
        .await;

    let fake = fake_gh_dir("#!/bin/sh\nexit 1\n");
    let path_env = path_with_fake_gh(fake.path());
    let output = repo.run_stax_with_env(
        &[
            "refresh",
            "--force",
            "--yes",
            "--no-prompt",
            "--delete-merged",
        ],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path_env),
        ],
    );
    let transcript = format!(
        "stdout:\n{}\nstderr:\n{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(output.status.success(), "{transcript}");

    assert_eq!(repo.current_branch(), branches[1]);
    let posts: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| {
            r.method.as_str() == "POST" && r.url.path() == "/repos/test-owner/test-repo/pulls"
        })
        .map(|r| String::from_utf8_lossy(&r.body).to_string())
        .collect();
    assert_eq!(posts.len(), 1, "exactly one PR is created; {transcript}");
    assert!(
        posts[0].contains("\"base\":\"main\"") && posts[0].contains(&branches[1]),
        "the new PR targets main from the child branch: {}",
        posts[0]
    );
}
