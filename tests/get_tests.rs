//! Integration tests for `stax get`.

use crate::common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

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

fn configure_submit_remote(repo: &TestRepo) {
    let remote_path = repo
        .remote_path()
        .expect("Expected remote path for repository with origin");
    let remote_path_str = remote_path.to_string_lossy().to_string();

    repo.git(&[
        "remote",
        "set-url",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ]);
    repo.git(&["remote", "set-url", "--push", "origin", &remote_path_str]);

    let file_url = format!("file://{}", remote_path_str);
    repo.git(&[
        "config",
        "--local",
        &format!("url.{}.insteadOf", file_url.trim_end_matches('/')),
        "https://github.com/test-owner/test-repo.git",
    ]);
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
fn get_refuses_to_overwrite_divergent_local_branch_without_force() {
    let repo = TestRepo::new_with_remote();
    let remote_sha =
        push_remote_only_branch(&repo, "divergent-feature", "remote.txt", "remote content\n");

    repo.git(&["checkout", "-b", "divergent-feature", "main"])
        .assert_success();
    repo.create_file("local.txt", "local content\n");
    repo.commit("Local divergent commit");
    let local_sha = repo.head_sha();
    repo.git(&["checkout", "main"]).assert_success();

    let out = repo.run_stax(&["get", "divergent-feature", "--no-checkout"]);
    out.assert_failure();

    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("already exists and differs"),
        "expected divergent-branch error, got:\n{}",
        stderr
    );
    assert_eq!(repo.get_commit_sha("divergent-feature"), local_sha);
    assert_ne!(repo.get_commit_sha("divergent-feature"), remote_sha);
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
fn submit_does_not_push_imported_support_branch() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);
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
