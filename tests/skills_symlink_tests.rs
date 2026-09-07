//! Integration tests for broken-symlink repair in `stax skills update`.

use crate::common::stax_bin;
use std::process::Command;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SKILLS_BODY: &str = "<!-- stax-skills-version: 0.51.0 -->\n# Stax Skills\n";

fn ensure_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn base_cmd(home: &std::path::Path, skills_url: &str, args: &[&str]) -> Command {
    let config_dir = home.join(".config/stax");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let mut cmd = Command::new(stax_bin());
    cmd.args(args)
        .current_dir(home)
        .env("HOME", home)
        .env("STAX_CONFIG_DIR", &config_dir)
        .env("STAX_DISABLE_UPDATE_CHECK", "1")
        .env("STAX_SKILLS_URL", skills_url);
    cmd
}

#[cfg(unix)]
#[tokio::test]
async fn skills_update_replaces_dangling_skill_dir_symlink() {
    use std::os::unix::fs::symlink;

    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SKILLS_BODY))
        .mount(&mock_server)
        .await;

    let home_dir = tempdir().expect("home");
    let home = home_dir.path();
    let skills_dir = home.join(".codex/skills");
    std::fs::create_dir_all(&skills_dir).expect("create skills dir");
    // Create a dangling symlink: .codex/skills/stax → missing target
    let link = skills_dir.join("stax");
    symlink(home.join("missing-dotfiles/stax"), &link).expect("create broken symlink");

    let output = base_cmd(
        home,
        &format!("{}/skills.md", mock_server.uri()),
        &["skills", "update", "--skills", "codex"],
    )
    .output()
    .expect("run skills update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    assert!(
        output.status.success(),
        "exit status: {:?}\n{combined}",
        output.status
    );

    let skill_file = home.join(".codex/skills/stax/SKILL.md");
    // The symlink should have been removed and replaced with a real directory.
    assert!(
        !home.join(".codex/skills/stax").is_symlink(),
        "stax should be a real dir, not a symlink"
    );
    assert!(
        home.join(".codex/skills/stax").is_dir(),
        "stax should now be a real directory"
    );
    assert!(skill_file.exists(), "SKILL.md should exist");
    let content = std::fs::read_to_string(&skill_file).expect("read SKILL.md");
    assert!(content.contains("# Stax Skills"), "content: {content}");
    assert!(
        stdout.contains("replaced broken symlink"),
        "stdout: {stdout}"
    );
    assert!(
        !combined.contains("File exists"),
        "should not contain 'File exists': {combined}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_update_writes_through_valid_dir_symlink() {
    use std::os::unix::fs::symlink;

    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SKILLS_BODY))
        .mount(&mock_server)
        .await;

    let home_dir = tempdir().expect("home");
    let home = home_dir.path();
    // Real target dir that the symlink will point at.
    let target = home.join("dotfiles/stax-skill");
    std::fs::create_dir_all(&target).expect("create target");
    let skills_dir = home.join(".codex/skills");
    std::fs::create_dir_all(&skills_dir).expect("create skills dir");
    // Valid symlink: .codex/skills/stax → ~/dotfiles/stax-skill
    let link = skills_dir.join("stax");
    symlink(&target, &link).expect("create valid symlink");

    let output = base_cmd(
        home,
        &format!("{}/skills.md", mock_server.uri()),
        &["skills", "update", "--skills", "codex"],
    )
    .output()
    .expect("run skills update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "exit status: {:?}\n{combined}",
        output.status
    );

    // The symlink must still be a symlink (not replaced).
    assert!(
        link.is_symlink(),
        ".codex/skills/stax should still be a symlink"
    );
    // The file should be written through the symlink into the real target.
    assert!(
        target.join("SKILL.md").exists(),
        "SKILL.md should exist inside the target dir"
    );
    assert!(
        !stdout.contains("replaced broken symlink"),
        "must not report replacing a valid symlink; stdout: {stdout}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_update_continues_after_harness_failure_and_exits_nonzero() {
    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SKILLS_BODY))
        .mount(&mock_server)
        .await;

    let home_dir = tempdir().expect("home");
    let home = home_dir.path();
    // Force a non-symlink failure for the first harness (Codex):
    // create .codex/skills as a real dir, then write a regular *file* named
    // .codex/skills/stax — create_dir_all on it will fail with ENOTDIR.
    std::fs::create_dir_all(home.join(".codex/skills")).expect("create skills dir");
    std::fs::write(home.join(".codex/skills/stax"), "not a dir").expect("write obstacle");

    let output = base_cmd(
        home,
        &format!("{}/skills.md", mock_server.uri()),
        &["skills", "update", "--all"],
    )
    .output()
    .expect("run skills update --all");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}{stderr}");

    // Overall exit code must be non-zero.
    assert!(
        !output.status.success(),
        "should exit non-zero; combined: {combined}"
    );

    // Codex failure must be reported.
    assert!(
        combined.contains("Codex"),
        "should mention Codex; combined: {combined}"
    );
    assert!(
        combined.contains("failed"),
        "should mention failed; combined: {combined}"
    );
    assert!(
        stderr.contains("harness(es) failed to update"),
        "stderr: {stderr}"
    );

    // The obstacle file must not have been deleted (our repair only removes symlinks).
    assert!(
        home.join(".codex/skills/stax").is_file(),
        ".codex/skills/stax should remain a regular file"
    );

    // All four later harnesses must have been written successfully.
    assert!(
        home.join(".config/opencode/skills/stax/SKILL.md").exists(),
        "opencode skill missing"
    );
    assert!(
        home.join(".claude/skills/stax/SKILL.md").exists(),
        "claude skill missing"
    );
    assert!(
        home.join(".cursor/skills/stax/SKILL.md").exists(),
        "cursor skill missing"
    );
    assert!(
        home.join(".pi/agent/skills/stax/SKILL.md").exists(),
        "pi skill missing"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_update_dry_run_leaves_broken_symlink_intact() {
    use std::os::unix::fs::symlink;

    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SKILLS_BODY))
        .mount(&mock_server)
        .await;

    let home_dir = tempdir().expect("home");
    let home = home_dir.path();
    let skills_dir = home.join(".codex/skills");
    std::fs::create_dir_all(&skills_dir).expect("create skills dir");
    let link = skills_dir.join("stax");
    symlink(home.join("missing-dotfiles/stax"), &link).expect("create broken symlink");

    let output = base_cmd(
        home,
        &format!("{}/skills.md", mock_server.uri()),
        &["skills", "update", "--skills", "codex", "--dry-run"],
    )
    .output()
    .expect("run skills update --dry-run");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "dry-run should exit 0; combined: {combined}"
    );

    // Symlink must still be a symlink and still broken.
    assert!(link.is_symlink(), "link should still be a symlink");
    assert!(!link.is_dir(), "link should still be broken (not a dir)");
    assert!(
        !home.join(".codex/skills/stax/SKILL.md").exists(),
        "SKILL.md must not be written in dry-run"
    );
    assert!(
        stdout.contains("would replace broken symlink"),
        "stdout: {stdout}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn skills_update_replaces_broken_skill_file_symlink() {
    use std::os::unix::fs::symlink;

    ensure_crypto_provider();
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/skills.md"))
        .respond_with(ResponseTemplate::new(200).set_body_string(SKILLS_BODY))
        .mount(&mock_server)
        .await;

    let home_dir = tempdir().expect("home");
    let home = home_dir.path();
    let skill_dir = home.join(".codex/skills/stax");
    std::fs::create_dir_all(&skill_dir).expect("create real skill dir");
    let skill_file = skill_dir.join("SKILL.md");
    // Broken file symlink: target's parent doesn't exist.
    symlink(home.join("missing-dir/SKILL.md"), &skill_file).expect("create broken file symlink");

    let output = base_cmd(
        home,
        &format!("{}/skills.md", mock_server.uri()),
        &["skills", "update", "--skills", "codex"],
    )
    .output()
    .expect("run skills update");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, String::from_utf8_lossy(&output.stderr));

    assert!(
        output.status.success(),
        "exit status: {:?}\n{combined}",
        output.status
    );

    // The broken symlink should have been replaced by a real file.
    assert!(
        !skill_file.is_symlink(),
        "SKILL.md should no longer be a symlink"
    );
    assert!(skill_file.is_file(), "SKILL.md should now be a real file");
    let content = std::fs::read_to_string(&skill_file).expect("read SKILL.md");
    assert!(content.contains("# Stax Skills"), "content: {content}");
    assert!(
        stdout.contains("replaced broken symlink"),
        "stdout: {stdout}"
    );
}
