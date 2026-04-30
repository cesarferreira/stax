mod common;

use common::TestRepo;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_test_config_with_ai(home: &Path) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        r#"
[ai.generate]
agent = "claude"
"#,
    )
    .expect("write config");
}

fn write_fake_claude(home: &Path, response: &str, prompt_log: &Path) -> PathBuf {
    let bin_dir = home.join("bin");
    fs::create_dir_all(&bin_dir).expect("create bin dir");

    let claude = bin_dir.join("claude");
    fs::write(
        &claude,
        format!(
            "#!/bin/sh\ncat > \"{}\"\nprintf '%s\\n' '{}'\n",
            prompt_log.display(),
            response.replace('\'', "'\"'\"'")
        ),
    )
    .expect("write fake claude");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&claude, fs::Permissions::from_mode(0o755)).expect("chmod fake claude");
    }

    bin_dir
}

fn path_with_bin(bin_dir: &Path) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    if current.is_empty() {
        bin_dir.display().to_string()
    } else {
        format!("{}:{}", bin_dir.display(), current)
    }
}

fn ai_env(home: &Path, bin_dir: &Path) -> Vec<(String, String)> {
    vec![
        ("HOME".to_string(), home.display().to_string()),
        ("PATH".to_string(), path_with_bin(bin_dir)),
    ]
}

fn env_refs(env: &[(String, String)]) -> Vec<(&str, &str)> {
    env.iter()
        .map(|(key, value)| (key.as_str(), value.as_str()))
        .collect()
}

fn current_subject(repo: &TestRepo) -> String {
    let output = repo.git(&["log", "-1", "--format=%s"]);
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

#[test]
fn create_ai_all_yes_uses_generated_branch_and_commit_message() {
    let repo = TestRepo::new();
    repo.create_file("src/lib.rs", "pub fn answer() -> i32 { 42 }\n");

    let home = TempDir::new().expect("create home");
    write_test_config_with_ai(home.path());
    let prompt_log = home.path().join("prompt.txt");
    let bin_dir = write_fake_claude(
        home.path(),
        r#"{"branch":"add-answer-helper","message":"Add answer helper"}"#,
        &prompt_log,
    );
    let env = ai_env(home.path(), &bin_dir);
    let env = env_refs(&env);

    let output = repo.run_stax_with_env(&["create", "--ai", "-a", "--yes"], &env);

    assert!(
        output.status.success(),
        "create failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert_eq!(repo.current_branch(), "add-answer-helper");
    assert_eq!(current_subject(&repo), "Add answer helper");

    let prompt = fs::read_to_string(prompt_log).expect("read prompt");
    assert!(prompt.contains("\"branch\" and \"message\""));
    assert!(prompt.contains("src/lib.rs"));
}

#[test]
fn create_named_ai_all_yes_uses_name_and_generated_commit_message() {
    let repo = TestRepo::new();
    repo.create_file("src/lib.rs", "pub fn feature() {}\n");

    let home = TempDir::new().expect("create home");
    write_test_config_with_ai(home.path());
    let prompt_log = home.path().join("prompt.txt");
    let bin_dir = write_fake_claude(
        home.path(),
        r#"{"message":"Add named feature"}"#,
        &prompt_log,
    );
    let env = ai_env(home.path(), &bin_dir);
    let env = env_refs(&env);

    let output = repo.run_stax_with_env(&["create", "manual-feature", "--ai", "-a", "--yes"], &env);

    assert!(
        output.status.success(),
        "create failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert_eq!(repo.current_branch(), "manual-feature");
    assert_eq!(current_subject(&repo), "Add named feature");

    let prompt = fs::read_to_string(prompt_log).expect("read prompt");
    assert!(prompt.contains("\"message\""));
    assert!(!prompt.contains("\"branch\" and \"message\""));
}

#[test]
fn create_ai_manual_message_yes_uses_generated_branch_and_manual_commit_message() {
    let repo = TestRepo::new();
    repo.create_file("src/lib.rs", "pub fn manual_message() {}\n");

    let home = TempDir::new().expect("create home");
    write_test_config_with_ai(home.path());
    let prompt_log = home.path().join("prompt.txt");
    let bin_dir = write_fake_claude(
        home.path(),
        r#"{"branch":"manual-message-branch"}"#,
        &prompt_log,
    );
    let env = ai_env(home.path(), &bin_dir);
    let env = env_refs(&env);

    let output = repo.run_stax_with_env(
        &["create", "--ai", "-a", "-m", "Keep manual message", "--yes"],
        &env,
    );

    assert!(
        output.status.success(),
        "create failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert_eq!(repo.current_branch(), "manual-message-branch");
    assert_eq!(current_subject(&repo), "Keep manual message");

    let prompt = fs::read_to_string(prompt_log).expect("read prompt");
    assert!(prompt.contains("\"branch\""));
    assert!(!prompt.contains("\"branch\" and \"message\""));
}

#[test]
fn create_ai_yes_without_staging_generates_branch_only() {
    let repo = TestRepo::new();
    let original_head = repo.head_sha();
    repo.create_file("src/lib.rs", "pub fn uncommitted() {}\n");

    let home = TempDir::new().expect("create home");
    write_test_config_with_ai(home.path());
    let prompt_log = home.path().join("prompt.txt");
    let bin_dir = write_fake_claude(home.path(), r#"{"branch":"branch-only-ai"}"#, &prompt_log);
    let env = ai_env(home.path(), &bin_dir);
    let env = env_refs(&env);

    let output = repo.run_stax_with_env(&["create", "--ai", "--yes"], &env);

    assert!(
        output.status.success(),
        "create failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert_eq!(repo.current_branch(), "branch-only-ai");
    assert_eq!(repo.head_sha(), original_head);

    let status = repo.git(&["status", "--short", "--untracked-files=all"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(status.contains("src/lib.rs"));

    let prompt = fs::read_to_string(prompt_log).expect("read prompt");
    assert!(prompt.contains("\"branch\""));
    assert!(!prompt.contains("\"message\""));
}
