mod common;

use common::TestRepo;
use std::fs;
use std::process::{Command, Output};

const CHANGELOG_TEMPLATE: &str = r#"# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate

_placeholder_

## [0.1.0] - 2026-04-01

### Added
- Initial release.

<!-- next-url -->
[Unreleased]: https://github.com/cesarferreira/stax/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cesarferreira/stax/compare/v0.0.0...v0.1.0
"#;

fn run_release_prep(repo: &TestRepo) -> Output {
    run_release_prep_with_args(repo, &[])
}

fn run_release_prep_with_args(repo: &TestRepo, args: &[&str]) -> Output {
    let mut command = Command::new("python3");
    command
        .arg(format!(
            "{}/scripts/prepare_release.py",
            env!("CARGO_MANIFEST_DIR")
        ))
        .args(["--repo", repo.path().to_str().unwrap()])
        .args(args);
    command.output().expect("failed to run prepare_release.py")
}

fn run_release_prep_with_env(repo: &TestRepo, args: &[&str], env: &[(&str, &str)]) -> Output {
    let mut command = Command::new("python3");
    command
        .arg(format!(
            "{}/scripts/prepare_release.py",
            env!("CARGO_MANIFEST_DIR")
        ))
        .args(["--repo", repo.path().to_str().unwrap()])
        .args(args);

    for (key, value) in env {
        command.env(key, value);
    }

    command
        .output()
        .expect("failed to run prepare_release.py with env")
}

#[test]
fn test_prepare_release_generates_categorized_unreleased_notes() {
    let repo = TestRepo::new();
    repo.create_file("CHANGELOG.md", CHANGELOG_TEMPLATE);
    repo.commit("chore: add changelog");
    repo.git(&["tag", "v0.1.0"]);

    repo.create_file("feature.txt", "feature");
    repo.commit("feat(parser): add release generator (#12)");
    repo.create_file("fix.txt", "fix");
    repo.commit("fix: handle empty changelog (#13)");
    repo.create_file("docs.md", "docs");
    repo.commit("docs: document release automation");
    repo.create_file("refactor.txt", "refactor");
    repo.commit("refactor: simplify release pipeline");

    let output = run_release_prep(&repo);
    assert!(
        output.status.success(),
        "prepare_release.py failed: {}",
        TestRepo::stderr(&output)
    );

    let changelog = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");

    assert!(changelog.contains("## [Unreleased] - ReleaseDate\n\n### Added"));
    assert!(changelog.contains("- Add release generator (#12)"));
    assert!(changelog.contains("### Changed\n- Simplify release pipeline"));
    assert!(changelog.contains("### Fixed\n- Handle empty changelog (#13)"));
    assert!(changelog.contains("### Documentation\n- Document release automation"));
    assert!(!changelog.contains("_placeholder_"));
}

#[test]
fn test_prepare_release_replaces_existing_unreleased_body() {
    let repo = TestRepo::new();
    repo.create_file("CHANGELOG.md", CHANGELOG_TEMPLATE);
    repo.commit("chore: add changelog");
    repo.git(&["tag", "v0.1.0"]);

    repo.create_file("bugfix.txt", "fix");
    repo.commit("fix(ui): keep release notes current");

    let output = run_release_prep(&repo);
    assert!(
        output.status.success(),
        "prepare_release.py failed: {}",
        TestRepo::stderr(&output)
    );

    let changelog = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");
    assert!(!changelog.contains("_placeholder_"));
    assert!(changelog.contains("### Fixed\n- Keep release notes current"));
}

#[test]
fn test_prepare_release_fails_when_no_commits_since_last_tag() {
    let repo = TestRepo::new();
    repo.create_file("CHANGELOG.md", CHANGELOG_TEMPLATE);
    repo.commit("chore: add changelog");
    repo.git(&["tag", "v0.1.0"]);

    let before = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");

    let output = run_release_prep(&repo);
    assert!(
        !output.status.success(),
        "prepare_release.py unexpectedly succeeded"
    );

    let stderr = TestRepo::stderr(&output);
    assert!(stderr.contains("No commits found since last tag"));

    let after = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");
    assert_eq!(before, after);
}

#[test]
fn test_prepare_release_hook_mode_writes_released_entry_and_refreshes_links() {
    let repo = TestRepo::new();
    repo.create_file("CHANGELOG.md", CHANGELOG_TEMPLATE);
    repo.commit("chore: add changelog");
    repo.git(&["tag", "v0.1.0"]);

    repo.create_file("feature.txt", "feature");
    repo.commit("feat(parser): add release generator (#12)");
    repo.create_file("fix.txt", "fix");
    repo.commit("fix: handle empty changelog (#13)");

    let output = run_release_prep_with_env(
        &repo,
        &[
            "--release-version",
            "0.2.0",
            "--previous-version",
            "0.1.0",
            "--release-date",
            "2026-04-21",
        ],
        &[("DRY_RUN", "false")],
    );
    assert!(
        output.status.success(),
        "prepare_release.py hook mode failed: {}",
        TestRepo::stderr(&output)
    );

    let changelog = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");

    assert!(changelog.contains("## [Unreleased] - ReleaseDate\n\n## [0.2.0] - 2026-04-21"));
    assert!(changelog.contains("### Added\n- Add release generator (#12)"));
    assert!(changelog.contains("### Fixed\n- Handle empty changelog (#13)"));
    assert!(changelog
        .contains("[Unreleased]: https://github.com/cesarferreira/stax/compare/v0.2.0...HEAD"));
    assert!(changelog
        .contains("[0.2.0]: https://github.com/cesarferreira/stax/compare/v0.1.0...v0.2.0"));
    assert!(!changelog.contains("_placeholder_"));
}

#[test]
fn test_prepare_release_hook_mode_skips_changes_during_dry_run() {
    let repo = TestRepo::new();
    repo.create_file("CHANGELOG.md", CHANGELOG_TEMPLATE);
    repo.commit("chore: add changelog");
    repo.git(&["tag", "v0.1.0"]);

    repo.create_file("feature.txt", "feature");
    repo.commit("feat(parser): add release generator (#12)");

    let before = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");

    let output = run_release_prep_with_env(
        &repo,
        &[
            "--release-version",
            "0.2.0",
            "--previous-version",
            "0.1.0",
            "--release-date",
            "2026-04-21",
        ],
        &[("DRY_RUN", "true")],
    );
    assert!(
        output.status.success(),
        "prepare_release.py dry-run hook mode failed: {}",
        TestRepo::stderr(&output)
    );

    let after = fs::read_to_string(repo.path().join("CHANGELOG.md")).expect("read changelog");
    assert_eq!(before, after);
}
