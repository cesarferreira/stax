use crate::common::{TestRepo, stax_bin};
use std::process::{Command, Output};

fn run_copy_without_clipboard(repo: &TestRepo) -> Output {
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let home = repo.clean_home();

    let mut cmd = Command::new(stax_bin());
    cmd.args(["copy"])
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

#[test]
fn copy_prints_branch_and_succeeds_when_clipboard_is_unavailable() {
    let repo = TestRepo::new();

    let output = run_copy_without_clipboard(&repo);

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
