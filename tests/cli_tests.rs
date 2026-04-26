mod common;

use common::{run_stax_in_script_with_env, TestRepo};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Get path to compiled binary (built by cargo test)
fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

fn stax(args: &[&str]) -> std::process::Output {
    Command::new(stax_bin())
        .args(args)
        .output()
        .expect("Failed to execute stax")
}

fn stax_with_home(args: &[&str], home: &std::path::Path) -> std::process::Output {
    Command::new(stax_bin())
        .args(args)
        .env("HOME", home)
        .output()
        .expect("Failed to execute stax")
}

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

const TEST_SHELL: &str = "/bin/bash";

fn shell_rc_path(home: &std::path::Path, shell: &str) -> std::path::PathBuf {
    if shell.ends_with("zsh") {
        home.join(".zshrc")
    } else if shell.ends_with("bash") {
        home.join(".bashrc")
    } else if shell.ends_with("fish") {
        home.join(".config").join("fish").join("config.fish")
    } else {
        home.join(".profile")
    }
}

fn configure_existing_shell_setup(home: &std::path::Path, shell: &str) -> std::path::PathBuf {
    let config_dir = home.join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");

    let snippet_path = config_dir.join("shell-setup.sh");
    let rc_path = shell_rc_path(home, shell);
    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent).expect("create shell rc dir");
    }
    std::fs::write(
        &rc_path,
        format!("source \"{}\" # stax shell-setup\n", snippet_path.display()),
    )
    .expect("write shell rc");

    snippet_path
}

fn write_fake_gh(home: &std::path::Path, token: &str) -> std::path::PathBuf {
    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");

    let gh_path = bin_dir.join("gh");
    std::fs::write(
        &gh_path,
        format!(
            "#!/bin/sh\nif [ \"$1\" = \"auth\" ] && [ \"$2\" = \"token\" ]; then\n  echo \"{token}\"\n  exit 0\nfi\nexit 1\n"
        ),
    )
    .expect("write fake gh");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod fake gh");
    }

    bin_dir
}

fn write_unavailable_gh(home: &std::path::Path) -> std::path::PathBuf {
    let bin_dir = home.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");

    let gh_path = bin_dir.join("gh");
    std::fs::write(&gh_path, "#!/bin/sh\nexit 1\n").expect("write unavailable gh");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755))
            .expect("chmod unavailable gh");
    }

    bin_dir
}

fn path_with_bin(bin_dir: &std::path::Path) -> String {
    let current = std::env::var("PATH").unwrap_or_default();
    if current.is_empty() {
        bin_dir.display().to_string()
    } else {
        format!("{}:{}", bin_dir.display(), current)
    }
}

fn install_test_binary(target: &Path) -> PathBuf {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).expect("create binary parent dir");
    }
    fs::copy(common::stax_bin(), target).expect("copy test binary");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(target, fs::Permissions::from_mode(0o755)).expect("chmod test binary");
    }

    target.to_path_buf()
}

fn write_fake_installer(bin_dir: &Path, name: &str, log_path: &Path, exit_code: i32) {
    fs::create_dir_all(bin_dir).expect("create fake installer dir");
    let installer_path = bin_dir.join(name);
    fs::write(
        &installer_path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit {}\n",
            log_path.display(),
            exit_code
        ),
    )
    .expect("write fake installer");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&installer_path, fs::Permissions::from_mode(0o755))
            .expect("chmod fake installer");
    }
}

fn run_upgrade(binary: &Path, home: &Path, extra_path: &Path) -> std::process::Output {
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    Command::new(binary)
        .args(["cli", "upgrade"])
        .current_dir(home)
        .env("HOME", home)
        .env("PATH", path_with_bin(extra_path))
        .env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run stax cli upgrade")
}

#[test]
fn test_help() {
    let output = stax(&["--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Fast stacked Git branches and PRs"));
    assert!(stdout.contains("status"));
    assert!(stdout.contains("submit"));
    assert!(stdout.contains("run"));
    assert!(stdout.contains("restack"));
    assert!(stdout.contains("resolve"));
}

#[test]
fn test_status_alias_ls() {
    let output1 = stax(&["status", "--help"]);
    let output2 = stax(&["ls", "--help"]);
    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_stack_alias_s() {
    let output = stax(&["s", "--help"]);
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("submit") || String::from_utf8_lossy(&output.stdout).contains("submit")
    );
}

#[test]
fn test_submit_alias_ss() {
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("draft"));
    assert!(stdout.contains("--open"));
    assert!(stdout.contains("reviewers"));
    assert!(stdout.contains("labels"));
    assert!(stdout.contains("assignees"));
    assert!(stdout.contains("no-verify"));
    assert!(stdout.contains("no-prompt"));
    assert!(stdout.contains("yes"));
}

#[test]
fn test_sync_alias_rs() {
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restack")); // --restack option
    assert!(stdout.contains("delete")); // --no-delete option
    assert!(stdout.contains("delete-upstream-gone"));
    assert!(stdout.contains("safe"));
    assert!(stdout.contains("continue"));
}

#[test]
fn test_refresh_help() {
    let output = stax(&["refresh", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Sync trunk"));
    assert!(stdout.contains("no-pr"));
    assert!(stdout.contains("no-submit"));
    assert!(stdout.contains("force"));
    assert!(stdout.contains("safe"));
    assert!(stdout.contains("verbose"));
    assert!(stdout.contains("--yes"));
    assert!(stdout.contains("--no-prompt"));
}

#[test]
fn test_run_command_help() {
    let output = stax(&["run", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Run a command on each branch in the stack"));
    assert!(stdout.contains("--fail-fast"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("--stack"));
    assert!(stdout.contains("--stack[=<STACK>]"));
}

#[test]
fn test_test_command_backcompat_alias_works() {
    let output = stax(&["test", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_merge_help_flags_include_when_ready_mode() {
    let output = stax(&["merge", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--when-ready"));
    assert!(stdout.contains("--remote"));
    assert!(stdout.contains("--interval"));
    assert!(stdout.contains("--no-sync"));
}

#[test]
fn test_merge_when_ready_hidden_alias_still_works() {
    let output = stax(&["merge-when-ready", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_checkout_aliases() {
    // co and bco should both work
    let output1 = stax(&["co", "--help"]);
    let output2 = stax(&["bco", "--help"]);
    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_branch_subcommands() {
    let output = stax(&["branch", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("track"));
    assert!(stdout.contains("untrack"));
    assert!(stdout.contains("delete"));
    assert!(stdout.contains("reparent"));
    assert!(stdout.contains("fold"));
    assert!(stdout.contains("squash"));
    assert!(stdout.contains("up"));
    assert!(stdout.contains("down"));
    assert!(stdout.contains("submit"));
}

#[test]
fn test_bc_shortcut() {
    // bc should work as hidden shortcut
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_bd_shortcut() {
    // bd should work as hidden shortcut
    let output = stax(&["bd", "--help"]);
    assert!(output.status.success());
}

#[test]
fn cli_upgrade_uses_cargo_for_cargo_installs_and_refreshes_shell_setup() {
    let home = tempdir().expect("temp home");
    let installer_bin_dir = home.path().join("bin");
    let cargo_log = home.path().join("cargo.log");
    write_fake_installer(&installer_bin_dir, "cargo", &cargo_log, 0);

    let snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    fs::write(
        &snippet_path,
        "# Generated by stax shell-setup\ncommand stax old-wrapper\n",
    )
    .expect("seed stale shell setup");

    let binary = install_test_binary(&home.path().join(".cargo/bin/stax"));
    let output = run_upgrade(&binary, home.path(), &installer_bin_dir);

    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        fs::read_to_string(&cargo_log).expect("read cargo log"),
        "install\nstax\n"
    );

    let snippet = fs::read_to_string(&snippet_path).expect("read refreshed snippet");
    assert!(snippet.contains("# Generated by stax shell-setup"));
    assert!(!snippet.contains("old-wrapper"));
}

#[test]
fn cli_upgrade_uses_homebrew_for_homebrew_installs() {
    let home = tempdir().expect("temp home");
    let installer_bin_dir = home.path().join("bin");
    let brew_log = home.path().join("brew.log");
    write_fake_installer(&installer_bin_dir, "brew", &brew_log, 0);

    let binary = install_test_binary(&home.path().join("opt/homebrew/bin/stax"));
    let output = run_upgrade(&binary, home.path(), &installer_bin_dir);

    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        fs::read_to_string(&brew_log).expect("read brew log"),
        "upgrade\nstax\n"
    );
}

#[test]
fn cli_upgrade_falls_back_to_generic_upgrade_command() {
    let home = tempdir().expect("temp home");
    let installer_bin_dir = home.path().join("bin");
    let upgrade_log = home.path().join("upgrade.log");
    write_fake_installer(&installer_bin_dir, "upgrade", &upgrade_log, 0);

    let binary = install_test_binary(&home.path().join("custom/bin/stax"));
    let output = run_upgrade(&binary, home.path(), &installer_bin_dir);

    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        fs::read_to_string(&upgrade_log).expect("read upgrade log"),
        "stax\n"
    );
}

#[test]
fn cli_upgrade_surfaces_installer_failures() {
    let home = tempdir().expect("temp home");
    let installer_bin_dir = home.path().join("bin");
    let cargo_log = home.path().join("cargo-fail.log");
    write_fake_installer(&installer_bin_dir, "cargo", &cargo_log, 23);

    let binary = install_test_binary(&home.path().join(".cargo/bin/stax"));
    let output = run_upgrade(&binary, home.path(), &installer_bin_dir);

    assert!(!output.status.success(), "upgrade should fail");
    assert_eq!(
        fs::read_to_string(&cargo_log).expect("read cargo fail log"),
        "install\nstax\n"
    );
}

#[test]
fn test_upstack_commands() {
    let output = stax(&["upstack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("restack"));
    assert!(stdout.contains("submit"));
}

#[test]
fn test_downstack_commands() {
    let output = stax(&["downstack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("get"));
    assert!(stdout.contains("submit"));
}

#[test]
fn test_scoped_submit_subcommand_help_flags() {
    for args in [
        ["branch", "submit", "--help"],
        ["upstack", "submit", "--help"],
        ["downstack", "submit", "--help"],
    ] {
        let output = stax(&args);
        assert!(output.status.success(), "{:?}", args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("--no-pr"), "Expected --no-pr in {:?}", args);
        assert!(
            stdout.contains("--no-fetch"),
            "Expected --no-fetch in {:?}",
            args
        );
        assert!(
            stdout.contains("--no-verify"),
            "Expected --no-verify in {:?}",
            args
        );
        assert!(stdout.contains("--open"), "Expected --open in {:?}", args);
        assert!(stdout.contains("--yes"), "Expected --yes in {:?}", args);
        assert!(
            stdout.contains("--no-prompt"),
            "Expected --no-prompt in {:?}",
            args
        );
    }
}

#[test]
fn test_us_alias() {
    let output = stax(&["us", "--help"]);
    assert!(output.status.success());
}

#[test]
fn test_config_command() {
    let output = stax(&["config"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Config path:"));
    assert!(stdout.contains(".config/stax/config.toml"));
}

#[test]
fn test_config_help_includes_reset_ai_flag() {
    let output = stax(&["config", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--reset-ai"));
    assert!(stdout.contains("--no-prompt"));
    assert!(stdout.contains("--yes"));
}

#[test]
fn test_shell_setup_help_uses_static_install_language() {
    let output = stax(&["setup", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("manual install"));
    assert!(stdout.contains("~/.config/stax"));
    assert!(stdout.contains("--skip-skills"));
    assert!(stdout.contains("--install-skills"));
    assert!(stdout.contains("--skip-auth"));
    assert!(stdout.contains("--auth-from-gh"));
    assert!(stdout.contains("--yes"));
    assert!(!stdout.contains("eval \"$(stax shell-setup)\""));
}

#[test]
fn test_legacy_shell_setup_alias_still_works() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");

    let snippet_path = config_dir.join("shell-setup.sh");
    std::fs::write(
        &snippet_path,
        "# Generated by stax shell-setup\ncommand stax stale-wrapper\n",
    )
    .expect("write stale snippet");

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["shell-setup", "--refresh"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run shell-setup --refresh");

    assert!(output.status.success(), "{:?}", output);

    let refreshed = std::fs::read_to_string(&snippet_path).expect("read refreshed snippet");
    assert!(refreshed.contains("__stax_lookup_path()"));
}

#[test]
fn test_shell_setup_runs_outside_repo() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--print"])
        .current_dir(tmp.path())
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup");

    assert!(output.status.success(), "{:?}", output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("Generated by stax shell-setup"));
    assert!(
        !stdout.contains("Welcome to stax!") && !stderr.contains("Welcome to stax!"),
        "shell-setup should not trigger repo init:\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}

#[test]
fn test_regular_command_auto_refreshes_installed_shell_snippet() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");

    let snippet_path = config_dir.join("shell-setup.sh");
    std::fs::write(
        &snippet_path,
        "# Generated by stax shell-setup\ncommand stax stale-wrapper\n",
    )
    .expect("write stale snippet");

    let output = stax_with_home(&["config"], home.path());
    assert!(output.status.success(), "{:?}", output);

    let refreshed = std::fs::read_to_string(&snippet_path).expect("read refreshed snippet");
    assert!(
        refreshed.contains("__stax_resolve_bin()"),
        "expected startup auto-refresh to rewrite generated snippet, got:\n{}",
        refreshed
    );
    assert!(
        !refreshed.contains("command stax stale-wrapper"),
        "expected stale wrapper to be replaced, got:\n{}",
        refreshed
    );
}

#[test]
fn test_shell_setup_refresh_updates_installed_shell_snippet_outside_repo() {
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");

    let snippet_path = config_dir.join("shell-setup.sh");
    std::fs::write(
        &snippet_path,
        "# Generated by stax shell-setup\ncommand stax stale-wrapper\n",
    )
    .expect("write stale snippet");

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--refresh"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup --refresh");

    assert!(output.status.success(), "{:?}", output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("Welcome to stax!") && !stderr.contains("Welcome to stax!"),
        "shell-setup --refresh should not trigger repo init:\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );

    let refreshed = std::fs::read_to_string(&snippet_path).expect("read refreshed snippet");
    assert!(
        refreshed.contains("__stax_lookup_path()"),
        "expected explicit refresh to rewrite generated snippet, got:\n{}",
        refreshed
    );
    assert!(
        !refreshed.contains("command stax stale-wrapper"),
        "expected stale wrapper to be replaced, got:\n{}",
        refreshed
    );
}

#[test]
fn test_shell_setup_rejects_conflicting_skill_flags() {
    let output = stax(&["setup", "--skip-skills", "--install-skills"]);
    assert!(!output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--skip-skills"));
    assert!(stderr.contains("--install-skills"));
}

#[test]
fn test_shell_setup_noninteractive_default_skips_skills_install() {
    let home = tempdir().expect("temp home");
    let snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let unavailable_gh_bin = write_unavailable_gh(home.path());

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(&unavailable_gh_bin))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup");

    assert!(output.status.success(), "{:?}", output);
    assert!(
        snippet_path.exists(),
        "expected shell snippet to be refreshed"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("Install stax AI agent skills"),
        "non-interactive setup should not prompt for skills:\n{}",
        stdout
    );
    assert!(
        !home.path().join(".codex/skills/stax/SKILL.md").exists(),
        "skills should not be installed without prompt/flag"
    );
}

#[tokio::test]
async fn test_shell_setup_install_skills_flag_installs_skills() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let _snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let unavailable_gh_bin = write_unavailable_gh(home.path());

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--install-skills"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(&unavailable_gh_bin))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("run setup --install-skills");

    assert!(output.status.success(), "{:?}", output);

    let codex_skill = home.path().join(".codex/skills/stax/SKILL.md");
    assert!(codex_skill.exists(), "expected Codex skill to be installed");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("stax skills update"),
        "expected setup to run skills installer:\n{}",
        stdout
    );
}

#[tokio::test]
async fn test_shell_setup_interactive_prompt_can_decline_skills_install() {
    ensure_crypto_provider();
    let home = tempdir().expect("temp home");
    let _snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let cwd = tempfile::tempdir().expect("create temp dir");
    let home_str = home.path().to_str().expect("home path");
    let unavailable_gh_bin = write_unavailable_gh(home.path());
    let path_buf = path_with_bin(&unavailable_gh_bin);

    let output = run_stax_in_script_with_env(
        cwd.path(),
        &["setup"],
        "printf 'n\\n'",
        &[
            ("HOME", home_str),
            ("SHELL", TEST_SHELL),
            ("PATH", &path_buf),
        ],
    );

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Install stax AI agent skills"),
        "expected interactive setup to ask about skills:\n{}",
        combined
    );
    assert!(
        !home.path().join(".codex/skills/stax/SKILL.md").exists(),
        "declining the prompt should not install skills"
    );
}

#[test]
fn test_shell_setup_rejects_conflicting_auth_flags() {
    let output = stax(&["setup", "--skip-auth", "--auth-from-gh"]);
    assert!(!output.status.success(), "{:?}", output);

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--skip-auth"));
    assert!(stderr.contains("--auth-from-gh"));
}

#[tokio::test]
async fn test_shell_setup_yes_installs_skills_and_imports_auth_from_gh() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let _snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let fake_gh_bin = write_fake_gh(home.path(), "gh-imported-token");

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--yes"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(&fake_gh_bin))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("run setup --yes");

    assert!(output.status.success(), "{:?}", output);

    let credentials_path = home.path().join(".config/stax/.credentials");
    let saved = std::fs::read_to_string(&credentials_path).expect("read credentials");
    assert_eq!(saved.trim(), "gh-imported-token");
    assert!(
        home.path().join(".codex/skills/stax/SKILL.md").exists(),
        "expected skills to be installed"
    );
}

#[test]
fn test_shell_setup_yes_without_gh_prints_auth_next_steps() {
    let home = tempdir().expect("temp home");
    let _snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let unavailable_gh_bin = write_unavailable_gh(home.path());

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--yes", "--skip-skills"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(&unavailable_gh_bin))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup --yes --skip-skills");

    assert!(output.status.success(), "{:?}", output);

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("gh auth login"),
        "missing gh hint:\n{}",
        combined
    );
    assert!(
        combined.contains("st auth --from-gh"),
        "missing st auth import hint:\n{}",
        combined
    );
    assert!(
        combined.contains("st auth"),
        "missing manual auth hint:\n{}",
        combined
    );
    assert!(
        !home.path().join(".config/stax/.credentials").exists(),
        "should not create credentials without gh or explicit auth"
    );
}

#[test]
fn test_shell_setup_yes_auto_accepts_shell_install_prompt() {
    let home = tempdir().expect("temp home");
    let unavailable_gh_bin = write_unavailable_gh(home.path());

    let tmp = tempfile::tempdir().expect("create temp dir");
    let output = Command::new(stax_bin())
        .args(["setup", "--yes", "--skip-skills", "--skip-auth"])
        .current_dir(tmp.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(&unavailable_gh_bin))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup --yes --skip-skills --skip-auth");

    assert!(output.status.success(), "{:?}", output);
    assert!(
        shell_rc_path(home.path(), TEST_SHELL).exists(),
        "expected --yes to install shell integration without prompting"
    );
}

#[test]
fn test_shell_setup_interactive_prompt_can_decline_auth_import_from_gh() {
    let home = tempdir().expect("temp home");
    let _snippet_path = configure_existing_shell_setup(home.path(), TEST_SHELL);
    let fake_gh_bin = write_fake_gh(home.path(), "gh-imported-token");
    let cwd = tempfile::tempdir().expect("create temp dir");
    let home_str = home.path().to_str().expect("home path");
    let path_buf = path_with_bin(&fake_gh_bin);

    let output = run_stax_in_script_with_env(
        cwd.path(),
        &["setup", "--skip-skills"],
        "printf 'n\\n'",
        &[
            ("HOME", home_str),
            ("SHELL", TEST_SHELL),
            ("PATH", &path_buf),
        ],
    );

    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("Import GitHub auth from `gh` now?"),
        "expected auth import prompt:\n{}",
        combined
    );
    assert!(
        !home.path().join(".config/stax/.credentials").exists(),
        "declining gh import should not save credentials"
    );
}

#[test]
fn test_bare_worktree_command_falls_back_when_input_reader_probe_fails() {
    let repo = TestRepo::new();
    let home = tempdir().expect("temp home");

    let output = repo.run_stax_with_env(
        &["wt"],
        &[
            ("HOME", home.path().to_str().expect("home path")),
            ("STAX_TEST_FORCE_INTERACTIVE_TERMINAL", "1"),
            (
                "STAX_TEST_FORCE_INPUT_READER_FAILURE",
                "Failed to initialize input reader",
            ),
        ],
    );
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");
    assert!(
        combined.contains("interactive worktree dashboard unavailable"),
        "expected fallback warning, got:\n{}",
        combined
    );
    assert!(
        combined.contains("Failed to initialize input reader"),
        "expected crossterm probe reason, got:\n{}",
        combined
    );
    assert!(
        combined.contains("Usage: worktree [COMMAND]"),
        "expected worktree help output, got:\n{}",
        combined
    );
}

#[test]
fn test_generate_help_includes_no_prompt_flag() {
    let output = stax(&["generate", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--pr-body"));
    assert!(stdout.contains("--no-prompt"));
    assert!(stdout.contains("--edit"));
    assert!(stdout.contains("--template"));
    assert!(stdout.contains("--no-template"));
}

#[test]
fn test_config_reset_ai_no_prompt_clears_saved_defaults() {
    let temp_dir = std::env::temp_dir().join(format!(
        "stax-cli-test-config-reset-ai-{}",
        std::process::id()
    ));
    let config_dir = temp_dir.join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).unwrap();
    std::fs::write(
        config_dir.join("config.toml"),
        "[ai]\nagent = \"codex\"\nmodel = \"gpt-5.3-codex\"\n",
    )
    .unwrap();

    let output = stax_with_home(&["config", "--reset-ai", "--no-prompt", "--yes"], &temp_dir);
    assert!(output.status.success(), "{:?}", output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cleared saved AI defaults"));
    assert!(stdout.contains("Skipped reconfiguration"));

    let updated = std::fs::read_to_string(config_dir.join("config.toml")).unwrap();
    assert!(!updated.contains("agent = \"codex\""));
    assert!(!updated.contains("model = \"gpt-5.3-codex\""));

    let _ = std::fs::remove_dir_all(temp_dir);
}

#[test]
fn test_init_help_includes_trunk_flag() {
    let output = stax(&["init", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialize stax"));
    assert!(stdout.contains("--trunk"));
}

#[test]
fn test_status_help_flags() {
    let output = stax(&["status", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("json"));
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
    assert!(stdout.contains("compact"));
    assert!(stdout.contains("quiet"));
}

#[test]
fn test_log_help_flags() {
    let output = stax(&["log", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("json"));
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
    assert!(stdout.contains("compact"));
    assert!(stdout.contains("quiet"));
}

#[test]
fn test_restack_help_flags() {
    let output = stax(&["restack", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("continue"));
    assert!(stdout.contains("stop-here"));
    assert!(stdout.contains("quiet"));
    assert!(stdout.contains("stop-here"));
    assert!(stdout.contains("submit-after"));
    assert!(stdout.contains("ask"));
    assert!(stdout.contains("yes"));
    assert!(stdout.contains("no"));
}

#[test]
fn test_resolve_help_flags() {
    let output = stax(&["resolve", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--agent"));
    assert!(stdout.contains("--model"));
    assert!(stdout.contains("--max-rounds"));
}

#[test]
fn test_checkout_help_flags() {
    let output = stax(&["checkout", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("trunk"));
    assert!(stdout.contains("parent"));
    assert!(stdout.contains("child"));
}

#[test]
fn test_branch_create_help_flags() {
    let output = stax(&["branch", "create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("from"));
    assert!(stdout.contains("prefix"));
    assert!(stdout.contains("-n"));
    assert!(stdout.contains("--no-verify"));
}

#[test]
fn test_diff_help_flags() {
    let output = stax(&["diff", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
}

#[test]
fn test_range_diff_help_flags() {
    let output = stax(&["range-diff", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stack"));
    assert!(stdout.contains("all"));
}

#[test]
fn test_doctor_help() {
    let output = stax(&["doctor", "--help"]);
    assert!(output.status.success());
}

// ============================================================================
// Freephite (fp) Command Parity Tests
// These tests ensure stax maintains compatibility with freephite commands
// ============================================================================

#[test]
fn fp_parity_ss_submit_stack() {
    // fp ss -> stax ss (submit stack)
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Submit stack"));
}

#[test]
fn fp_parity_bs_branch_submit() {
    // fp bs -> stax bs (branch submit)
    let output = stax(&["bs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--no-pr"));
    assert!(stdout.contains("--no-fetch"));
    assert!(stdout.contains("--open"));
    assert!(stdout.contains("--no-prompt"));
}

#[test]
fn fp_parity_rs_repo_sync() {
    // fp rs -> stax rs (repo sync)
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Sync repo"));
}

#[test]
fn fp_parity_bc_branch_create() {
    // fp bc -> stax bc (branch create)
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("message")); // -m flag
    assert!(stdout.contains("from")); // --from flag
    assert!(stdout.contains("no-verify")); // -n/--no-verify flag
}

#[test]
fn fp_parity_bco_branch_checkout() {
    // fp bco -> stax bco (branch checkout)
    let output = stax(&["bco", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_bu_branch_up() {
    // fp bu -> stax bu (branch up)
    let output = stax(&["bu", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COUNT")); // supports count argument
}

#[test]
fn fp_parity_bd_branch_down() {
    // fp bd -> stax bd (branch down)
    let output = stax(&["bd", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("COUNT")); // supports count argument
}

#[test]
fn fp_parity_s_status() {
    // fp s -> stax s (status)
    let output = stax(&["s", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_ls_status() {
    // fp ls -> stax ls (status/list)
    let output = stax(&["ls", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_l_log() {
    // fp l -> stax l (log)
    let output = stax(&["l", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_co_checkout() {
    // fp co -> stax co (checkout)
    let output = stax(&["co", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_cont_continue() {
    // fp cont -> stax cont (continue)
    let output = stax(&["cont", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_branch() {
    // fp b -> stax b (branch subcommand)
    let output = stax(&["b", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("create"));
    assert!(stdout.contains("checkout"));
    assert!(stdout.contains("track"));
    assert!(stdout.contains("untrack"));
    assert!(stdout.contains("delete"));
}

#[test]
fn fp_parity_b_c_branch_create() {
    // fp b c -> stax b c (branch create)
    let output = stax(&["b", "c", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_co_branch_checkout() {
    // fp b co -> stax b co (branch checkout)
    let output = stax(&["b", "co", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_d_branch_delete() {
    // fp b d -> stax b d (branch delete)
    let output = stax(&["b", "d", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_b_u_branch_up() {
    // fp b u -> stax b u (branch up)
    let output = stax(&["b", "u", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_us_upstack() {
    // fp us -> stax us (upstack)
    let output = stax(&["us", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_ds_downstack() {
    // fp ds -> stax ds (downstack)
    let output = stax(&["ds", "--help"]);
    assert!(output.status.success());
}

#[test]
fn fp_parity_bc_with_message() {
    // fp bc -m "message" -> stax bc -m "message"
    let _output = stax(&["bc", "-m", "test", "--help"]);
    // This tests that -m is a valid flag (help still shows)
    let output2 = stax(&["bc", "--help"]);
    assert!(output2.status.success());
    let stdout = String::from_utf8_lossy(&output2.stdout);
    assert!(stdout.contains("-m"));
    assert!(stdout.contains("--message"));
}

#[test]
fn fp_parity_bc_with_all_flag() {
    // fp bc -a -> stax bc -a (stage all changes)
    let output = stax(&["bc", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-a"));
    assert!(stdout.contains("--all"));
}

#[test]
fn fp_parity_rs_restack_flag() {
    // fp rs --restack -> stax rs --restack
    let output = stax(&["rs", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--restack"));
    assert!(stdout.contains("-r")); // short flag
}

#[test]
fn fp_parity_ss_draft_flag() {
    // fp ss --draft -> stax ss --draft
    let output = stax(&["ss", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--draft"));
    assert!(stdout.contains("-d")); // short flag
}

// ============================================================================
// Graphite (gt) Command Parity Tests
// These tests ensure stax also supports graphite-style commands
// ============================================================================

#[test]
fn gt_parity_create_command() {
    // gt create -> stax create
    let output = stax(&["create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Create a new branch"));
}

#[test]
fn gt_parity_c_alias() {
    // gt c -> stax c (create alias)
    let output = stax(&["c", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_create_am_flags() {
    // gt create -am "message" -> stax create -am "message"
    let output = stax(&["create", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-a"));
    assert!(stdout.contains("--all"));
    assert!(stdout.contains("-m"));
    assert!(stdout.contains("--message"));
    assert!(stdout.contains("-n"));
    assert!(stdout.contains("--no-verify"));
}

#[test]
fn gt_parity_modify_command() {
    // gt modify -> stax modify
    let output = stax(&["modify", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("amend"));
}

#[test]
fn gt_parity_m_alias() {
    // gt m -> stax m (modify alias)
    let output = stax(&["m", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_up_command() {
    // gt up -> stax up
    let output = stax(&["up", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Move up"));
    assert!(stdout.contains("COUNT"));
}

#[test]
fn gt_parity_u_alias() {
    // gt u -> stax u (up alias)
    let output = stax(&["u", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_down_command() {
    // gt down -> stax down
    let output = stax(&["down", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Move down"));
    assert!(stdout.contains("COUNT"));
}

#[test]
fn gt_parity_d_alias() {
    // gt d -> stax d (down alias)
    let output = stax(&["d", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_top_command() {
    // gt top -> stax top
    let output = stax(&["top", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("top") || stdout.contains("tip"));
}

#[test]
fn gt_parity_bottom_command() {
    // gt bottom -> stax bottom
    let output = stax(&["bottom", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("bottom") || stdout.contains("base"));
}

#[test]
fn gt_parity_trunk_command() {
    // gt checkout --trunk -> stax trunk (or stax t)
    let output = stax(&["trunk", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_t_alias() {
    // stax t -> trunk
    let output = stax(&["t", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_pr_command() {
    // gt pr -> stax pr (open PR in browser)
    let output = stax(&["pr", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("open"));
    assert!(stdout.contains("list"));
}

#[test]
fn gt_parity_pr_open_subcommand() {
    let output = stax(&["pr", "open", "--help"]);
    assert!(output.status.success());
}

#[test]
fn gt_parity_pr_list_subcommand() {
    let output = stax(&["pr", "list", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--json"));
}

#[test]
fn gt_parity_issue_list_subcommand() {
    let output = stax(&["issue", "list", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--limit"));
    assert!(stdout.contains("--json"));
}

#[test]
fn gt_parity_submit_command() {
    // gt submit -> stax submit
    let output = stax(&["submit", "--help"]);
    assert!(output.status.success());
}

// ============================================================================
// Rename Command Tests
// ============================================================================

#[test]
fn test_rename_help() {
    let output = stax(&["rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rename"));
    assert!(stdout.contains("--edit"));
}

#[test]
fn test_branch_rename_help() {
    let output = stax(&["branch", "rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Rename"));
}

#[test]
fn test_branch_rename_alias() {
    // b r should work as alias
    let output = stax(&["b", "r", "--help"]);
    assert!(output.status.success());
}

// ============================================================================
// LL Command Tests
// ============================================================================

#[test]
fn test_ll_command_help() {
    let output = stax(&["ll", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("PR") || stdout.contains("details") || stdout.contains("full"));
}

#[test]
fn test_ll_command_flags() {
    let output = stax(&["ll", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--json"));
    assert!(stdout.contains("--stack"));
    assert!(stdout.contains("--current"));
    assert!(stdout.contains("--compact"));
    assert!(stdout.contains("--quiet"));
}

// ============================================================================
// Rename --push Flag Tests
// ============================================================================

#[test]
fn test_rename_push_flag_help() {
    let output = stax(&["rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--push") || stdout.contains("-p"),
        "Expected --push flag in rename help: {}",
        stdout
    );
}

#[test]
fn test_branch_rename_push_flag_help() {
    let output = stax(&["branch", "rename", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("--push") || stdout.contains("-p"),
        "Expected --push flag in branch rename help: {}",
        stdout
    );
}

// ============================================================================
// CI Command Tests
// ============================================================================

#[test]
fn test_ci_command_help() {
    let output = stax(&["ci", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("CI") || stdout.contains("status"),
        "Expected CI-related help text: {}",
        stdout
    );
}

#[test]
fn test_ci_command_flags() {
    let output = stax(&["ci", "--help"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--all"), "Expected --all flag: {}", stdout);
    assert!(
        stdout.contains("--json"),
        "Expected --json flag: {}",
        stdout
    );
    assert!(
        stdout.contains("--refresh"),
        "Expected --refresh flag: {}",
        stdout
    );
}

#[test]
fn test_submit_short_dash_f_is_rejected() {
    let output = stax(&["ss", "-f", "--help"]);
    assert!(
        !output.status.success(),
        "expected `stax ss -f` to fail clap parsing after -f short is removed; \
         stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument") && stderr.contains("-f"),
        "expected clap error mentioning -f, got: {stderr}"
    );
}

#[test]
fn test_submit_long_force_emits_deprecation_warning() {
    let tmp = tempdir().expect("tempdir");
    let output = Command::new(stax_bin())
        .args(["ss", "--force", "--no-fetch", "--no-prompt", "--yes"])
        .current_dir(tmp.path())
        .output()
        .expect("Failed to execute stax");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("--force is deprecated"),
        "expected deprecation warning when --force is passed, got:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
