//! Tests for `st create <name> -m "message"`.
//!
//! Regression: when a positional branch name was supplied, the `-m` commit
//! message was silently dropped (only staging happened), leaving an empty
//! branch. The message must be committed while the branch keeps the explicit
//! name.

use crate::common;

use common::{OutputAssertions, TestRepo};

#[test]
fn test_create_with_name_and_message_commits_with_that_message() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    let trunk_head = repo.get_commit_sha("HEAD");

    repo.create_file("feature.txt", "feature work\n");

    let output = repo.run_stax(&["create", "my-feature", "-a", "-m", "Add my feature"]);
    output.assert_success();

    // The branch keeps the explicit name, not the commit message.
    let branch = repo.current_branch();
    assert!(
        branch.contains("my-feature"),
        "Expected branch named after positional name, got: {}",
        branch
    );

    // The message must actually be committed.
    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "Add my feature");

    // HEAD advanced past trunk (i.e. the branch is not empty).
    let head = repo.get_commit_sha("HEAD");
    assert_ne!(
        head, trunk_head,
        "create <name> -m should produce a new commit, not an empty branch"
    );

    // The committed file is present in the tree.
    let show = repo.git(&["show", "HEAD:feature.txt"]);
    assert!(
        show.status.success(),
        "committed change should be in HEAD tree: {}",
        TestRepo::stderr(&show)
    );
}

#[test]
fn test_create_with_message_lowercases_branch_but_not_commit_message() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    repo.create_file("fastlane.txt", "gem 'fastlane'\n");
    repo.run_stax(&["create", "-a", "-m", "Bump Fastlane Version"])
        .assert_success();

    assert_eq!(repo.current_branch(), "bump-fastlane-version");

    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert_eq!(TestRepo::stdout(&subject).trim(), "Bump Fastlane Version");
}

#[test]
fn test_create_lowercases_explicit_branch_name() {
    let repo = TestRepo::new();
    repo.run_stax(&["status"]).assert_success();

    repo.run_stax(&["create", "My-Feature"]).assert_success();

    assert_eq!(repo.current_branch(), "my-feature");
}

#[test]
fn test_create_preserves_case_when_lowercase_disabled() {
    let repo = TestRepo::new();
    let home = std::path::PathBuf::from(repo.clean_home());
    std::fs::write(
        home.join(".config").join("stax").join("config.toml"),
        "[branch]\nlowercase = false\n",
    )
    .expect("write stax config");
    repo.run_stax(&["status"]).assert_success();

    repo.create_file("fastlane.txt", "gem 'fastlane'\n");
    repo.run_stax(&["create", "-a", "-m", "Bump Fastlane Version"])
        .assert_success();

    assert_eq!(repo.current_branch(), "Bump-Fastlane-Version");
}
