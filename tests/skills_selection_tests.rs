//! Integration tests for per-harness skills selection during setup/update.

use crate::common::stax_bin;
use std::process::Command;
use tempfile::{TempDir, tempdir};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

fn configure_existing_shell_setup(home: &std::path::Path) -> std::path::PathBuf {
    let config_dir = home.join(".config").join("stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let snippet_path = config_dir.join("shell-setup.sh");
    let rc_path = shell_rc_path(home, TEST_SHELL);
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

fn write_unavailable_gh(home: &std::path::Path) -> TempDir {
    let bin_dir = tempdir().expect("bin dir");
    let gh_path = bin_dir.path().join("gh");
    std::fs::write(&gh_path, "#!/bin/sh\nexit 127\n").expect("write fake gh");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&gh_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let _ = home;
    bin_dir
}

fn path_with_bin(bin_dir: &std::path::Path) -> String {
    let path = std::env::var("PATH").unwrap_or_default();
    format!("{}:{}", bin_dir.display(), path)
}

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[tokio::test]
async fn setup_install_skills_flag_with_skills_list_installs_subset() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let _snippet = configure_existing_shell_setup(home.path());
    let gh_bin = write_unavailable_gh(home.path());

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let repo = tempdir().expect("repo");
    let output = Command::new(stax_bin())
        .args([
            "setup",
            "--yes",
            "--install-skills",
            "--skills",
            "codex,cursor",
        ])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(gh_bin.path()))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("run setup");

    assert!(output.status.success(), "{:?}", output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("installed:"),
        "stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    assert!(home.path().join(".codex/skills/stax/SKILL.md").exists());
    assert!(home.path().join(".cursor/skills/stax/SKILL.md").exists());
    assert!(!home.path().join(".claude/skills/stax/SKILL.md").exists());
    assert!(!home.path().join(".pi/agent/skills/stax/SKILL.md").exists());
    assert!(
        !home
            .path()
            .join(".config/opencode/skills/stax/SKILL.md")
            .exists()
    );

    let config =
        std::fs::read_to_string(home.path().join(".config/stax/config.toml")).expect("read config");
    assert!(config.contains("harnesses"));
    assert!(config.contains("codex"));
    assert!(config.contains("cursor"));
}

#[tokio::test]
async fn setup_yes_installs_skills_only_for_detected_harnesses() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let _snippet = configure_existing_shell_setup(home.path());
    std::fs::create_dir_all(home.path().join(".codex")).expect("create codex dir");
    let gh_bin = write_unavailable_gh(home.path());

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let repo = tempdir().expect("repo");
    let output = Command::new(stax_bin())
        .args(["setup", "--yes"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(gh_bin.path()))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("run setup --yes");

    assert!(output.status.success(), "{:?}", output);
    assert!(home.path().join(".codex/skills/stax/SKILL.md").exists());
    assert!(!home.path().join(".cursor/skills/stax/SKILL.md").exists());
}

#[test]
fn setup_skills_unknown_harness_fails() {
    let home = tempdir().expect("temp home");
    let _snippet = configure_existing_shell_setup(home.path());
    let repo = tempdir().expect("repo");

    let output = Command::new(stax_bin())
        .args(["setup", "--skills", "bogus"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup");

    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("bogus"));
    assert!(combined.contains("codex"));
}

#[tokio::test]
async fn setup_skills_none_writes_nothing() {
    ensure_crypto_provider();
    let home = tempdir().expect("temp home");
    let _snippet = configure_existing_shell_setup(home.path());
    let gh_bin = write_unavailable_gh(home.path());
    let repo = tempdir().expect("repo");

    let output = Command::new(stax_bin())
        .args(["setup", "--yes", "--skills", "none"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("SHELL", TEST_SHELL)
        .env("PATH", path_with_bin(gh_bin.path()))
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .output()
        .expect("run setup");

    assert!(output.status.success(), "{:?}", output);
    assert!(!home.path().join(".codex/skills/stax/SKILL.md").exists());
}

#[tokio::test]
async fn skills_update_respects_configured_harnesses() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".config/stax");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("config.toml"),
        "[skills]\nharnesses = [\"codex\"]\n",
    )
    .expect("write config");
    let installed_skill = home.path().join(".codex/skills/stax/SKILL.md");
    std::fs::create_dir_all(installed_skill.parent().expect("skill parent"))
        .expect("create skill dir");
    std::fs::write(
        &installed_skill,
        "<!-- stax-skills-version: 0.50.0 -->\n# Old Stax Skills\n",
    )
    .expect("write outdated skill");

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let repo = tempdir().expect("repo");
    let output = Command::new(stax_bin())
        .args(["skills", "update"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("STAX_CONFIG_DIR", &config_dir)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("skills update");

    assert!(output.status.success(), "{:?}", output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("updated:"),
        "stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(home.path().join(".codex/skills/stax/SKILL.md").exists());
    assert!(!home.path().join(".cursor/skills/stax/SKILL.md").exists());
}

#[tokio::test]
async fn skills_update_all_overrides_configured_selection() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    let home = tempdir().expect("temp home");
    let config_dir = home.path().join(".config/stax");
    std::fs::create_dir_all(&config_dir).expect("config dir");
    std::fs::write(
        config_dir.join("config.toml"),
        "[skills]\nharnesses = [\"codex\"]\n",
    )
    .expect("write config");

    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n"),
        )
        .mount(&mock_server)
        .await;

    let repo = tempdir().expect("repo");
    let output = Command::new(stax_bin())
        .args(["skills", "update", "--all"])
        .current_dir(repo.path())
        .env("HOME", home.path())
        .env("STAX_CONFIG_DIR", &config_dir)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env(
            "STAX_SKILLS_URL",
            format!("{}/skills.md", mock_server.uri()),
        )
        .output()
        .expect("skills update --all");

    assert!(output.status.success(), "{:?}", output);
    assert!(home.path().join(".codex/skills/stax/SKILL.md").exists());
    assert!(
        home.path()
            .join(".config/opencode/skills/stax/SKILL.md")
            .exists()
    );
    assert!(home.path().join(".claude/skills/stax/SKILL.md").exists());
    assert!(home.path().join(".cursor/skills/stax/SKILL.md").exists());
    assert!(home.path().join(".pi/agent/skills/stax/SKILL.md").exists());
}
