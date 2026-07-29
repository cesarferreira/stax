use crate::common::github_mock::{
    mount_current_user, mount_pr_create_and_refresh, write_stax_config,
};
use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Reject every push to a bare repo the way GitHub rejects a push from a
    /// user without write access, so `push_error_indicates_no_write_access`
    /// has real vocabulary to classify.
    fn install_permission_denying_pre_receive_hook(bare_repo_path: &Path) {
        let hook = bare_repo_path.join("hooks").join("pre-receive");
        fs::write(
            &hook,
            "#!/bin/sh\necho 'Permission to test-owner/test-repo.git denied to contributor.' >&2\nexit 1\n",
        )
        .expect("write pre-receive hook");
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755))
            .expect("chmod pre-receive hook");
    }

    /// Redirect a fake `https://github.com/...` fork URL to a local bare repo,
    /// mirroring `TestRepo::configure_github_like_submit_remote`'s trick for
    /// origin so `git push` to the resolved fork remote hits real disk.
    fn configure_fork_insteadof(repo: &TestRepo, fork_bare_path: &Path, fork_https_url: &str) {
        let fork_path_str = fork_bare_path.to_string_lossy().to_string();
        let out = repo.git(&[
            "config",
            "--local",
            &format!("url.{}.insteadOf", fork_path_str),
            fork_https_url,
        ]);
        assert!(
            out.status.success(),
            "fork insteadOf config failed: {}",
            TestRepo::stderr(&out)
        );
    }

    async fn mock_no_existing_fork(mock_server: &MockServer, login: &str) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/{login}/test-repo")))
            .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
                "message": "Not Found",
            })))
            .mount(mock_server)
            .await;
    }

    async fn mock_create_fork(
        mock_server: &MockServer,
        login: &str,
        ssh_url: &str,
        https_url: &str,
    ) {
        Mock::given(method("POST"))
            .and(path("/repos/test-owner/test-repo/forks"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "fork": true,
                "owner": { "login": login },
                "ssh_url": ssh_url,
                "clone_url": https_url,
                "parent": { "full_name": "test-owner/test-repo" },
                "permissions": { "push": true }
            })))
            .mount(mock_server)
            .await;
    }

    #[tokio::test]
    async fn branch_submit_fork_pushes_branch_and_opens_pr_from_fork() {
        let mock_server = MockServer::start().await;
        // The authenticated user is a contributor who is NOT the upstream
        // owner — that is the only realistic setup for fork submit.
        mount_current_user(&mock_server, "contributor").await;
        mock_no_existing_fork(&mock_server, "contributor").await;

        let fork_bare = tempfile::tempdir().expect("fork bare tempdir");
        let fork_https_url = "https://github.com/contributor/test-repo.git";
        let fork_ssh_url = "git@github.com:contributor/test-repo.git";
        hermetic_git_init_bare(fork_bare.path());
        mock_create_fork(&mock_server, "contributor", fork_ssh_url, fork_https_url).await;
        mount_pr_create_and_refresh(
            &mock_server,
            "test-owner",
            "test-repo",
            42,
            "fork-fallback-branch",
            "main",
            "contributor",
        )
        .await;

        let repo = TestRepo::new_with_remote();
        let home = repo.clean_home();
        write_stax_config(Path::new(&home), &mock_server.uri(), "");
        repo.configure_github_like_submit_remote();
        configure_fork_insteadof(&repo, fork_bare.path(), fork_https_url);

        let remote_path = repo.remote_path().expect("origin bare path");
        install_permission_denying_pre_receive_hook(&remote_path);

        repo.run_stax(&["bc", "fork-fallback-branch"])
            .assert_success();
        repo.create_file("fork-fallback.txt", "content");
        repo.commit("Commit for fork-fallback-branch");
        let branch = repo.current_branch();

        let output = repo.run_stax_with_env(
            &[
                "branch",
                "submit",
                "--fork",
                "--yes",
                "--no-prompt",
                "--publish",
                "--no-template",
            ],
            &[("STAX_GITHUB_TOKEN", "test-token")],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        // Branch landed on the fork, not on the (permission-denied) origin.
        let fork_branches = hermetic_git_list_branches(fork_bare.path());
        assert!(
            fork_branches.contains(&branch),
            "expected {branch} to be pushed to the fork, got: {fork_branches:?}"
        );

        // Read the literal configured value (not `remote get-url`, which
        // applies the `insteadOf` rewrite used to redirect this test's fake
        // fork URL onto a local bare repo).
        let fork_remote_url = repo.git(&["config", "--local", "--get", "remote.fork.url"]);
        fork_remote_url.assert_success();
        assert_eq!(
            TestRepo::stdout(&fork_remote_url).trim(),
            fork_https_url,
            "fork remote should point at the resolved fork URL"
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_payload = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST"
                    && request.url.path() == "/repos/test-owner/test-repo/pulls"
            })
            .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap())
            .expect("missing PR create payload");

        assert_eq!(
            pr_payload["head"],
            format!("contributor:{branch}"),
            "PR head should be qualified with the fork owner, not the upstream owner: {pr_payload}"
        );
        assert_eq!(pr_payload["base"], "main");
    }

    #[test]
    fn branch_submit_permission_denied_without_fork_opt_in_fails_actionably() {
        let repo = TestRepo::new_with_remote();
        repo.configure_github_like_submit_remote();
        let remote_path = repo.remote_path().expect("origin bare path");
        install_permission_denying_pre_receive_hook(&remote_path);

        repo.run_stax(&["bc", "fork-optin-branch"]).assert_success();
        repo.create_file("fork-optin.txt", "content");
        repo.commit("Commit for fork-optin-branch");

        repo.run_stax(&["branch", "submit", "--no-pr", "--yes", "--no-prompt"])
            .assert_failure()
            .assert_stderr_contains("--fork")
            .assert_stderr_contains("auto_fork");

        let remotes = repo.git(&["remote"]);
        remotes.assert_success();
        assert!(
            !TestRepo::stdout(&remotes).lines().any(|r| r == "fork"),
            "no fork remote should be created when not opted in"
        );
    }

    #[test]
    fn multi_branch_stack_permission_denied_with_fork_reports_single_branch_error() {
        let repo = TestRepo::new_with_remote();
        repo.configure_github_like_submit_remote();
        let remote_path = repo.remote_path().expect("origin bare path");
        install_permission_denying_pre_receive_hook(&remote_path);

        let branches = repo.create_stack(&["fork-stack-a", "fork-stack-b"]);
        repo.run_stax(&["checkout", &branches[0]]).assert_success();

        // Now that the default-submit router honours `--fork`, `Stack` scope +
        // `--no-pr` + `--fork` reaches the fork-fallback code path directly and
        // the multi-branch guard fires with a single-branch error.
        repo.run_stax(&[
            "stack",
            "submit",
            "--fork",
            "--no-pr",
            "--yes",
            "--no-prompt",
        ])
        .assert_failure()
        .assert_stderr_contains("single branch");

        let remotes = repo.git(&["remote"]);
        remotes.assert_success();
        assert!(
            !TestRepo::stdout(&remotes).lines().any(|r| r == "fork"),
            "no fork remote should be created for a multi-branch permission error"
        );
    }

    #[test]
    fn branch_submit_fork_conflicting_existing_fork_remote_fails() {
        let repo = TestRepo::new_with_remote();
        let home = repo.clean_home();
        // No live wiremock server needed — the conflict check fires before
        // any API call.
        write_stax_config(Path::new(&home), "http://127.0.0.1:1", "");
        repo.configure_github_like_submit_remote();
        // Pre-existing `fork` remote pointing at an unrelated URL that should
        // NOT be silently overwritten.
        repo.git(&[
            "remote",
            "add",
            "fork",
            "https://github.com/someone-else/unrelated.git",
        ])
        .assert_success();
        let remote_path = repo.remote_path().expect("origin bare path");
        install_permission_denying_pre_receive_hook(&remote_path);

        repo.run_stax(&["bc", "conflicting-fork-branch"])
            .assert_success();
        repo.create_file("conflict.txt", "content");
        repo.commit("Commit for conflicting-fork-branch");

        // No `remote.fork_remote` set → stax will try to auto-detect and hit
        // the `find_pushable_fork` API. Without a live server that fails, but
        // the point of this test is that the pre-existing `fork` remote's URL
        // is still what it was.
        let _ = repo.run_stax_with_env(
            &[
                "branch",
                "submit",
                "--fork",
                "--yes",
                "--no-prompt",
                "--publish",
                "--no-template",
            ],
            &[("STAX_GITHUB_TOKEN", "test-token")],
        );

        let after = repo.git(&["config", "--local", "--get", "remote.fork.url"]);
        after.assert_success();
        assert_eq!(
            TestRepo::stdout(&after).trim(),
            "https://github.com/someone-else/unrelated.git",
            "stax must not silently repoint a pre-existing `fork` remote"
        );
    }
}

fn hermetic_git_init_bare(path: &Path) {
    let output = std::process::Command::new("git")
        .args(["init", "--bare"])
        .current_dir(path)
        .output()
        .expect("git init --bare");
    assert!(output.status.success());
}

fn hermetic_git_list_branches(bare_path: &Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .current_dir(bare_path)
        .output()
        .expect("git for-each-ref");
    assert!(output.status.success());
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.to_string())
        .collect()
}
