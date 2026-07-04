use crate::common::{OutputAssertions, TestRepo, stax_bin};
use std::fs;
use std::process::{Command, Output};

fn run_copy_without_clipboard(repo: &TestRepo, args: &[&str]) -> Output {
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let home = repo.clean_home();

    let mut cmd = Command::new(stax_bin());
    cmd.args(args)
        .current_dir(repo.path())
        .env("HOME", home)
        .env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env("STAX_TEST_DISABLE_HEAD_SYNC", "1")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .env_remove("XDG_SESSION_TYPE")
        .env_remove("GITHUB_TOKEN")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("GH_TOKEN")
        .env_remove("STAX_SHELL_INTEGRATION");

    cmd.output().expect("Failed to execute stax copy")
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, pr_number: u64) {
    let metadata = serde_json::json!({
        "parentBranchName": "main",
        "parentBranchRevision": repo.get_commit_sha("main"),
        "prInfo": {
            "number": pr_number,
            "state": "OPEN"
        }
    });

    let metadata_file = tempfile::NamedTempFile::new().expect("metadata temp file");
    fs::write(metadata_file.path(), metadata.to_string()).expect("write metadata temp file");

    let hash = repo.git(&[
        "hash-object",
        "-w",
        metadata_file.path().to_str().expect("metadata path"),
    ]);
    assert!(
        hash.status.success(),
        "git hash-object failed: {}",
        TestRepo::stderr(&hash)
    );

    let update = repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{}", branch),
        TestRepo::stdout(&hash).trim(),
    ]);
    assert!(
        update.status.success(),
        "git update-ref failed: {}",
        TestRepo::stderr(&update)
    );
}

#[test]
fn copy_prints_branch_and_succeeds_when_clipboard_is_unavailable() {
    let repo = TestRepo::new();

    let output = run_copy_without_clipboard(&repo, &["copy"]);

    assert!(
        output.status.success(),
        "expected stax copy to succeed without clipboard\nstderr:\n{}\nstdout:\n{}",
        TestRepo::stderr(&output),
        TestRepo::stdout(&output)
    );

    assert_eq!(TestRepo::stdout(&output), "main\n");

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("warning:") && stderr.contains("Clipboard unavailable"),
        "expected clipboard warning, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("Branch name below:"),
        "expected branch fallback label, got:\n{}",
        stderr
    );
}

#[test]
fn copy_pr_prints_url_and_succeeds_when_clipboard_is_unavailable() {
    let repo = TestRepo::new();
    let remote = repo.git(&[
        "remote",
        "add",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ]);
    remote.assert_success();

    repo.create_stack(&["copy-pr"]);
    let branch = repo.current_branch();
    write_branch_pr_metadata(&repo, &branch, 42);

    let output = run_copy_without_clipboard(&repo, &["copy", "--pr"]);

    assert!(
        output.status.success(),
        "expected stax copy --pr to succeed without clipboard\nstderr:\n{}\nstdout:\n{}",
        TestRepo::stderr(&output),
        TestRepo::stdout(&output)
    );

    assert_eq!(
        TestRepo::stdout(&output),
        "https://github.com/test-owner/test-repo/pull/42\n"
    );

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("warning:") && stderr.contains("Clipboard unavailable"),
        "expected clipboard warning, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("PR URL below:"),
        "expected PR URL fallback label, got:\n{}",
        stderr
    );
}
