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
