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

#[test]
fn detects_installed_gh_stack_extension_from_gh_extension_list() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
esac
exit 1
"#,
    );

    let status =
        stax::github::gh_stack::extension_status_with_path(&path_with_fake_gh(fake.path()));

    assert_eq!(status, stax::github::gh_stack::ExtensionStatus::Installed);
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
fn view_stack_parses_branches_from_gh_stack_json() {
    let fake = fake_gh_dir(
        r#"#!/bin/sh
if [ "$1 $2" = "stack view" ]; then
  printf '%s\n' "$@" > "$GH_ARGS_FILE"
  cat <<'JSON'
{"branches":[
  {"branch":"feature-a","pr_number":10,"base":"main"},
  {"branch":"feature-b","pr_number":20,"base":"feature-a"}
]}
JSON
  exit 0
fi
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let entries = stax::github::gh_stack::view_stack_with_env(
        "10",
        &[
            ("PATH", path.as_str()),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    )
    .expect("view stack");

    assert_eq!(
        entries,
        vec![
            stax::github::gh_stack::NativeStackEntry {
                branch: "feature-a".to_string(),
                pr_number: Some(10),
                base: Some("main".to_string()),
            },
            stax::github::gh_stack::NativeStackEntry {
                branch: "feature-b".to_string(),
                pr_number: Some(20),
                base: Some("feature-a".to_string()),
            },
        ]
    );
    let args = fs::read_to_string(args_file).expect("args written");
    assert_eq!(args, "stack\nview\n--json\n10\n");
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
async fn submit_auto_registers_single_pr_native_stack_seed() {
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
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
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
    assert_eq!(args, "stack\nlink\n10\n--base\nmain\n--remote\norigin\n");
    assert_eq!(
        git_stdout(&repo, &["config", "--get", "stax.nativeStack.enabled"]),
        "true"
    );
}

#[tokio::test]
async fn submit_single_pr_native_stack_validation_rejection_is_harmless() {
    let mock_server = MockServer::start().await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_config(&home, &mock_server.uri());
    repo.configure_github_like_submit_remote();
    let branches = repo.create_stack(&["native-single-rejected"]);
    repo.git(&["push", "-u", "origin", &branches[0]])
        .assert_success();
    write_branch_pr_metadata(&repo, &branches[0], "main", 10);

    mock_existing_pr(&mock_server, 10, &branches[0], "main").await;

    let fake = fake_gh_dir(
        r#"#!/bin/sh
case "$1 $2" in
  "--version "*) echo "gh version 2.71.0"; exit 0 ;;
  "extension list") echo "gh-stack github/gh-stack v0.0.6"; exit 0 ;;
  "stack link")
    printf '%s\n' "$@" >> "$GH_ARGS_FILE"
    echo "native stacks require at least two PRs" >&2
    exit 4
    ;;
esac
exit 1
"#,
    );
    let args_file = fake.path().join("args.txt");
    let path = path_with_fake_gh(fake.path());

    let output = repo.run_stax_with_env(
        &["submit", "--no-fetch", "--yes", "--no-prompt", "--quiet"],
        &[
            ("HOME", &home),
            ("STAX_GITHUB_TOKEN", "test-token"),
            ("PATH", &path),
            ("GH_ARGS_FILE", args_file.to_str().unwrap()),
        ],
    );

    output.assert_success();
    assert!(
        TestRepo::stderr(&output).trim().is_empty(),
        "single-PR native validation rejection should be silent, stderr was:\n{}",
        TestRepo::stderr(&output)
    );
    let args = fs::read_to_string(&args_file).expect("gh stack link attempt recorded");
    assert_eq!(args, "stack\nlink\n10\n--base\nmain\n--remote\norigin\n");
    let cached = repo.git(&["config", "--get", "stax.nativeStack.enabled"]);
    assert!(
        !cached.status.success(),
        "single-PR validation rejection should not cache native stack as disabled"
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
