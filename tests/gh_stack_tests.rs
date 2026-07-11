use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fake_gh_dir(script: &str) -> TempDir {
    let dir = TempDir::new().expect("temp fake gh dir");
    let gh = dir.path().join("gh");
    fs::write(&gh, script).expect("write fake gh");
    let mut perms = fs::metadata(&gh).expect("fake gh metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&gh, perms).expect("chmod fake gh");
    dir
}

fn path_with_fake_gh(fake_dir: &Path) -> String {
    let old_path = std::env::var("PATH").unwrap_or_default();
    format!("{}:{old_path}", fake_dir.display())
}

fn write_config(home: &str, api_base_url: &str) {
    let config_dir = Path::new(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create stax config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!(
            "[remote]\napi_base_url = \"{api_base_url}\"\n\n[submit]\nstack_links = \"body\"\n"
        ),
    )
    .expect("write config");
}

fn git_stdout(repo: &TestRepo, args: &[&str]) -> String {
    let output = repo.git(args);
    output.assert_success();
    TestRepo::stdout(&output).trim().to_string()
}

fn write_branch_pr_metadata(repo: &TestRepo, branch: &str, parent: &str, pr_number: u64) {
    let parent_revision = git_stdout(repo, &["rev-parse", parent]);
    let json = serde_json::json!({
        "parentBranchName": parent,
        "parentBranchRevision": parent_revision,
        "prInfo": {
            "number": pr_number,
            "state": "OPEN",
            "isDraft": false
        }
    })
    .to_string();

    let mut hash_child = Command::new("git")
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn git hash-object");
    hash_child
        .stdin
        .as_mut()
        .expect("hash-object stdin")
        .write_all(json.as_bytes())
        .expect("write metadata json");
    let hash_output = hash_child.wait_with_output().expect("wait hash-object");
    assert!(
        hash_output.status.success(),
        "hash-object failed: {}",
        String::from_utf8_lossy(&hash_output.stderr)
    );
    let hash = String::from_utf8_lossy(&hash_output.stdout)
        .trim()
        .to_string();
    repo.git(&[
        "update-ref",
        &format!("refs/branch-metadata/{branch}"),
        &hash,
    ])
    .assert_success();
}

async fn mock_existing_pr(mock_server: &MockServer, number: u64, branch: &str, base: &str) {
    let body = serde_json::json!({
        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
        "id": number,
        "number": number,
        "state": "open",
        "title": format!("PR {number}"),
        "body": "",
        "draft": false,
        "head": { "ref": branch, "sha": "aaaa", "label": format!("test-owner:{branch}") },
        "base": { "ref": base, "sha": "bbbb" },
        "html_url": format!("https://github.com/test-owner/test-repo/pull/{number}")
    });

    Mock::given(method("GET"))
        .and(path(format!("/repos/test-owner/test-repo/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path(format!(
            "/repos/test-owner/test-repo/issues/{number}/comments"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;

    Mock::given(method("PATCH"))
        .and(path(format!("/repos/test-owner/test-repo/pulls/{number}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{number}"),
            "id": number,
            "number": number,
            "state": "open",
            "title": format!("PR {number}"),
            "body": "<!-- stax-stack-links:start -->\nupdated\n<!-- stax-stack-links:end -->",
            "draft": false,
            "head": { "ref": branch, "sha": "aaaa", "label": format!("test-owner:{branch}") },
            "base": { "ref": base, "sha": "bbbb" },
            "html_url": format!("https://github.com/test-owner/test-repo/pull/{number}")
        })))
        .mount(mock_server)
        .await;
}

async fn run_multi_pr_submit_with_fake_gh(
    fake_gh_script: &str,
    submit_args: &[&str],
) -> (std::process::Output, bool) {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["native-setup-bottom", "native-setup-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(fake_gh_script);
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        submit_args,
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    let called_stack_link = args_file.exists();
    (output, called_stack_link)
}

#[test]
fn detects_installed_gh_stack_extension_from_gh_extension_list() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
esac
exit 1
"#,
    );

    let status =
        stax::github::gh_stack::extension_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::Installed);
}

#[test]
fn extension_status_reports_no_gh_when_gh_binary_is_missing() {
    // An empty PATH (no `gh` anywhere) must be classified as `NoGh`, not
    // silently treated as `NoExtension` or crash/hang the caller.
    let empty_dir = TempDir::new().expect("empty temp dir");

    let status = stax::github::gh_stack::extension_status_with_path(
        empty_dir.path().to_str().expect("utf8 path"),
    );

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::NoGh);
}

#[test]
fn extension_status_reports_no_extension_when_gh_stack_not_in_extension_list() {
    // `gh` is present and working, but the user never installed the
    // `github/gh-stack` extension — the single most common "no gh-stack"
    // scenario for real users.
    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-eco github/gh-eco v1.0.0"; exit 0 ;;
esac
exit 1
"#,
    );

    let status =
        stax::github::gh_stack::extension_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::NoExtension);
}

#[test]
fn extension_status_reports_no_extension_when_extension_list_is_empty() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") exit 0 ;;
esac
exit 1
"#,
    );

    let status =
        stax::github::gh_stack::extension_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::NoExtension);
}

#[test]
fn detects_outdated_gh_stack_extension_without_link_command() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.1"; exit 0 ;;
  "stack --help") printf 'Available Commands:\n  add   Add a new branch\n  view  View the current stack\n'; exit 0 ;;
esac
exit 1
"#,
    );

    let status =
        stax::github::gh_stack::extension_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::Outdated);
}

#[test]
fn link_stack_classifies_private_preview_feature_disabled_failures() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "stack link" ]; then
  echo "Stacked PRs is currently in private preview and has not been enabled for this repository" >&2
  exit 4
fi
exit 1
"#,
    );

    let outcome = stax::github::gh_stack::link_stack_with_path(
        &[10, 20, 30],
        "main",
        "origin",
        &path_with_fake_gh(fake.path()),
    );

    assert!(matches!(
        outcome,
        stax::github::gh_stack::LinkOutcome::FeatureDisabled { .. }
    ));
}

#[test]
fn version_status_flags_versions_below_recommended() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "extension list" ]; then
  echo "gh-stack github/gh-stack v0.0.4"
  exit 0
fi
exit 1
"#,
    );

    let status = stax::github::gh_stack::version_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(
        status,
        stax::github::gh_stack::VersionStatus::BelowRecommended {
            installed: "0.0.4".to_string()
        }
    );
}

#[test]
fn version_status_accepts_recommended_or_newer_versions() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "extension list" ]; then
  echo "gh-stack github/gh-stack v0.0.7"
  exit 0
fi
exit 1
"#,
    );

    let status = stax::github::gh_stack::version_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(
        status,
        stax::github::gh_stack::VersionStatus::MeetsRecommended {
            installed: "0.0.7".to_string()
        }
    );
}

#[test]
fn version_status_is_unknown_when_version_cannot_be_parsed() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "extension list" ]; then
  echo "gh-stack github/gh-stack (unknown)"
  exit 0
fi
exit 1
"#,
    );

    let status = stax::github::gh_stack::version_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::VersionStatus::Unknown);
}

#[test]
fn doctor_recommends_upgrade_for_gh_stack_below_recommended_version() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.4"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
esac
exit 1
"#,
    );
    let home = repo.clean_home();
    let git_config = repo.path().join("test-global-gitconfig");
    let git_config_str = git_config.to_string_lossy().into_owned();
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["doctor"],
        &[
            ("HOME", &home),
            ("GIT_CONFIG_GLOBAL", &git_config_str),
            ("PATH", &path),
        ],
    );

    output.assert_success();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("v0.0.4") && stdout.contains("gh extension upgrade gh-stack"),
        "expected a version-upgrade recommendation, stdout was:\n{stdout}"
    );
}

#[test]
fn link_stack_classifies_pat_rejection_distinctly_from_feature_disabled() {
    // gh-stack's PAT-rejection message also contains "private preview", so
    // this must not be misclassified as the repo/org lacking the feature
    // (which would get permanently cached as `FeatureDisabled`).
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "stack link" ]; then
  echo "Personal access tokens are not supported by gh stack during private preview" >&2
  echo "  Run gh auth login to authenticate with OAuth instead." >&2
  exit 1
fi
exit 1
"#,
    );

    let outcome = stax::github::gh_stack::link_stack_with_path(
        &[10, 20],
        "main",
        "origin",
        &path_with_fake_gh(fake.path()),
    );

    assert!(
        matches!(
            outcome,
            stax::github::gh_stack::LinkOutcome::AuthTokenUnsupported { .. }
        ),
        "expected AuthTokenUnsupported, got {outcome:?}"
    );
}

#[test]
fn link_stack_strips_injected_token_env_vars_before_calling_gh() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "stack link" ]; then
  {
    echo "GH_TOKEN=${GH_TOKEN:-unset}"
    echo "GITHUB_TOKEN=${GITHUB_TOKEN:-unset}"
  } > "$ENV_DUMP_FILE"
  exit 0
fi
exit 1
"#,
    );
    let env_dump_file = fake.path().join("env.txt");
    let path = path_with_fake_gh(fake.path());

    let outcome = stax::github::gh_stack::link_stack_with_env(
        &[10, 20],
        "main",
        "origin",
        &[
            ("PATH", path.as_str()),
            ("ENV_DUMP_FILE", env_dump_file.to_str().unwrap()),
            ("GH_TOKEN", "ghp_should_be_stripped"),
            ("GITHUB_TOKEN", "ghp_should_also_be_stripped"),
        ],
    );

    assert_eq!(outcome, stax::github::gh_stack::LinkOutcome::Linked);
    let dump = fs::read_to_string(env_dump_file).expect("env dump written");
    assert_eq!(dump, "GH_TOKEN=unset\nGITHUB_TOKEN=unset\n");
}

#[test]
fn link_stack_passes_pr_numbers_bottom_to_top_with_base_and_remote() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
printf '%s\n' "$@" > "$GH_ARGS_FILE"
exit 0
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let outcome = stax::github::gh_stack::link_stack_with_env(
        &[10, 20, 30],
        "main",
        "upstream",
        &[
            ("PATH", path.as_str()),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    assert_eq!(outcome, stax::github::gh_stack::LinkOutcome::Linked);
    let args = fs::read_to_string(args_file).expect("args written");
    assert_eq!(
        args,
        "stack\nlink\n10\n20\n30\n--base\nmain\n--remote\nupstream\n"
    );
}

#[test]
fn caches_native_stack_feature_state_in_repo_git_config() {
    let repo = TestRepo::new();

    assert_eq!(
        stax::github::gh_stack::feature_enabled(repo.path()),
        stax::github::gh_stack::FeatureState::Unknown
    );

    stax::github::gh_stack::set_feature_enabled(repo.path(), false).expect("set feature disabled");
    assert_eq!(
        stax::github::gh_stack::feature_enabled(repo.path()),
        stax::github::gh_stack::FeatureState::Disabled
    );

    stax::github::gh_stack::set_feature_enabled(repo.path(), true).expect("set feature enabled");
    assert_eq!(
        stax::github::gh_stack::feature_enabled(repo.path()),
        stax::github::gh_stack::FeatureState::Enabled
    );
}

#[test]
fn doctor_fix_installs_missing_gh_stack_extension() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "gh version 2.71.0"
  exit 0
fi
if [ "$1 $2" = "extension list" ]; then
  exit 0
fi
if [ "$1 $2 $3" = "extension install github/gh-stack" ]; then
  echo "installed" >> "$GH_INSTALL_FILE"
  exit 0
fi
exit 1
"#,
    );

    let install_file = fake.path().join("installed.txt");
    let home = repo.clean_home();
    let git_config = repo.path().join("test-global-gitconfig");
    let git_config_str = git_config.to_string_lossy().into_owned();
    let path = path_with_fake_gh(fake.path());
    let output = crate::common::run_stax_in_script_with_env(
        &repo.path(),
        &["doctor", "--fix"],
        "printf 'y\n'",
        &[
            ("HOME", &home),
            ("GIT_CONFIG_GLOBAL", &git_config_str),
            ("PATH", &path),
            ("GH_INSTALL_FILE", install_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Install GitHub gh-stack extension"),
        "stdout was:\n{stdout}"
    );
    assert!(
        install_file.exists(),
        "doctor --fix should invoke gh extension install"
    );
}

#[test]
fn doctor_fix_upgrades_outdated_gh_stack_extension() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "gh version 2.71.0"
  exit 0
fi
if [ "$1 $2" = "extension list" ]; then
  echo "gh-stack github/gh-stack v0.0.1"
  exit 0
fi
if [ "$1 $2" = "stack --help" ]; then
  printf 'Available Commands:\n  view  View the current stack\n'
  exit 0
fi
if [ "$1 $2 $3" = "extension upgrade gh-stack" ]; then
  echo "upgraded" >> "$GH_UPGRADE_FILE"
  exit 0
fi
exit 1
"#,
    );

    let upgrade_file = fake.path().join("upgraded.txt");
    let home = repo.clean_home();
    let git_config = repo.path().join("test-global-gitconfig");
    let git_config_str = git_config.to_string_lossy().into_owned();
    let path = path_with_fake_gh(fake.path());
    let output = crate::common::run_stax_in_script_with_env(
        &repo.path(),
        &["doctor", "--fix"],
        "printf 'y\n'",
        &[
            ("HOME", &home),
            ("GIT_CONFIG_GLOBAL", &git_config_str),
            ("PATH", &path),
            ("GH_UPGRADE_FILE", upgrade_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("outdated") && stdout.contains("gh extension upgrade gh-stack"),
        "doctor should report the outdated extension, stdout was:\n{stdout}"
    );
    assert!(
        upgrade_file.exists(),
        "doctor --fix should invoke gh extension upgrade"
    );
}

#[test]
fn submit_help_exposes_native_stack_overrides() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["submit", "--help"]);

    output
        .assert_success()
        .assert_stdout_contains("--native-stack")
        .assert_stdout_contains("--no-native-stack");
}

#[test]
fn stack_help_exposes_native_link_commands() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["stack", "--help"]);

    output
        .assert_success()
        .assert_stdout_contains("link")
        .assert_stdout_contains("unlink");
}

#[test]
fn stack_link_rejects_single_pr_stack_with_clear_message() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&["only-branch"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link") echo "requires at least 2 arg(s), only received 1" >&2; exit 1 ;;
esac
exit 1
"#,
    );
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "link"], &[("HOME", &home), ("PATH", &path)]);

    assert!(!output.status.success(), "single-PR link should fail");
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("at least 2 PRs"),
        "expected a clear multi-PR requirement message, stderr was:\n{stderr}"
    );
    // The friendly guard must fire before shelling out to `gh stack link`.
    assert!(
        !stderr.contains("arg(s)"),
        "raw gh-stack arity error should not leak, stderr was:\n{stderr}"
    );
}

#[test]
fn stack_link_tells_user_to_submit_when_a_branch_has_no_pr() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&["linked-bottom", "unsubmitted-top"]);
    // Only the bottom branch has a PR; the current (top) branch does not.
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link") echo "should not be called" >&2; exit 1 ;;
esac
exit 1
"#,
    );
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "link"], &[("HOME", &home), ("PATH", &path)]);

    assert!(
        !output.status.success(),
        "link should fail when a branch lacks a PR"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains(&branches[1]) && stderr.contains("stax submit"),
        "expected an actionable submit-first message naming the branch, stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("should not be called"),
        "must not shell out to `gh stack link` when PRs are missing, stderr was:\n{stderr}"
    );
}

#[test]
fn stack_link_fails_with_actionable_message_when_extension_not_installed() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&["stacklink-a", "stacklink-b"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-eco github/gh-eco v1.0.0"; exit 0 ;;
  "stack link") echo "should not be called" >&2; exit 1 ;;
esac
exit 1
"#,
    );
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "link"], &[("HOME", &home), ("PATH", &path)]);

    assert!(
        !output.status.success(),
        "link should fail cleanly without the gh-stack extension"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("gh-stack") && stderr.contains("gh extension install github/gh-stack"),
        "expected an actionable install hint, stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("should not be called"),
        "must not shell out to `gh stack link` when the extension isn't installed, stderr was:\n{stderr}"
    );
}

#[test]
fn stack_unlink_fails_with_actionable_message_when_extension_not_installed() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&["stackunlink-a", "stackunlink-b"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-eco github/gh-eco v1.0.0"; exit 0 ;;
  "stack unstack") echo "should not be called" >&2; exit 1 ;;
esac
exit 1
"#,
    );
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "unlink"], &[("HOME", &home), ("PATH", &path)]);

    assert!(
        !output.status.success(),
        "unlink should fail cleanly without the gh-stack extension"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("gh-stack") && stderr.contains("gh extension install github/gh-stack"),
        "expected an actionable install hint, stderr was:\n{stderr}"
    );
    assert!(
        !stderr.contains("should not be called"),
        "must not shell out to gh-stack when the extension isn't installed, stderr was:\n{stderr}"
    );
}

#[tokio::test]
async fn submit_multi_pr_stack_succeeds_without_gh_stack_extension_installed() {
    // The most common real-world case: a user who has never installed
    // `github/gh-stack` at all. `st submit` with the default config
    // (`native_stack = "auto"`) must behave exactly as if native-stack
    // support didn't exist — no hang, no error, no `gh stack link` call,
    // and stax's own PR/body-link management proceeds normally.
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["no-extension-bottom", "no-extension-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") exit 0 ;;
  "stack link") echo "should not be called" >&2; exit 1 ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        !stdout.contains("gh-stack extension missing"),
        "default auto mode should stay quiet when gh-stack is missing, stdout was:\n{stdout}"
    );
    assert!(
        !args_file.exists(),
        "submit must never invoke `gh stack link` when the extension isn't installed"
    );
    let feature_cache = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !feature_cache.status.success(),
        "the native-stack feature cache must stay unset when gh-stack was never called, but got: {}",
        TestRepo::stdout(&feature_cache)
    );

    // stax's own PR management (body/comment stack links) must be entirely
    // unaffected by the absence of gh-stack.
    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test-owner/test-repo/pulls/10"
        }),
        "stax body stack links for #10 should still sync without gh-stack installed"
    );
    assert!(
        requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test-owner/test-repo/pulls/20"
        }),
        "stax body stack links for #20 should still sync without gh-stack installed"
    );
}

#[tokio::test]
async fn submit_native_stack_override_explains_unusable_gh_cli() {
    let (output, called_stack_link) = run_multi_pr_submit_with_fake_gh(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) exit 127 ;;
  "stack link") printf '%s\n' "$@" > "$GH_ARGS_FILE"; exit 1 ;;
esac
exit 1
"#,
        &[
            "submit",
            "--native-stack",
            "--no-fetch",
            "--yes",
            "--no-prompt",
        ],
    )
    .await;

    output.assert_success();
    assert!(
        !called_stack_link,
        "--native-stack should not invoke `gh stack link` when gh is unusable"
    );
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("GitHub CLI `gh`")
            && stdout.contains("gh auth login")
            && stdout.contains("st doctor --fix"),
        "expected an actionable gh setup note, stdout was:\n{stdout}"
    );
}

#[tokio::test]
async fn submit_native_stack_override_explains_missing_gh_stack_extension() {
    let (output, called_stack_link) = run_multi_pr_submit_with_fake_gh(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-eco github/gh-eco v1.0.0"; exit 0 ;;
  "stack link") printf '%s\n' "$@" > "$GH_ARGS_FILE"; exit 1 ;;
esac
exit 1
"#,
        &[
            "submit",
            "--native-stack",
            "--no-fetch",
            "--yes",
            "--no-prompt",
        ],
    )
    .await;

    output.assert_success();
    assert!(
        !called_stack_link,
        "--native-stack should not invoke `gh stack link` when gh-stack is missing"
    );
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("gh-stack extension missing")
            && stdout.contains("st doctor --fix")
            && stdout.contains("gh extension install github/gh-stack"),
        "expected an actionable missing-extension note, stdout was:\n{stdout}"
    );
}

#[tokio::test]
async fn submit_native_stack_override_explains_outdated_gh_stack_extension() {
    let (output, called_stack_link) = run_multi_pr_submit_with_fake_gh(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.1"; exit 0 ;;
  "stack --help") printf 'Available Commands:\n  add   Add a new branch\n  view  View the current stack\n'; exit 0 ;;
  "stack link") printf '%s\n' "$@" > "$GH_ARGS_FILE"; exit 1 ;;
esac
exit 1
"#,
        &["submit", "--native-stack", "--no-fetch", "--yes", "--no-prompt"],
    )
    .await;

    output.assert_success();
    assert!(
        !called_stack_link,
        "--native-stack should not invoke `gh stack link` when gh-stack lacks link support"
    );
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("gh-stack extension is outdated")
            && stdout.contains("st doctor --fix")
            && stdout.contains("gh extension upgrade gh-stack"),
        "expected an actionable outdated-extension note, stdout was:\n{stdout}"
    );
}

/// The fake `gh stack link` response gh-stack gives when a local stack has
/// forked: another branch sharing the same ancestor PRs already anchors a
/// native GitHub Stack, and GitHub's native Stack feature only supports one
/// linear chain at a time.
const FORK_CONFLICT_GH_STACK_SCRIPT: &str = r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    {
      echo "Looking up PRs for 2 branches..."
      echo "Checking existing stacks..."
      echo "Cannot update stack: this would remove #999 from the stack"
      echo "Current stack: #10, #20, #999"
      echo "Include all existing PRs in the command to update the stack"
    } >&2
    exit 1
    ;;
esac
exit 1
"#;

#[tokio::test]
async fn submit_explains_forked_native_stack_conflict_instead_of_raw_gh_stack_dump() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["fork-conflict-bottom", "fork-conflict-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(FORK_CONFLICT_GH_STACK_SCRIPT);
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
        ],
    );

    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("this stack has forked"),
        "expected a plain-language fork-conflict note, stdout was:\n{stdout}"
    );
    assert!(
        !stdout.contains("Include all existing PRs"),
        "the raw multi-line gh-stack dump should not leak to the user, stdout was:\n{stdout}"
    );

    // Non-fatal: the fork conflict must not have blocked native-stack
    // caching from being left alone (it's not a durable repo-level fact),
    // nor stax's own PR management.
    let feature_cache = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !feature_cache.status.success(),
        "a forked-stack conflict must not be cached as feature-disabled, but got: {}",
        TestRepo::stdout(&feature_cache)
    );
}

#[test]
fn stack_link_explains_forked_native_stack_conflict_instead_of_raw_gh_stack_dump() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&["fork-conflict-link-a", "fork-conflict-link-b"]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    let fake = fake_gh_dir(FORK_CONFLICT_GH_STACK_SCRIPT);
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "link"], &[("HOME", &home), ("PATH", &path)]);

    assert!(
        !output.status.success(),
        "linking a forked stack should fail"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("shares ancestor PRs") && stderr.contains("stax stack unlink"),
        "expected a plain-language fork-conflict explanation with unlink guidance, stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("would remove #999"),
        "the underlying gh-stack detail should still be included for debugging, stderr was:\n{stderr}"
    );
}

/// The fake `gh stack link` response gh-stack gives when it doesn't detect a
/// forked stack up front and instead attempts to reorder PRs to fit its
/// assumed linear chain, which fails once it hits a branch it can't
/// reparent — a real-world variant of the same forked-stack limitation as
/// `FORK_CONFLICT_GH_STACK_SCRIPT` above, but with different wording.
const FORK_CONFLICT_REORDER_GH_STACK_SCRIPT: &str = r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    {
      echo "Looking up PRs for 2 branches..."
      echo "Checking existing stacks..."
      echo "failed to update base branch for PR #999 to fork-conflict-reorder-top: HTTP 422: Validation Failed"
      echo "PullRequest.base is invalid"
      echo "Failed to update stack (HTTP 409): Stack contents have changed"
    } >&2
    exit 1
    ;;
esac
exit 1
"#;

#[tokio::test]
async fn submit_explains_forked_native_stack_reorder_conflict_instead_of_raw_gh_stack_dump() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches =
        repo.create_stack(&["fork-conflict-reorder-bottom", "fork-conflict-reorder-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(FORK_CONFLICT_REORDER_GH_STACK_SCRIPT);
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
        ],
    );

    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("this stack has forked"),
        "expected a plain-language fork-conflict note, stdout was:\n{stdout}"
    );
    assert!(
        !stdout.contains("PullRequest.base is invalid"),
        "the raw multi-line gh-stack dump should not leak to the user, stdout was:\n{stdout}"
    );

    let feature_cache = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !feature_cache.status.success(),
        "a forked-stack reorder conflict must not be cached as feature-disabled, but got: {}",
        TestRepo::stdout(&feature_cache)
    );
}

#[test]
fn stack_link_explains_forked_native_stack_reorder_conflict_instead_of_raw_gh_stack_dump() {
    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    repo.configure_github_like_submit_remote();
    repo.run_stax(&["init", "--trunk", "main"]).assert_success();
    let branches = repo.create_stack(&[
        "fork-conflict-reorder-link-a",
        "fork-conflict-reorder-link-b",
    ]);
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    let fake = fake_gh_dir(FORK_CONFLICT_REORDER_GH_STACK_SCRIPT);
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(&["stack", "link"], &[("HOME", &home), ("PATH", &path)]);

    assert!(
        !output.status.success(),
        "linking a forked stack should fail"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("shares ancestor PRs") && stderr.contains("stax stack unlink"),
        "expected a plain-language fork-conflict explanation with unlink guidance, stderr was:\n{stderr}"
    );
    assert!(
        stderr.contains("Stack contents have changed"),
        "the underlying gh-stack detail should still be included for debugging, stderr was:\n{stderr}"
    );
}

/// A local stack that genuinely forks (one branch has two children, both
/// included in this submit) must never even invoke `gh stack link` — the
/// real-world failure mode isn't just gh-stack rejecting the request; it can
/// also silently accept a linearized version of a forked branch set and
/// misrepresent which branch each PR actually builds on. stax must detect
/// the fork itself and skip proactively.
#[tokio::test]
async fn submit_skips_native_link_proactively_when_local_stack_has_forked() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();

    let bottom_branches = repo.create_stack(&["fork-proactive-bottom"]);
    let bottom = bottom_branches[0].clone();

    repo.run_stax(&["bc", "fork-proactive-a"]);
    repo.create_file("fork-proactive-a.txt", "a\n");
    repo.commit("Commit for fork-proactive-a");
    let fork_a = repo.current_branch();

    repo.run_stax(&["checkout", &bottom]).assert_success();
    repo.run_stax(&["bc", "fork-proactive-b"]);
    repo.create_file("fork-proactive-b.txt", "b\n");
    repo.commit("Commit for fork-proactive-b");
    let fork_b = repo.current_branch();

    repo.git(&["push", "-u", "origin", &bottom, &fork_a, &fork_b])
        .assert_success();
    write_branch_pr_metadata(&repo, &bottom, "main", 10);
    write_branch_pr_metadata(&repo, &fork_a, &bottom, 20);
    write_branch_pr_metadata(&repo, &fork_b, &bottom, 30);

    mock_existing_pr(&mock_server, 10, &bottom, "main").await;
    mock_existing_pr(&mock_server, 20, &fork_a, &bottom).await;
    mock_existing_pr(&mock_server, 30, &fork_b, &bottom).await;

    // Submitting from the shared bottom branch pulls both forks into scope.
    repo.run_stax(&["checkout", &bottom]).assert_success();

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    printf '%s\n' "$@" >> "$GH_ARGS_FILE"
    exit 0
    ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    assert!(
        !args_file.exists(),
        "a genuinely forked local stack must never invoke `gh stack link`"
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("this local stack has forked"),
        "expected a proactive fork note, stdout was:\n{stdout}"
    );
}

#[tokio::test]
async fn submit_auto_registers_native_stack_and_keeps_stax_links() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["native-bottom", "native-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    printf '%s\n' "$@" >> "$GH_ARGS_FILE"
    exit 0
    ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    let args = fs::read_to_string(&args_file).expect("gh stack link args written");
    assert_eq!(
        args,
        "stack\nlink\n10\n20\n--base\nmain\n--remote\norigin\n"
    );
    assert_eq!(
        git_stdout(&repo, &["config", "--get", "stax.nativeStack.enabled"]),
        "true"
    );

    let requests = mock_server.received_requests().await.unwrap();
    assert!(
        requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test-owner/test-repo/pulls/10"
        }),
        "stax body stack links for #10 should still be synced"
    );
    assert!(
        requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test-owner/test-repo/pulls/20"
        }),
        "stax body stack links for #20 should still be synced"
    );
}

#[tokio::test]
async fn submit_does_not_native_link_single_pr_stack() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["native-single"]);
    repo.git(&["push", "-u", "origin", &branches[0]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.7"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    printf '%s\n' "$@" >> "$GH_ARGS_FILE"
    exit 0
    ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    // `gh stack link` requires >=2 PRs, so a single-PR stack must never attempt it.
    assert!(
        !args_file.exists(),
        "single-PR submit should not invoke `gh stack link`"
    );
    let cached = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !cached.status.success(),
        "single-PR submit should not touch the native stack feature cache"
    );
}

#[tokio::test]
async fn submit_feature_disabled_caches_false_and_does_not_retry() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["disabled-bottom", "disabled-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    echo link >> "$GH_ARGS_FILE"
    echo "Stacked PRs is currently in private preview and has not been enabled for this repository" >&2
    exit 4
    ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());
    let env = [
        ("HOME", home.as_str()),
        ("STAX_GITHUB_TOKEN", "test-token"),
        ("PATH", path.as_str()),
        ("GH_ARGS_FILE", args_file.to_str().unwrap()),
    ];

    let first = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt", "--quiet"],
        &env,
    );
    first.assert_success();
    assert!(
        TestRepo::stderr(&first).trim().is_empty(),
        "feature-disabled native stack should be silent, stderr was:\n{}",
        TestRepo::stderr(&first)
    );
    assert_eq!(
        git_stdout(&repo, &["config", "--get", "stax.nativeStack.enabled"]),
        "false"
    );

    let second = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt", "--quiet"],
        &env,
    );
    second.assert_success();

    let args = fs::read_to_string(&args_file).expect("first gh stack link attempt recorded");
    assert_eq!(
        args, "link\n",
        "second submit should not retry gh stack link"
    );
}

#[tokio::test]
async fn submit_auth_token_unsupported_does_not_cache_feature_disabled() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["pat-bottom", "pat-top"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);
    write_branch_pr_metadata(&repo, &branches[1], &branches[0], 20);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;
    mock_existing_pr(&mock_server, 20, &branches[1], &branches[0]).await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
  "stack --help") printf 'Remote operations:\n  link  Link PRs into a stack on GitHub\n'; exit 0 ;;
  "stack link")
    echo "Personal access tokens are not supported by gh stack during private preview" >&2
    exit 1
    ;;
esac
exit 1
"#,
    );
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
        ],
    );

    output.assert_success();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("no OAuth-authenticated") || stdout.contains("gh auth login"),
        "expected an actionable auth note, stdout was:\n{stdout}"
    );
    // Unlike a genuine feature-disabled response, an auth-token rejection is
    // not a durable repo-level fact — it must not be cached, so a later
    // submit retries once the user's `gh` auth situation changes.
    let cached = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !cached.status.success(),
        "auth-token-unsupported outcome must not cache the native stack feature as disabled"
    );
}
