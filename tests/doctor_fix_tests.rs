mod common;

use common::{run_stax_in_script_with_env, TestRepo};

#[test]
fn doctor_fix_applies_git_config_repairs_after_single_confirmation() {
    let repo = TestRepo::new_with_remote();
    let init = repo.run_stax(&["init", "--trunk", "main"]);
    assert!(
        init.status.success(),
        "init failed: {}",
        TestRepo::stderr(&init)
    );
    repo.git(&["config", "rerere.enabled", "false"]);
    repo.git(&["config", "rebase.autoStash", "false"]);

    let home = repo.clean_home();
    let git_config = repo.path().join("test-global-gitconfig");
    let git_config_str = git_config.to_string_lossy().into_owned();

    let output = run_stax_in_script_with_env(
        &repo.path(),
        &["doctor", "--fix"],
        "printf 'y\\n'",
        &[("HOME", &home), ("GIT_CONFIG_GLOBAL", &git_config_str)],
    );

    assert!(
        output.status.success(),
        "doctor --fix failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Repair plan"), "stdout was:\n{stdout}");
    assert!(
        stdout.contains("Set git config rerere.enabled=true"),
        "stdout was:\n{stdout}"
    );
    assert!(
        stdout.contains("Set git config rebase.autoStash=true"),
        "stdout was:\n{stdout}"
    );
    assert!(
        stdout.contains("Doctor repair complete"),
        "stdout was:\n{stdout}"
    );

    let config_contents = std::fs::read_to_string(&git_config).expect("git config written");
    assert!(
        config_contents.contains("enabled = true"),
        "config was:\n{config_contents}"
    );
    assert!(
        config_contents.contains("autoStash = true"),
        "config was:\n{config_contents}"
    );
}

#[test]
fn doctor_fix_does_not_apply_repairs_when_confirmation_is_rejected() {
    let repo = TestRepo::new_with_remote();
    let init = repo.run_stax(&["init", "--trunk", "main"]);
    assert!(
        init.status.success(),
        "init failed: {}",
        TestRepo::stderr(&init)
    );
    repo.git(&["config", "rerere.enabled", "false"]);
    repo.git(&["config", "rebase.autoStash", "false"]);

    let home = repo.clean_home();
    let git_config = repo.path().join("test-global-gitconfig");
    let git_config_str = git_config.to_string_lossy().into_owned();

    let output = run_stax_in_script_with_env(
        &repo.path(),
        &["doctor", "--fix"],
        "printf 'n\\n'",
        &[("HOME", &home), ("GIT_CONFIG_GLOBAL", &git_config_str)],
    );

    assert!(
        output.status.success(),
        "doctor --fix failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Repair plan"), "stdout was:\n{stdout}");
    assert!(stdout.contains("No fixes applied"), "stdout was:\n{stdout}");
    assert!(
        !git_config.exists(),
        "git config should not be created when user rejects fixes"
    );
}
