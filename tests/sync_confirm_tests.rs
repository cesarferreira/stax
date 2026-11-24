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
    assert!(
        stdout.contains("Found") && stdout.contains("merged"),
        "plan should list merged branches before the delete prompt; stdout: {stdout}"
    );
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
fn sync_does_not_prompt_when_only_trunk_moves() {
    let repo = TestRepo::new_with_remote();
    repo.git(&["push", "-u", "origin", "main"]);
    // Advance origin/main only — nothing to delete, nothing to restack.
    repo.simulate_remote_commit("upstream.txt", "content", "Upstream commit");

    let out = repo.run_stax(&["sync"]);
    assert!(
        out.status.success(),
        "sync failed: {}",
        TestRepo::stderr(&out)
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("Sync plan") && !stdout.contains("How should sync proceed?"),
        "a plain trunk fast-forward must not prompt: {stdout}"
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

/// Picks "Cancel sync" if the plan prompt appears, so a regression fails the
/// branch assertion instead of hanging on a prompt nobody answers.
const CANCEL_IF_PLAN_PROMPTS: &str = "if STAX_TUI_WAIT_ATTEMPTS=40 wait_for_tui_text \"How should sync proceed?\"; then printf '\\033[B\\033[B\\n'; fi";

fn write_home_config(home: &str, config_toml: &str) {
    std::fs::write(
        std::path::Path::new(home).join(".config/stax/config.toml"),
        config_toml,
    )
    .expect("write config");
}

#[test]
fn sync_confirm_delete_false_deletes_merged_branch_without_prompt() {
    let (repo, feature) = repo_with_merged_feature("feat-no-confirm");

    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = false\n");
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        CANCEL_IF_PLAN_PROMPTS,
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(
        !branch_exists(&repo, &feature),
        "confirm_delete = false should delete the merged branch"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("How should sync proceed?") && !stdout.contains("Delete '"),
        "confirm_delete = false must not prompt: {stdout}"
    );
}

#[test]
fn get_confirm_delete_false_deletes_merged_branch_without_prompt() {
    let (repo, feature) = repo_with_merged_feature("feat-get-no-confirm");

    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = false\n");
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["get"],
        CANCEL_IF_PLAN_PROMPTS,
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(
        !branch_exists(&repo, &feature),
        "`st get` should delete the merged branch without asking"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("How should sync proceed?") && !stdout.contains("Delete '"),
        "confirm_delete = false must not prompt: {stdout}"
    );
}

#[test]
fn sync_confirm_delete_false_still_asks_about_linked_worktree() {
    let (repo, feature) = repo_with_merged_feature("feat-no-confirm-worktree");
    let worktree_root = tempfile::tempdir().expect("create worktree root");
    let worktree = worktree_root.path().join("feature-worktree");
    assert!(
        repo.git(&[
            "worktree",
            "add",
            worktree.to_str().expect("utf8 worktree path"),
            &feature,
        ])
        .status
        .success()
    );

    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = false\n");
    // Second option: remove the worktree and delete the branch.
    let out = run_stax_in_script_with_env(
        &repo.path(),
        &["sync"],
        "wait_for_tui_text \"What should stax do?\"; printf '\\033[B\\n'",
        &[("HOME", &home)],
    );
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    let stdout = TestRepo::stdout(&out);
    assert!(
        !stdout.contains("How should sync proceed?"),
        "confirm_delete = false should skip the plan prompt: {stdout}"
    );
    assert!(
        !worktree.exists(),
        "chosen action should remove the worktree"
    );
    assert!(!branch_exists(&repo, &feature));
}

#[test]
fn sync_confirm_delete_false_keeps_quiet_mode_non_destructive() {
    let (repo, feature) = repo_with_merged_feature("feat-no-confirm-quiet");

    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = false\n");
    let out = repo.run_stax(&["sync", "--quiet"]);
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(
        branch_exists(&repo, &feature),
        "--quiet should still skip deletions without --force"
    );
}

#[test]
fn sync_rejects_non_bool_confirm_delete() {
    let (repo, feature) = repo_with_merged_feature("feat-bad-confirm");

    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = \"never\"\n");
    let out = repo.run_stax(&["sync", "--force"]);
    assert!(!out.status.success(), "invalid config should fail sync");
    assert!(
        TestRepo::stderr(&out).contains("confirm_delete"),
        "error should name the bad key: {}",
        TestRepo::stderr(&out)
    );
    assert!(branch_exists(&repo, &feature));
}

fn rev(repo: &TestRepo, name: &str) -> String {
    TestRepo::stdout(&repo.git(&["rev-parse", name]))
        .trim()
        .to_string()
}

fn next_hint_count(stdout: &str) -> usize {
    stdout
        .lines()
        .filter(|line| line.trim_start().starts_with("Next:"))
        .count()
}

fn assert_non_tty_kept_with_hint(repo: &TestRepo, feature: &str, out: &std::process::Output) {
    let stdout = TestRepo::stdout(out);
    let stderr = TestRepo::stderr(out);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    assert!(
        branch_exists(repo, feature),
        "merged branch must be kept without a terminal"
    );
    assert!(
        !stderr.contains("not a terminal"),
        "must not hit a dialoguer failure: {stderr}"
    );
    assert!(
        !stdout.contains("Sync plan") && !stdout.contains("How should sync proceed?"),
        "plan prompt must be skipped: {stdout}"
    );
    assert!(
        stdout.contains(&format!(
            "Cleanup skipped for {feature} (no terminal to confirm deletion)"
        )),
        "skip reason missing: {stdout}"
    );
    assert!(stdout.contains("st sync --force"), "hint missing: {stdout}");
    assert!(
        stdout.contains("sync.confirm_delete = false"),
        "hint missing: {stdout}"
    );
    assert_eq!(next_hint_count(&stdout), 1, "stdout: {stdout}");
}

#[test]
fn non_tty_rs_keeps_merged_branch_with_hint() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-rs");
    repo.simulate_remote_commit("upstream.txt", "upstream", "Upstream commit");

    let out = repo.run_stax(&["rs"]);
    assert_non_tty_kept_with_hint(&repo, &feature, &out);
    assert_eq!(
        rev(&repo, "main"),
        rev(&repo, "origin/main"),
        "trunk should still be updated"
    );
}

#[test]
fn non_tty_pull_keeps_merged_branch_with_hint() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-pull");
    repo.simulate_remote_commit("upstream.txt", "upstream", "Upstream commit");

    let out = repo.run_stax(&["pull"]);
    assert_non_tty_kept_with_hint(&repo, &feature, &out);
    assert_eq!(rev(&repo, "main"), rev(&repo, "origin/main"));
}

#[test]
fn non_tty_refresh_delete_merged_keeps_merged_branch() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-refresh");

    let out = repo.run_stax(&["refresh", "--no-submit", "--delete-merged"]);
    let stdout = TestRepo::stdout(&out);
    assert!(
        out.status.success(),
        "stdout: {stdout}\nstderr: {}",
        TestRepo::stderr(&out)
    );
    assert!(branch_exists(&repo, &feature));
    assert!(
        stdout.contains("no terminal to confirm deletion"),
        "reason missing: {stdout}"
    );
}

#[test]
fn non_tty_confirm_delete_false_still_deletes() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-nodelconfirm");
    let home = repo.clean_home();
    write_home_config(&home, "[sync]\nconfirm_delete = false\n");

    let out = repo.run_stax(&["rs"]);
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(!branch_exists(&repo, &feature));
}

#[test]
fn non_tty_force_still_deletes_merged_branch() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-force");

    let out = repo.run_stax(&["rs", "--force"]);
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(!branch_exists(&repo, &feature));
}

#[test]
fn non_tty_quiet_keeps_merged_branch() {
    let (repo, feature) = repo_with_merged_feature("feat-non-tty-quiet");

    let out = repo.run_stax(&["sync", "--quiet"]);
    assert!(out.status.success(), "stderr: {}", TestRepo::stderr(&out));
    assert!(branch_exists(&repo, &feature));
}

fn assert_non_tty_keeps_linked_worktree(prefix: &str, config: Option<&str>) {
    let (repo, feature) = repo_with_merged_feature(prefix);
    let worktree_root = tempfile::tempdir().expect("create worktree root");
    let worktree = worktree_root.path().join("feature-worktree");
    assert!(
        repo.git(&[
            "worktree",
            "add",
            worktree.to_str().expect("utf8 worktree path"),
            &feature,
        ])
        .status
        .success()
    );
    if let Some(config) = config {
        write_home_config(&repo.clean_home(), config);
    }

    let out = repo.run_stax(&["sync"]);
    let stdout = TestRepo::stdout(&out);
    assert!(
        out.status.success(),
        "stdout: {stdout}\nstderr: {}",
        TestRepo::stderr(&out)
    );
    assert!(branch_exists(&repo, &feature));
    assert!(worktree.exists(), "linked worktree must be left untouched");
    let head = repo.git(&[
        "-C",
        worktree.to_str().expect("utf8 path"),
        "rev-parse",
        "--abbrev-ref",
        "HEAD",
    ]);
    assert_eq!(
        TestRepo::stdout(&head).trim(),
        feature,
        "worktree HEAD must not be switched"
    );
    assert!(
        stdout.contains("no terminal to confirm deletion"),
        "reason missing: {stdout}"
    );
    assert!(
        !stdout.contains("What should stax do?"),
        "must not prompt: {stdout}"
    );
}

#[test]
fn non_tty_confirm_delete_false_skips_linked_worktree_branch() {
    assert_non_tty_keeps_linked_worktree(
        "feat-non-tty-wt-nodel",
        Some("[sync]\nconfirm_delete = false\n"),
    );
}

#[test]
fn non_tty_default_skips_linked_worktree_branch() {
    assert_non_tty_keeps_linked_worktree("feat-non-tty-wt-default", None);
}
