//! Tests for the `stax user` preferences command.

use crate::common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::PathBuf;

fn config_path(repo: &TestRepo) -> PathBuf {
    PathBuf::from(repo.clean_home())
        .join(".config")
        .join("stax")
        .join("config.toml")
}

fn config_contents(repo: &TestRepo) -> String {
    fs::read_to_string(config_path(repo)).unwrap_or_default()
}

#[test]
fn user_default_prints_all_settings() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["user"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);

    assert!(stdout.contains("branch-prefix"), "stdout: {stdout}");
    assert!(stdout.contains("branch-date"), "stdout: {stdout}");
    assert!(stdout.contains("branch-replacement"), "stdout: {stdout}");
    assert!(stdout.contains("editor"), "stdout: {stdout}");
    assert!(stdout.contains("tips"), "stdout: {stdout}");
    assert!(stdout.contains("submit-body"), "stdout: {stdout}");
}

#[test]
fn user_branch_prefix_set_and_unset() {
    let repo = TestRepo::new();

    repo.run_stax(&["user", "branch-prefix", "--set", "cesar/"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("prefix = \"cesar/\""),
        "config: {contents}"
    );

    repo.run_stax(&["user", "branch-prefix", "--unset"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(!contents.contains("prefix ="), "config: {contents}");
}

#[test]
fn user_branch_date_enable_and_disable() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["user", "branch-date", "--enable"]);
    output.assert_success();
    let contents = config_contents(&repo);
    assert!(contents.contains("date = true"), "config: {contents}");

    repo.run_stax(&["user", "branch-date", "--disable"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(contents.contains("date = false"), "config: {contents}");
}

#[test]
fn user_branch_date_warns_when_format_takes_precedence() {
    let repo = TestRepo::new();

    let config_dir = config_path(&repo).parent().unwrap().to_path_buf();
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_path(&repo),
        "[branch]\nformat = \"{user}/{message}\"\n",
    )
    .unwrap();

    let output = repo.run_stax(&["user", "branch-date", "--enable"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.to_lowercase().contains("format"),
        "expected a format-precedence warning, got: {stdout}"
    );
}

#[test]
fn user_branch_replacement_set_dash_and_underscore() {
    let repo = TestRepo::new();

    repo.run_stax(&["user", "branch-replacement", "--set-underscore"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("replacement = \"_\""),
        "config: {contents}"
    );

    repo.run_stax(&["user", "branch-replacement", "--set-dash"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("replacement = \"-\""),
        "config: {contents}"
    );

    repo.run_stax(&["user", "branch-replacement", "--set", "~"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("replacement = \"~\""),
        "config: {contents}"
    );
}

#[test]
fn user_editor_set_and_unset() {
    let repo = TestRepo::new();

    repo.run_stax(&["user", "editor", "--set", "vim"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(contents.contains("editor = \"vim\""), "config: {contents}");

    repo.run_stax(&["user", "editor", "--unset"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(!contents.contains("editor ="), "config: {contents}");
}

#[test]
fn user_tips_enable_and_disable() {
    let repo = TestRepo::new();

    repo.run_stax(&["user", "tips", "--disable"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(contents.contains("tips = false"), "config: {contents}");

    repo.run_stax(&["user", "tips", "--enable"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(contents.contains("tips = true"), "config: {contents}");
}

#[test]
fn user_submit_body_enable_and_disable() {
    let repo = TestRepo::new();

    repo.run_stax(&["user", "submit-body", "--disable"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("commit_messages_in_body = false"),
        "config: {contents}"
    );

    repo.run_stax(&["user", "submit-body", "--enable"])
        .assert_success();
    let contents = config_contents(&repo);
    assert!(
        contents.contains("commit_messages_in_body = true"),
        "config: {contents}"
    );
}
