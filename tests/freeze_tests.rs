use crate::common::{OutputAssertions, TestRepo};

#[test]
fn freeze_and_unfreeze_are_idempotent_and_persist_in_metadata() {
    let repo = TestRepo::new();
    let branch = repo.create_stack(&["freeze-me"]).remove(0);

    repo.run_stax(&["freeze"]).assert_success();
    repo.run_stax(&["freeze", &branch]).assert_success();
    let frozen = TestRepo::stdout(&repo.git(&["show", &format!("refs/branch-metadata/{branch}")]));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&frozen).unwrap()["frozen"],
        true
    );

    repo.run_stax(&["unfreeze", &branch]).assert_success();
    repo.run_stax(&["unfreeze"]).assert_success();
    let unfrozen =
        TestRepo::stdout(&repo.git(&["show", &format!("refs/branch-metadata/{branch}")]));
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&unfrozen).unwrap()["frozen"],
        false
    );
}

#[test]
fn restack_skips_frozen_branches_until_unfrozen() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["freeze-parent", "freeze-child"]);
    let child_before = repo.get_commit_sha(&branches[1]);

    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("parent-v2.txt", "parent update\n");
    repo.commit("Advance frozen parent");
    repo.run_stax(&["checkout", &branches[1]]).assert_success();
    repo.run_stax(&["freeze"]).assert_success();

    let frozen_restack = repo.run_stax(&["restack", "--yes"]);
    frozen_restack.assert_success();
    frozen_restack.assert_stdout_contains("frozen");
    assert_eq!(repo.get_commit_sha(&branches[1]), child_before);

    repo.run_stax(&["unfreeze"]).assert_success();
    repo.run_stax(&["restack", "--yes"]).assert_success();
    assert_ne!(repo.get_commit_sha(&branches[1]), child_before);
}

#[test]
fn upstack_restack_skips_frozen_descendants() {
    let repo = TestRepo::new();
    let branches = repo.create_stack(&["upstack-freeze-parent", "upstack-freeze-child"]);
    let child_before = repo.get_commit_sha(&branches[1]);
    repo.run_stax(&["freeze", &branches[1]]).assert_success();
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("upstack-parent-v2.txt", "parent update\n");
    repo.commit("Advance parent below frozen upstack child");

    let output = repo.run_stax(&["upstack", "restack"]);

    output.assert_success();
    output.assert_stdout_contains("frozen");
    assert_eq!(repo.get_commit_sha(&branches[1]), child_before);
}

#[test]
fn sync_restack_skips_frozen_branches() {
    let repo = TestRepo::new_with_remote();
    let branches = repo.create_stack(&["sync-freeze-parent", "sync-freeze-child"]);
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    let child_before = repo.get_commit_sha(&branches[1]);
    repo.run_stax(&["freeze", &branches[1]]).assert_success();
    repo.run_stax(&["checkout", &branches[0]]).assert_success();
    repo.create_file("sync-parent-v2.txt", "parent update\n");
    repo.commit("Advance parent below frozen sync child");
    repo.run_stax(&["checkout", &branches[1]]).assert_success();

    let output = repo.run_stax(&["sync", "--restack", "--no-delete"]);

    output.assert_success();
    output.assert_stdout_contains("frozen");
    assert_eq!(repo.get_commit_sha(&branches[1]), child_before);
}

#[test]
fn sync_skips_remote_updates_for_frozen_imported_branches() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["checkout", "-b", "frozen-imported"])
        .assert_success();
    repo.create_file("imported.txt", "imported v1\n");
    repo.commit("Create imported branch");
    repo.git(&["push", "-u", "origin", "frozen-imported"])
        .assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["branch", "-D", "frozen-imported"])
        .assert_success();
    repo.run_stax(&["get", "frozen-imported", "--no-checkout", "--no-restack"])
        .assert_success();
    repo.run_stax(&["freeze", "frozen-imported"])
        .assert_success();
    let frozen_before = repo.get_commit_sha("frozen-imported");

    repo.git(&["checkout", "-b", "remote-import-update", "frozen-imported"])
        .assert_success();
    repo.create_file("imported-v2.txt", "imported v2\n");
    repo.commit("Update imported branch remotely");
    repo.git(&["push", "origin", "HEAD:refs/heads/frozen-imported"])
        .assert_success();
    repo.git(&["checkout", "main"]).assert_success();
    repo.git(&["branch", "-D", "remote-import-update"])
        .assert_success();

    let output = repo.run_stax(&["sync", "--force", "--no-delete"]);

    output.assert_success();
    output.assert_stdout_contains("frozen");
    assert_eq!(repo.get_commit_sha("frozen-imported"), frozen_before);
}

#[test]
fn sync_preserves_frozen_child_when_squash_merged_parent_is_cleaned_up() {
    let repo = TestRepo::new_with_remote();
    let mut branches = repo.create_stack(&["frozen-squash-parent"]);
    repo.run_stax(&["create", "frozen-squash-child"])
        .assert_success();
    repo.create_file("frozen-child.txt", "child\n");
    repo.commit("Create frozen squash child");
    branches.push(repo.current_branch());
    repo.git(&["push", "-u", "origin", &branches[0], &branches[1]])
        .assert_success();
    repo.run_stax(&["freeze", &branches[1]]).assert_success();
    let child_before = repo.get_commit_sha(&branches[1]);

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--squash", &branches[0]])
        .assert_success();
    repo.git(&["commit", "-m", "Squash merge frozen parent"])
        .assert_success();
    repo.create_file("main-after-squash.txt", "later main work\n");
    repo.commit("Advance main after squash merge");
    repo.git(&["push", "origin", "main"]).assert_success();
    repo.git(&["push", "origin", "--delete", &branches[0]])
        .assert_success();
    repo.run_stax(&["checkout", &branches[1]]).assert_success();

    let output = repo.run_stax(&["sync", "--force"]);

    output.assert_success();
    output.assert_stdout_contains("frozen");
    output.assert_stdout_contains("was squash-merged");
    assert_eq!(repo.get_commit_sha(&branches[1]), child_before);
    let metadata =
        TestRepo::stdout(&repo.git(&["show", &format!("refs/branch-metadata/{}", branches[1])]));
    let metadata: serde_json::Value = serde_json::from_str(&metadata).expect("metadata JSON");
    assert_eq!(metadata["parentBranchName"], "main");
    assert_eq!(metadata["frozen"], true);
}

#[test]
fn freeze_rejects_trunk_and_untracked_branches() {
    let repo = TestRepo::new();
    repo.run_stax(&["freeze"])
        .assert_failure()
        .assert_stderr_contains("tracked branch");
    repo.git(&["checkout", "-b", "untracked-freeze"])
        .assert_success();
    repo.run_stax(&["freeze"])
        .assert_failure()
        .assert_stderr_contains("tracked branch");
}
