mod common;

#[cfg(unix)]
mod unix {
    use super::common::{OutputAssertions, TestRepo};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn configure_submit_remote(repo: &TestRepo) {
        let remote_path = repo
            .remote_path()
            .expect("Expected remote path for repository with origin");
        let remote_path_str = remote_path.to_string_lossy().to_string();

        let out = repo.git(&[
            "remote",
            "set-url",
            "origin",
            "https://github.com/test-owner/test-repo.git",
        ]);
        assert!(
            out.status.success(),
            "set-url failed: {}",
            TestRepo::stderr(&out)
        );

        let out = repo.git(&["remote", "set-url", "--push", "origin", &remote_path_str]);
        assert!(
            out.status.success(),
            "set-url --push failed: {}",
            TestRepo::stderr(&out)
        );

        let file_url = format!("file://{}", remote_path_str);
        let instead_of_key = format!("url.{}.insteadOf", file_url.trim_end_matches('/'));
        let out = repo.git(&[
            "config",
            "--local",
            &instead_of_key,
            "https://github.com/test-owner/test-repo.git",
        ]);
        assert!(
            out.status.success(),
            "insteadOf config failed: {}",
            TestRepo::stderr(&out)
        );
    }

    fn install_failing_pre_push_hook(repo: &TestRepo) {
        let hook = repo.path().join(".git/hooks/pre-push");
        fs::write(
            &hook,
            "#!/bin/sh\necho 'pre-push hook blocked submit' >&2\nexit 1\n",
        )
        .expect("write pre-push hook");
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).expect("chmod pre-push hook");
    }

    fn repo_with_failing_pre_push_hook(branch_name: &str) -> (TestRepo, String) {
        let repo = TestRepo::new_with_remote();
        configure_submit_remote(&repo);

        repo.run_stax(&["bc", branch_name]).assert_success();
        repo.create_file(
            &format!("{branch_name}.txt"),
            &format!("content for {branch_name}"),
        );
        repo.commit(&format!("Commit for {branch_name}"));
        let branch = repo.current_branch();

        install_failing_pre_push_hook(&repo);
        (repo, branch)
    }

    fn remote_has_branch(repo: &TestRepo, branch: &str) -> bool {
        let out = repo.git(&["ls-remote", "--heads", "origin", branch]);
        assert!(
            out.status.success(),
            "ls-remote failed: {}",
            TestRepo::stderr(&out)
        );

        let expected_ref = format!("refs/heads/{branch}");
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .any(|line| line.ends_with(&expected_ref))
    }

    #[test]
    fn submit_no_verify_skips_pre_push_hook() {
        let (repo, branch) = repo_with_failing_pre_push_hook("submit-no-verify");

        repo.run_stax(&["ss", "--no-fetch", "--no-pr", "--no-prompt", "--yes"])
            .assert_failure();
        assert!(
            !remote_has_branch(&repo, &branch),
            "branch should not exist after hook-blocked push"
        );

        repo.run_stax(&[
            "ss",
            "--no-fetch",
            "--no-pr",
            "--no-prompt",
            "--yes",
            "--no-verify",
        ])
        .assert_success();
        assert!(
            remote_has_branch(&repo, &branch),
            "branch should be pushed when --no-verify bypasses the hook"
        );
    }

    #[test]
    fn branch_submit_alias_no_verify_skips_pre_push_hook() {
        let (repo, branch) = repo_with_failing_pre_push_hook("bs-no-verify");

        repo.run_stax(&["bs", "--no-fetch", "--no-pr", "--no-prompt", "--yes", "-n"])
            .assert_success();
        assert!(
            remote_has_branch(&repo, &branch),
            "bs -n should bypass the pre-push hook"
        );
    }

    #[test]
    fn upstack_submit_no_verify_skips_pre_push_hook() {
        let (repo, branch) = repo_with_failing_pre_push_hook("upstack-no-verify");

        repo.run_stax(&[
            "upstack",
            "submit",
            "--no-fetch",
            "--no-pr",
            "--no-prompt",
            "--yes",
            "--no-verify",
        ])
        .assert_success();
        assert!(
            remote_has_branch(&repo, &branch),
            "upstack submit --no-verify should bypass the pre-push hook"
        );
    }
}
