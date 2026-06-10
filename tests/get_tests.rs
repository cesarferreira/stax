//! Integration tests for `stax get`.

use crate::common;

use common::{OutputAssertions, TestRepo};

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
