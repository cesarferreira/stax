use crate::common;

use common::{TestRepo, run_stax_in_script_with_env};
use std::fs;
use std::path::Path;

const CURRENT: &str = env!("CARGO_PKG_VERSION");

fn config_dir_with(home: &str, last_seen: Option<&str>, config_toml: &str) -> String {
    let dir = Path::new(home).join(".config").join("stax");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("config.toml"), config_toml).unwrap();
    if let Some(version) = last_seen {
        fs::write(dir.join("last_seen_version"), format!("{version}\n")).unwrap();
    }
    dir.to_string_lossy().into_owned()
}

fn last_seen(config_dir: &str) -> String {
    fs::read_to_string(Path::new(config_dir).join("last_seen_version"))
        .expect("last_seen_version written")
        .trim()
        .to_string()
}

fn run_in_terminal(repo: &TestRepo, home: &str, config_dir: &str) -> String {
    let output = run_stax_in_script_with_env(
        &repo.path(),
        &["config"],
        "true",
        &[("HOME", home), ("STAX_CONFIG_DIR", config_dir)],
    );
    assert!(
        output.status.success(),
        "stax config failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let ansi = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    ansi.replace_all(&String::from_utf8_lossy(&output.stdout), "")
        .into_owned()
}

#[test]
fn first_run_after_upgrade_shows_release_notes_once() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    let config_dir = config_dir_with(&home, Some("0.0.1"), "");

    let first = run_in_terminal(&repo, &home, &config_dir);
    assert!(
        first.contains(&format!("What's new in stax {CURRENT}")),
        "expected release notes:\n{first}"
    );
    assert!(first.contains("Full changelog:"), "{first}");
    assert_eq!(last_seen(&config_dir), CURRENT);

    let second = run_in_terminal(&repo, &home, &config_dir);
    assert!(
        !second.contains("What's new in stax"),
        "notes must show only once:\n{second}"
    );
}

#[test]
fn piped_run_keeps_release_notes_for_a_later_terminal_run() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    let config_dir = config_dir_with(&home, Some("0.0.1"), "");

    let output = repo.run_stax_with_env(&["config"], &[("STAX_CONFIG_DIR", &config_dir)]);
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));
    assert!(!TestRepo::stderr(&output).contains("What's new"));
    assert!(!TestRepo::stdout(&output).contains("What's new"));
    assert_eq!(last_seen(&config_dir), "0.0.1");
}

#[test]
fn fresh_install_records_version_without_release_notes() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    let config_dir = config_dir_with(&home, None, "");

    let out = run_in_terminal(&repo, &home, &config_dir);
    assert!(!out.contains("What's new in stax"), "{out}");
    assert_eq!(last_seen(&config_dir), CURRENT);
}

#[test]
fn disabled_release_notes_record_version_silently() {
    let repo = TestRepo::new();
    let home = repo.clean_home();
    let config_dir = config_dir_with(&home, Some("0.0.1"), "[ui]\nwhats_new = false\n");

    let out = run_in_terminal(&repo, &home, &config_dir);
    assert!(!out.contains("What's new in stax"), "{out}");
    assert_eq!(last_seen(&config_dir), CURRENT);
}
