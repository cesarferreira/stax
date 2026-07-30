use crate::common::{TestRepo, run_stax_in_script_with_env};

fn branch_exists(repo: &TestRepo, branch: &str) -> bool {
    repo.git(&["rev-parse", "--verify", &format!("refs/heads/{branch}")])
        .status
        .success()
}

fn receipt_count(repo: &TestRepo) -> usize {
    let ops_dir = repo.path().join(".git/stax/ops");
    if !ops_dir.exists() {
        return 0;
    }
    std::fs::read_dir(&ops_dir)
        .expect("read ops dir")
        .filter_map(Result::ok)
        .count()
}

/// Merged feature on remote, local trunk updated — same shape as sync_json_tests.
fn repo_with_merged_feature(prefix: &str) -> (TestRepo, String) {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);
    repo.run_stax(&["bc", prefix]);
    let feature = repo.current_branch();
    repo.create_file(&format!("{prefix}.txt"), "content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature]);
    repo.run_stax(&["t"]);
    repo.merge_branch_on_remote(&feature);
    repo.git(&["pull", "origin", "main"]);
    (repo, feature)
}

#[test]
fn sync_confirm_cancel_leaves_merged_branch_and_writes_no_receipt() {
    let (repo, feature) = repo_with_merged_feature("feat-plan-cancel");

    let before_receipts = receipt_count(&repo);
    let home = repo.clean_home();
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        "wait_for_tui_text \"How should sync proceed?\"; printf '\\033[B\\033[B\\n'",
        &[("HOME", &home)],
    );
    assert!(
        out.status.success(),
        "cancelled sync should exit 0; stderr: {}",
        TestRepo::stderr(&out)
    );
    assert!(
        branch_exists(&repo, &feature),
        "merged branch should remain after cancelling the plan"
    );
    assert_eq!(
        receipt_count(&repo),
        before_receipts,
        "cancelling before trunk update should not write a sync receipt"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(stdout.contains("Sync plan"), "stdout: {stdout}");
    assert!(stdout.contains("Aborted."), "stdout: {stdout}");
}

#[test]
fn sync_confirm_bulk_deletes_merged_branch_without_per_branch_prompt() {
    let (repo, feature) = repo_with_merged_feature("feat-plan-bulk");

    let home = repo.clean_home();
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        "wait_for_tui_text \"How should sync proceed?\"; printf '\\n'",
        &[("HOME", &home)],
    );
    assert!(
        out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&out)
    );
    assert!(
        !branch_exists(&repo, &feature),
        "bulk confirm should delete the merged branch"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(stdout.contains("Sync plan"), "stdout: {stdout}");
}

#[test]
fn sync_confirm_per_branch_mode_still_allows_skip() {
    let (repo, feature) = repo_with_merged_feature("feat-plan-per-branch");

    let home = repo.clean_home();
    // Per-branch (one down + enter), then decline delete.
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        "wait_for_tui_text \"How should sync proceed?\"; printf '\\033[B\\n'; wait_for_tui_text \"Delete '\"; printf 'n\\n'",
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(
        branch_exists(&repo, &feature),
        "per-branch mode should honor a declined delete prompt"
    );
}

#[test]
fn sync_force_skips_interactive_sync_plan() {
    let (repo, feature) = repo_with_merged_feature("feat-plan-force");

    let out = repo.run_stax(&["sync", "--force"]);
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("Sync plan"),
        "--force must not show the interactive plan: {stdout}"
    );
    assert!(
        !branch_exists(&repo, &feature),
        "force should still delete the branch"
    );
}
