//! Integration tests for `stax get`.

use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn push_remote_only_branch_from(
    repo: &TestRepo,
    branch: &str,
    base: &str,
    file: &str,
    content: &str,
) -> String {
    repo.git(&["checkout", "-B", branch, base]).assert_success();
    repo.create_file(file, content);
    repo.commit(&format!("Commit {}", branch));
    let sha = repo.head_sha();
    repo.git(&["push", "-u", "origin", branch]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["branch", "-D", branch]).assert_success();
    sha
}

fn push_remote_only_branch(repo: &TestRepo, branch: &str, file: &str, content: &str) -> String {
    push_remote_only_branch_from(repo, branch, "main", file, content)
}

fn parent_for(repo: &TestRepo, branch: &str) -> Option<String> {
    let json = repo.get_status_json();
    json["branches"]
        .as_array()
        .and_then(|branches| {
            branches
                .iter()
                .find(|entry| entry["name"].as_str() == Some(branch))
        })
        .and_then(|entry| entry["parent"].as_str())
        .map(ToString::to_string)
}

fn metadata_for(repo: &TestRepo, branch: &str) -> Value {
    let output = repo.git(&["show", &format!("refs/branch-metadata/{}", branch)]);
    output.assert_success();
    serde_json::from_str(&TestRepo::stdout(&output)).expect("valid metadata JSON")
}

fn run_git_in(cwd: &Path, args: &[&str]) -> std::process::Output {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("run git");
    output.assert_success();
    output
}

fn update_remote_branch_in_clone(
    repo: &TestRepo,
    branch: &str,
    file: &str,
    content: &str,
) -> String {
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = TempDir::new().expect("temp clone");

    run_git_in(
        clone_dir.path(),
        &["clone", remote_path.to_str().unwrap(), "."],
    );
    run_git_in(
        clone_dir.path(),
        &["checkout", "-B", branch, &format!("origin/{}", branch)],
    );
    run_git_in(
        clone_dir.path(),
        &["config", "user.email", "other@test.com"],
    );
    run_git_in(clone_dir.path(), &["config", "user.name", "Other User"]);
    std::fs::write(clone_dir.path().join(file), content).expect("write remote update");
    run_git_in(clone_dir.path(), &["add", "-A"]);
    run_git_in(
        clone_dir.path(),
        &["commit", "-m", "Update imported branch"],
    );
    let sha = run_git_in(clone_dir.path(), &["rev-parse", "HEAD"]);
    run_git_in(clone_dir.path(), &["push", "origin", branch]);

    String::from_utf8_lossy(&sha.stdout).trim().to_string()
}

fn write_test_config(home: &str, api_base_url: &str) {
    let config_dir = Path::new(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create stax config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("[remote]\napi_base_url = \"{}\"\n", api_base_url),
    )
    .expect("write stax config");
}

fn mock_pr_json(number: u64, head: &str, base: &str) -> serde_json::Value {
    serde_json::json!({
        "url": format!("https://api.github.com/repos/test-owner/test-repo/pulls/{}", number),
        "id": number,
        "number": number,
        "title": format!("PR {}", number),
        "head": { "ref": head, "sha": "aaaa", "label": format!("test-owner:{}", head) },
        "base": { "ref": base, "sha": "bbbb" },
        "draft": false
    })
}

#[test]
fn get_fetches_remote_only_branch_and_tracks_it() {
    let repo = TestRepo::new_with_remote();
    let remote_sha = push_remote_only_branch(
        &repo,
        "remote-feature",
        "remote.txt",
        "remote branch content\n",
    );

    let out = repo.run_stax(&["get", "remote-feature"]);
    out.assert_success()
        .assert_stdout_contains("Created")
        .assert_stdout_contains("remote-feature");

    assert_eq!(repo.current_branch(), "remote-feature");
    assert_eq!(repo.get_commit_sha("remote-feature"), remote_sha);
    assert_eq!(repo.get_commit_sha("origin/remote-feature"), remote_sha);
    assert_eq!(parent_for(&repo, "remote-feature").as_deref(), Some("main"));
    assert_eq!(
        metadata_for(&repo, "remote-feature")["sourceRemote"],
        "origin"
    );

    repo.git(&["config", "branch.remote-feature.remote"])
        .assert_success()
        .assert_stdout_contains("origin");
    repo.git(&["config", "branch.remote-feature.merge"])
        .assert_success()
        .assert_stdout_contains("refs/heads/remote-feature");
}

#[test]
fn get_accepts_pr_number() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    let remote_sha = push_remote_only_branch(&repo, "pr-feature", "pr.txt", "remote PR branch\n");
    let home = repo.clean_home();

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mock_server = runtime.block_on(async {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls/123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_pr_json(
                123,
                "pr-feature",
                "main",
            )))
            .mount(&mock_server)
            .await;
        mock_server
    });
    write_test_config(&home, &mock_server.uri());

    let out = repo.run_stax_with_env(
        &["get", "123", "--no-checkout", "--no-restack"],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    );
    out.assert_success()
        .assert_stdout_contains("Created")
        .assert_stdout_contains("pr-feature");

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("pr-feature"), remote_sha);
    let metadata = metadata_for(&repo, "pr-feature");
    assert_eq!(metadata["prInfo"]["number"], 123);
    assert_eq!(parent_for(&repo, "pr-feature").as_deref(), Some("main"));
}

#[test]
fn get_remote_upstack_includes_remote_only_child_prs() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    push_remote_only_branch(&repo, "remote-base", "base.txt", "remote base\n");

    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = TempDir::new().expect("temp clone");
    run_git_in(
        clone_dir.path(),
        &["clone", remote_path.to_str().unwrap(), "."],
    );
    run_git_in(
        clone_dir.path(),
        &["checkout", "-B", "remote-child", "origin/remote-base"],
    );
    run_git_in(
        clone_dir.path(),
        &["config", "user.email", "other@test.com"],
    );
    run_git_in(clone_dir.path(), &["config", "user.name", "Other User"]);
    fs::write(clone_dir.path().join("child.txt"), "remote child\n").expect("write child");
    run_git_in(clone_dir.path(), &["add", "-A"]);
    run_git_in(clone_dir.path(), &["commit", "-m", "Commit remote child"]);
    let child_sha =
        String::from_utf8_lossy(&run_git_in(clone_dir.path(), &["rev-parse", "HEAD"]).stdout)
            .trim()
            .to_string();
    run_git_in(clone_dir.path(), &["push", "origin", "remote-child"]);

    let home = repo.clean_home();
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let mock_server = runtime.block_on(async {
        let mock_server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/test-owner/test-repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                mock_pr_json(124, "remote-child", "remote-base")
            ])))
            .mount(&mock_server)
            .await;
        mock_server
    });
    write_test_config(&home, &mock_server.uri());

    let out = repo.run_stax_with_env(
        &[
            "get",
            "remote-base",
            "--remote-upstack",
            "--no-checkout",
            "--no-restack",
        ],
        &[("STAX_GITHUB_TOKEN", "mock-token")],
    );
    out.assert_success()
        .assert_stdout_contains("remote-base")
        .assert_stdout_contains("remote-child");

    assert_eq!(repo.get_commit_sha("remote-child"), child_sha);
    assert_eq!(
        parent_for(&repo, "remote-child").as_deref(),
        Some("remote-base")
    );
    assert_eq!(metadata_for(&repo, "remote-child")["prInfo"]["number"], 124);
}

#[test]
fn get_can_track_without_checkout_under_explicit_parent() {
    let repo = TestRepo::new_with_remote();
    let parent = repo.create_stack(&["review-base"]).remove(0);
    repo.git(&["checkout", "main"]).assert_success();
    let remote_sha = push_remote_only_branch_from(
        &repo,
        "remote-child",
        &parent,
        "child.txt",
        "child branch content\n",
    );

    let out = repo.run_stax(&["get", "remote-child", "--parent", &parent, "--no-checkout"]);
    out.assert_success()
        .assert_stdout_contains("Tracked")
        .assert_stdout_contains("remote-child");

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("remote-child"), remote_sha);
    assert_eq!(
        parent_for(&repo, "remote-child").as_deref(),
        Some(parent.as_str())
    );
}

#[test]
fn get_accepts_remote_qualified_branch_name() {
    let repo = TestRepo::new_with_remote();
    let remote_sha = push_remote_only_branch(
        &repo,
        "qualified-feature",
        "qualified.txt",
        "qualified branch content\n",
    );

    let out = repo.run_stax(&["get", "origin/qualified-feature", "--no-checkout"]);
    out.assert_success();

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("qualified-feature"), remote_sha);
    assert_eq!(
        parent_for(&repo, "qualified-feature").as_deref(),
        Some("main")
    );
}

#[test]
fn get_missing_remote_branch_fails_clearly() {
    let repo = TestRepo::new_with_remote();

    let out = repo.run_stax(&["get", "does-not-exist"]);
    out.assert_failure();

    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("Remote branch 'does-not-exist' was not found"),
        "expected missing-branch error, got:\n{}",
        stderr
    );
}

#[test]
fn get_fast_forwards_existing_local_branch() {
    let repo = TestRepo::new_with_remote();
    push_remote_only_branch(&repo, "existing-feature", "remote.txt", "remote content\n");

    repo.run_stax(&["get", "existing-feature", "--no-checkout"])
        .assert_success();
    let remote_sha = update_remote_branch_in_clone(
        &repo,
        "existing-feature",
        "remote-2.txt",
        "remote content 2\n",
    );

    let out = repo.run_stax(&["get", "existing-feature", "--no-checkout"]);
    out.assert_success()
        .assert_stdout_contains("Fast-forwarded");

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("existing-feature"), remote_sha);
    repo.git(&["show", "existing-feature:remote-2.txt"])
        .assert_success();
}

#[test]
fn get_rebases_divergent_local_branch_without_overwriting_local_commits() {
    let repo = TestRepo::new_with_remote();
    push_remote_only_branch(&repo, "divergent-feature", "remote.txt", "remote content\n");

    repo.run_stax(&["get", "divergent-feature", "--no-checkout"])
        .assert_success();
    repo.git(&["checkout", "divergent-feature"])
        .assert_success();
    repo.create_file("local.txt", "local content\n");
    repo.commit("Local divergent commit");
    let local_sha = repo.head_sha();
    repo.git(&["checkout", "main"]).assert_success();
    let remote_sha = update_remote_branch_in_clone(
        &repo,
        "divergent-feature",
        "remote-2.txt",
        "remote content 2\n",
    );

    let out = repo.run_stax(&["get", "divergent-feature", "--no-checkout"]);
    out.assert_success().assert_stdout_contains("Rebased");

    assert_eq!(repo.current_branch(), "main");
    assert_ne!(repo.get_commit_sha("divergent-feature"), local_sha);
    repo.git(&[
        "merge-base",
        "--is-ancestor",
        &remote_sha,
        "divergent-feature",
    ])
    .assert_success();
    repo.git(&["show", "divergent-feature:local.txt"])
        .assert_success();
    repo.git(&["show", "divergent-feature:remote-2.txt"])
        .assert_success();
}

#[test]
fn get_force_resets_divergent_local_branch() {
    let repo = TestRepo::new_with_remote();
    let remote_sha =
        push_remote_only_branch(&repo, "force-feature", "remote.txt", "remote content\n");

    repo.git(&["checkout", "-b", "force-feature", "main"])
        .assert_success();
    repo.create_file("local.txt", "local content\n");
    repo.commit("Local divergent commit");
    repo.git(&["checkout", "main"]).assert_success();

    let out = repo.run_stax(&["get", "force-feature", "--force", "--no-checkout"]);
    out.assert_success()
        .assert_stdout_contains("Reset")
        .assert_stdout_contains("Tracked");

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha("force-feature"), remote_sha);
    assert_eq!(parent_for(&repo, "force-feature").as_deref(), Some("main"));
}

#[test]
fn get_existing_branch_syncs_local_upstack_by_default() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["review-base", "review-child"]);
    let base = &branches[0];
    let child = &branches[1];
    repo.git(&["push", "-u", "origin", base, child])
        .assert_success();
    repo.git(&["checkout", "main"]).assert_success();

    let updated_base_sha =
        update_remote_branch_in_clone(&repo, base, "base-2.txt", "review base 2\n");
    let updated_child_sha =
        update_remote_branch_in_clone(&repo, child, "child-2.txt", "review child 2\n");

    let out = repo.run_stax(&["get", base, "--no-checkout", "--no-restack"]);
    out.assert_success()
        .assert_stdout_contains("Fast-forwarded")
        .assert_stdout_contains(child);

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha(base), updated_base_sha);
    assert_eq!(repo.get_commit_sha(child), updated_child_sha);
}

#[test]
fn get_downstack_does_not_sync_local_upstack() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["review-base", "review-child"]);
    let base = &branches[0];
    let child = &branches[1];
    repo.git(&["push", "-u", "origin", base, child])
        .assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    let original_child_sha = repo.get_commit_sha(child);

    let updated_base_sha =
        update_remote_branch_in_clone(&repo, base, "base-2.txt", "review base 2\n");
    let _updated_child_sha =
        update_remote_branch_in_clone(&repo, child, "child-2.txt", "review child 2\n");

    let out = repo.run_stax(&["get", base, "--downstack", "--no-checkout", "--no-restack"]);
    out.assert_success();

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.get_commit_sha(base), updated_base_sha);
    assert_eq!(repo.get_commit_sha(child), original_child_sha);
}

#[test]
fn get_without_branch_syncs_current_stack() {
    let repo = TestRepo::new_with_remote();
    push_remote_only_branch(&repo, "review-base", "base.txt", "review base\n");

    repo.run_stax(&["get", "review-base"]).assert_success();
    repo.run_stax(&["create", "my-child"]).assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child change");
    let child = repo.current_branch();

    let updated_parent_sha =
        update_remote_branch_in_clone(&repo, "review-base", "base-2.txt", "review base 2\n");

    let out = repo.run_stax(&["get", "--force"]);
    out.assert_success()
        .assert_stdout_contains("updated imported branch")
        .assert_stdout_contains("restacked 1");

    assert_eq!(repo.get_commit_sha("review-base"), updated_parent_sha);
    assert_eq!(repo.current_branch(), child);
}

#[test]
fn get_skips_branch_checked_out_in_another_worktree() {
    let repo = TestRepo::new_with_remote();
    let original_sha =
        push_remote_only_branch(&repo, "worktree-feature", "remote.txt", "remote content\n");
    repo.run_stax(&["get", "worktree-feature", "--no-checkout"])
        .assert_success();

    let linked_worktree = TempDir::new().expect("linked worktree");
    run_git_in(
        &repo.path(),
        &[
            "worktree",
            "add",
            linked_worktree.path().to_str().unwrap(),
            "worktree-feature",
        ],
    );
    let updated_sha = update_remote_branch_in_clone(
        &repo,
        "worktree-feature",
        "remote-2.txt",
        "remote content 2\n",
    );

    let out = repo.run_stax(&["get", "worktree-feature", "--no-checkout"]);
    out.assert_success()
        .assert_stdout_contains("checked out in another worktree")
        .assert_stdout_contains("Skipped");

    assert_ne!(original_sha, updated_sha);
    assert_eq!(repo.get_commit_sha("worktree-feature"), original_sha);
}

#[test]
fn submit_does_not_push_imported_support_branch() {
    let repo = TestRepo::new_with_remote();
    repo.configure_github_like_submit_remote();
    let original_remote_sha =
        push_remote_only_branch(&repo, "imported-parent", "parent.txt", "remote parent\n");

    repo.run_stax(&["get", "imported-parent"]).assert_success();
    repo.create_file("local-parent.txt", "local parent change\n");
    repo.commit("Local parent change");
    let local_parent_sha = repo.head_sha();

    repo.run_stax(&["create", "my-child"]).assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child change");
    let child = repo.current_branch();

    let out = repo.run_stax(&["downstack", "submit", "--no-pr", "--yes"]);
    out.assert_success();

    repo.git(&["fetch", "origin", "imported-parent"])
        .assert_success();
    assert_eq!(
        repo.get_commit_sha("origin/imported-parent"),
        original_remote_sha
    );
    assert_eq!(repo.get_commit_sha("imported-parent"), local_parent_sha);
    assert_eq!(
        repo.get_commit_sha(&format!("origin/{}", child)),
        repo.get_commit_sha(&child)
    );
}

#[test]
fn sync_restack_updates_imported_parent_then_rebases_child() {
    let repo = TestRepo::new_with_remote();
    push_remote_only_branch(&repo, "review-base", "base.txt", "review base\n");

    repo.run_stax(&["get", "review-base"]).assert_success();
    repo.run_stax(&["create", "my-child"]).assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child change");
    let child = repo.current_branch();

    let updated_parent_sha =
        update_remote_branch_in_clone(&repo, "review-base", "base-2.txt", "review base 2\n");

    let out = repo.run_stax(&["sync", "--restack", "--force"]);
    out.assert_success()
        .assert_stdout_contains("updated imported branch")
        .assert_stdout_contains("restacked 1");

    assert_eq!(repo.get_commit_sha("review-base"), updated_parent_sha);
    assert_eq!(repo.current_branch(), child);
    assert!(repo.path().join("base-2.txt").exists());
}

#[test]
fn sync_restack_skips_dirty_imported_parent_worktree_without_force() {
    let repo = TestRepo::new_with_remote();
    let original_parent_sha =
        push_remote_only_branch(&repo, "dirty-base", "base.txt", "review base\n");

    repo.run_stax(&["get", "dirty-base"]).assert_success();
    repo.run_stax(&["create", "my-child"]).assert_success();
    repo.create_file("child.txt", "child\n");
    repo.commit("Child change");

    let imported_worktree = TempDir::new().expect("imported worktree");
    run_git_in(
        &repo.path(),
        &[
            "worktree",
            "add",
            imported_worktree.path().to_str().unwrap(),
            "dirty-base",
        ],
    );
    std::fs::write(imported_worktree.path().join("local.txt"), "local edit\n")
        .expect("dirty imported worktree");

    update_remote_branch_in_clone(&repo, "dirty-base", "base-2.txt", "review base 2\n");

    let out = repo.run_stax(&["sync", "--restack"]);
    out.assert_success()
        .assert_stdout_contains("skipped imported branch")
        .assert_stdout_contains("All branches up to date");

    assert_eq!(repo.get_commit_sha("dirty-base"), original_parent_sha);
    assert!(!repo.path().join("base-2.txt").exists());
}

#[test]
fn sync_deletes_merged_imported_branch_locally_without_deleting_remote() {
    let repo = TestRepo::new_with_remote();
    let imported_sha =
        push_remote_only_branch(&repo, "borrowed-base", "base.txt", "borrowed base\n");

    repo.run_stax(&["get", "borrowed-base"]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.merge_branch_on_remote("borrowed-base");

    let out = repo.run_stax(&["sync", "--force"]);
    out.assert_success();

    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches
            .iter()
            .any(|branch| branch == "borrowed-base"),
        "sync must not delete imported remote branches, got: {:?}",
        remote_branches
    );
    repo.git(&["fetch", "origin", "borrowed-base"])
        .assert_success();
    assert_eq!(repo.get_commit_sha("origin/borrowed-base"), imported_sha);

    let local_branches = repo.list_branches();
    assert!(
        !local_branches
            .iter()
            .any(|branch| branch == "borrowed-base"),
        "sync must clean up merged imported support branches locally, got: {:?}",
        local_branches
    );
    repo.git(&["show-ref", "--verify", "refs/branch-metadata/borrowed-base"])
        .assert_failure();
}

#[test]
fn sync_delete_upstream_gone_deletes_imported_branch_locally() {
    let repo = TestRepo::new_with_remote();
    push_remote_only_branch(&repo, "borrowed-gone", "base.txt", "borrowed base\n");

    repo.run_stax(&["get", "borrowed-gone"]).assert_success();
    repo.git(&["checkout", "main"]).assert_success();

    // Land the imported branch's work on trunk and publish it so it carries no
    // commits unique relative to origin/main and stays a legitimate deletion
    // candidate under the #478 local-only-work safety guard.
    repo.git(&["merge", "--ff-only", "borrowed-gone"])
        .assert_success();
    repo.git(&["push", "origin", "main"]).assert_success();

    repo.git(&["push", "origin", "--delete", "borrowed-gone"])
        .assert_success();

    let out = repo.run_stax(&["sync", "--force", "--delete-upstream-gone"]);
    out.assert_success();

    let local_branches = repo.list_branches();
    assert!(
        !local_branches
            .iter()
            .any(|branch| branch == "borrowed-gone"),
        "upstream-gone cleanup must delete imported support branches locally, got: {:?}",
        local_branches
    );
    repo.git(&["show-ref", "--verify", "refs/branch-metadata/borrowed-gone"])
        .assert_failure();
}
