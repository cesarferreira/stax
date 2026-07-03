use crate::common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct BranchSnapshot {
    name: String,
    before: String,
}

struct RemoteSyncedParentStack {
    submitted: BranchSnapshot,
    leaf: Option<BranchSnapshot>,
    parent_after: String,
}

fn remote_synced_parent_stack(repo: &TestRepo, names: &[&str]) -> RemoteSyncedParentStack {
    assert!(
        names.len() >= 2,
        "fixture requires at least a parent and submitted branch"
    );

    let branches = repo.create_stack(names);
    let parent = branches[0].clone();
    let submitted = BranchSnapshot {
        name: branches[1].clone(),
        before: repo.get_commit_sha(&branches[1]),
    };
    let leaf = branches.get(2).map(|branch| BranchSnapshot {
        name: branch.clone(),
        before: repo.get_commit_sha(branch),
    });

    repo.run_stax(&["checkout", &parent]).assert_success();
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.create_file(&format!("{}-remote-update.txt", names[0]), "parent v2\n");
    repo.commit("Parent update");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();
    let parent_after = repo.get_commit_sha(&parent);

    repo.run_stax(&["checkout", &submitted.name])
        .assert_success();

    RemoteSyncedParentStack {
        submitted,
        leaf,
        parent_after,
    }
}

fn fetch_origin(repo: &TestRepo) {
    repo.git(&["fetch", "origin"]).assert_success();
}

fn run_git(cwd: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    output.assert_success();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn push_remote_only_branch(repo: &TestRepo, branch: &str, file: &str, content: &str) {
    repo.git(&["checkout", "-B", branch, "main"])
        .assert_success();
    repo.create_file(file, content);
    repo.commit(&format!("Commit {branch}"));
    repo.git(&["push", "-u", "origin", branch]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["branch", "-D", branch]).assert_success();
}

fn remote_branch_sha(repo: &TestRepo, branch: &str) -> String {
    let output = repo.git(&["ls-remote", "origin", &format!("refs/heads/{branch}")]);
    output.assert_success();
    TestRepo::stdout(&output)
        .split_whitespace()
        .next()
        .expect("remote branch sha")
        .to_string()
}

fn simulate_remote_owner_update(
    repo: &TestRepo,
    branch: &str,
    file: &str,
    content: &str,
) -> String {
    let remote_path = repo.remote_path().expect("remote path");
    let clone = TempDir::new().expect("temp clone");

    run_git(clone.path(), &["clone", remote_path.to_str().unwrap(), "."]);
    run_git(
        clone.path(),
        &["checkout", "-B", branch, &format!("origin/{branch}")],
    );
    run_git(clone.path(), &["config", "user.email", "other@test.com"]);
    run_git(clone.path(), &["config", "user.name", "Other User"]);
    fs::write(clone.path().join(file), content).expect("write remote update");
    run_git(clone.path(), &["add", "-A"]);
    run_git(clone.path(), &["commit", "-m", "Update imported branch"]);
    run_git(clone.path(), &["push", "origin", branch]);
    run_git(clone.path(), &["rev-parse", branch])
}

fn assert_contains_commit(repo: &TestRepo, ancestor: &str, descendant: &str, message: &str) {
    let output = repo.git(&["merge-base", "--is-ancestor", ancestor, descendant]);
    assert!(
        output.status.success(),
        "{message}\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
}

fn assert_does_not_contain_commit(
    repo: &TestRepo,
    ancestor: &str,
    descendant: &str,
    message: &str,
) {
    let output = repo.git(&["merge-base", "--is-ancestor", ancestor, descendant]);
    assert!(
        !output.status.success(),
        "{message}\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
}

fn assert_no_temporary_submit_refs(repo: &TestRepo) {
    let temp_refs = repo.git(&["for-each-ref", "--format=%(refname)", "refs/stax/submit"]);
    assert!(
        TestRepo::stdout(&temp_refs).trim().is_empty(),
        "temporary submit refs should be cleaned up"
    );
}

fn assert_no_temporary_submit_worktrees(repo: &TestRepo) {
    let worktrees = repo.git(&["worktree", "list", "--porcelain"]);
    worktrees.assert_success();
    assert!(
        !TestRepo::stdout(&worktrees).contains("stax-submit-"),
        "temporary submit worktrees should be cleaned up"
    );
}

fn write_test_config(home: &Path, api_base_url: &str) {
    let config_dir = home.join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("failed to create test config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{api_base_url}\"\n\n[submit]\nstack_links = \"off\"\n"),
    )
    .expect("failed to write test config");
}

async fn mock_github_pr_create(mock_server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/repos/test-owner/test-repo/pulls"))
        .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
            "url": "https://api.github.com/repos/test-owner/test-repo/pulls/42",
            "id": 42,
            "number": 42,
            "state": "open",
            "title": "created",
            "body": "",
            "draft": false,
            "head": { "ref": "created", "sha": "aaaa", "label": "test-owner:created" },
            "base": { "ref": "main", "sha": "bbbb" },
            "html_url": "https://github.com/test-owner/test-repo/pull/42"
        })))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/issues/42/comments"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/repos/test-owner/test-repo/pulls/42"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "url": "https://api.github.com/repos/test-owner/test-repo/pulls/42",
            "id": 42,
            "number": 42,
            "state": "open",
            "title": "created",
            "body": "",
            "draft": false,
            "head": { "ref": "created", "sha": "aaaa", "label": "test-owner:created" },
            "base": { "ref": "main", "sha": "bbbb" },
            "html_url": "https://github.com/test-owner/test-repo/pull/42"
        })))
        .mount(mock_server)
        .await;
}

#[test]
fn branch_submit_temporarily_restacks_when_parent_is_remote_synced() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let stack = remote_synced_parent_stack(&repo, &["temp-parent", "temp-child"]);
    repo.create_file("child-extra.txt", "manual extra child commit\n");
    repo.commit("Manual extra child commit");
    let local_child_before = repo.get_commit_sha(&stack.submitted.name);

    repo.run_stax(&["branch", "submit", "--no-pr", "--yes"])
        .assert_success();

    assert_eq!(
        repo.get_commit_sha(&stack.submitted.name),
        local_child_before,
        "scoped submit should not move the local stale child"
    );

    fetch_origin(&repo);
    let remote_child = format!("origin/{}", stack.submitted.name);
    assert_ne!(
        repo.get_commit_sha(&remote_child),
        local_child_before,
        "remote child should be the temporary rebased head"
    );
    repo.git(&["show", &format!("{}:child-extra.txt", remote_child)])
        .assert_success();

    assert_contains_commit(
        &repo,
        &stack.parent_after,
        &remote_child,
        "remote child should include the synced parent update",
    );
    assert_does_not_contain_commit(
        &repo,
        &stack.parent_after,
        &stack.submitted.name,
        "local child should still need restack",
    );
    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}

#[tokio::test]
async fn upstack_submit_pr_defaults_exclude_parent_commits_from_temporary_parent() {
    let mock_server = MockServer::start().await;
    mock_github_pr_create(&mock_server).await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());
    repo.configure_github_like_submit_remote();

    let stack =
        remote_synced_parent_stack(&repo, &["temp-pr-parent", "temp-pr-middle", "temp-pr-leaf"]);
    let leaf = stack.leaf.as_ref().expect("fixture should include a leaf");

    let output = repo.run_stax_with_env(
        &[
            "upstack",
            "submit",
            "--yes",
            "--no-prompt",
            "--publish",
            "--no-template",
        ],
        &[("STAX_GITHUB_TOKEN", "test-token")],
    );
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));

    let requests = mock_server.received_requests().await.unwrap();
    let payloads = requests
        .iter()
        .filter(|request| {
            request.method.as_str() == "POST"
                && request.url.path() == "/repos/test-owner/test-repo/pulls"
        })
        .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap())
        .collect::<Vec<_>>();

    let middle = payloads
        .iter()
        .find(|payload| payload["head"].as_str() == Some(stack.submitted.name.as_str()))
        .expect("missing middle PR create payload");
    assert_eq!(middle["title"], "Commit for temp-pr-middle");
    assert!(
        !middle["body"]
            .as_str()
            .unwrap_or_default()
            .contains("Parent update"),
        "middle PR body should not include excluded parent commits: {middle}"
    );

    let leaf_payload = payloads
        .iter()
        .find(|payload| payload["head"].as_str() == Some(leaf.name.as_str()))
        .expect("missing leaf PR create payload");
    assert_eq!(leaf_payload["title"], "Commit for temp-pr-leaf");
    let leaf_body = leaf_payload["body"].as_str().unwrap_or_default();
    assert!(
        !leaf_body.contains("Parent update"),
        "leaf PR body should not include excluded grandparent commits: {leaf_payload}"
    );
    assert!(
        !leaf_body.contains("Commit for temp-pr-middle"),
        "leaf PR body should not include its temporary parent commit: {leaf_payload}"
    );
}

#[tokio::test]
async fn stack_submit_pr_defaults_exclude_rewritten_parent_commits() {
    let mock_server = MockServer::start().await;
    mock_github_pr_create(&mock_server).await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());
    repo.configure_github_like_submit_remote();

    let branches = repo.create_stack(&["rewrite-parent", "rewrite-child"]);
    let parent = &branches[0];
    let child = &branches[1];

    repo.run_stax(&["checkout", parent]).assert_success();
    repo.create_file("rewrite-parent.txt", "rewritten parent\n");
    repo.git(&["add", "-A"]).assert_success();
    repo.git(&["commit", "--amend", "-m", "Rewritten parent"])
        .assert_success();

    repo.run_stax(&["checkout", child]).assert_success();
    let output = repo.run_stax_with_env(
        &[
            "submit",
            "--yes",
            "--no-prompt",
            "--publish",
            "--no-template",
        ],
        &[("STAX_GITHUB_TOKEN", "test-token")],
    );
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));

    let requests = mock_server.received_requests().await.unwrap();
    let child_payload = requests
        .iter()
        .filter(|request| {
            request.method.as_str() == "POST"
                && request.url.path() == "/repos/test-owner/test-repo/pulls"
        })
        .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap())
        .find(|payload| payload["head"].as_str() == Some(child.as_str()))
        .expect("missing child PR create payload");

    assert_eq!(child_payload["title"], "Commit for rewrite-child");
    assert!(
        !child_payload["body"]
            .as_str()
            .unwrap_or_default()
            .contains("Commit for rewrite-parent"),
        "child PR body should not include stale parent commits: {child_payload}"
    );
}

#[tokio::test]
async fn branch_submit_pr_defaults_exclude_imported_parent_commits_after_temporary_restack() {
    let mock_server = MockServer::start().await;
    mock_github_pr_create(&mock_server).await;

    let repo = TestRepo::new_with_remote();
    let home = repo.clean_home();
    write_test_config(Path::new(&home), &mock_server.uri());
    repo.configure_github_like_submit_remote();

    push_remote_only_branch(
        &repo,
        "imported-pr-parent",
        "imported-parent.txt",
        "remote parent\n",
    );
    repo.run_stax(&["get", "imported-pr-parent"])
        .assert_success();
    repo.run_stax(&["create", "imported-pr-child"])
        .assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child change");
    let child = repo.current_branch();

    let imported_parent_remote_after = simulate_remote_owner_update(
        &repo,
        "imported-pr-parent",
        "imported-parent-update.txt",
        "remote parent update\n",
    );
    fetch_origin(&repo);
    repo.git(&[
        "branch",
        "-f",
        "imported-pr-parent",
        "origin/imported-pr-parent",
    ])
    .assert_success();

    let output = repo.run_stax_with_env(
        &[
            "branch",
            "submit",
            "--yes",
            "--no-prompt",
            "--publish",
            "--no-template",
        ],
        &[("STAX_GITHUB_TOKEN", "test-token")],
    );
    assert!(output.status.success(), "{}", TestRepo::stderr(&output));
    assert_eq!(
        remote_branch_sha(&repo, "imported-pr-parent"),
        imported_parent_remote_after,
        "branch submit should not push or rewrite the imported parent remote"
    );

    let requests = mock_server.received_requests().await.unwrap();
    let payload = requests
        .iter()
        .find(|request| {
            request.method.as_str() == "POST"
                && request.url.path() == "/repos/test-owner/test-repo/pulls"
        })
        .map(|request| serde_json::from_slice::<serde_json::Value>(&request.body).unwrap())
        .expect("missing child PR create payload");

    assert_eq!(payload["head"], child);
    assert_eq!(payload["title"], "Child change");
    let body = payload["body"].as_str().unwrap_or_default();
    assert!(
        !body.contains("Commit imported-pr-parent"),
        "child PR body should not include imported parent commit: {payload}"
    );
    assert!(
        !body.contains("Update imported branch"),
        "child PR body should not include imported parent update: {payload}"
    );
}

#[test]
fn upstack_submit_temporarily_restacks_descendants() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    let stack =
        remote_synced_parent_stack(&repo, &["temp-us-parent", "temp-us-middle", "temp-us-leaf"]);
    let leaf = stack.leaf.as_ref().expect("fixture should include a leaf");

    repo.run_stax(&["upstack", "submit", "--no-pr", "--yes"])
        .assert_success();

    assert_eq!(
        repo.get_commit_sha(&stack.submitted.name),
        stack.submitted.before
    );
    assert_eq!(repo.get_commit_sha(&leaf.name), leaf.before);

    fetch_origin(&repo);
    let remote_middle = format!("origin/{}", stack.submitted.name);
    let remote_leaf = format!("origin/{}", leaf.name);
    assert_ne!(repo.get_commit_sha(&remote_middle), stack.submitted.before);
    assert_ne!(repo.get_commit_sha(&remote_leaf), leaf.before);

    assert_contains_commit(
        &repo,
        &stack.parent_after,
        &remote_middle,
        "remote middle should include the synced parent update",
    );
    assert_contains_commit(
        &repo,
        &remote_middle,
        &remote_leaf,
        "remote leaf should be pushed on top of the temporary middle head",
    );
    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}

#[test]
fn branch_submit_cleans_up_temporary_state_when_temporary_restack_conflicts() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();

    repo.run_stax(&["bc", "temp-conflict-parent"])
        .assert_success();
    let parent = repo.current_branch();
    repo.create_file("conflict.txt", "parent v1\n");
    repo.commit("Parent commit");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.run_stax(&["bc", "temp-conflict-child"])
        .assert_success();
    repo.create_file("conflict.txt", "child edit\n");
    repo.commit("Child conflicting commit");
    let child = repo.current_branch();

    repo.run_stax(&["checkout", &parent]).assert_success();
    repo.create_file("conflict.txt", "parent v2\n");
    repo.commit("Parent update");
    repo.git(&["push", "-u", "origin", &parent])
        .assert_success();

    repo.run_stax(&["checkout", &child]).assert_success();
    repo.run_stax(&["branch", "submit", "--no-pr", "--yes"])
        .assert_failure();

    assert_no_temporary_submit_refs(&repo);
    assert_no_temporary_submit_worktrees(&repo);
}
