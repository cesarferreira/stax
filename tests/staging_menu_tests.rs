//! Tests for the shared "no files staged" menu used by `stax modify` and
//! `stax create -m`.
//!
//! The interactive menu branches are driven through a pseudo-terminal because
//! `dialoguer::Select` reads `/dev/tty` rather than stdin. Non-interactive
//! tests still lock in the observable behaviour that this flow must preserve:
//!
//! - Non-TTY: bail immediately with an actionable error (no menu shown).
//! - `-a/--all`: bypass the menu entirely and force-stage.
//! - Pre-staged index: skip the menu and commit what's staged.
//!
mod common;

use common::{OutputAssertions, TestRepo};

fn select_menu_option(index: usize) -> String {
    let downs = "\\033[B".repeat(index);
    format!("sleep 1; printf '{}\\r'", downs)
}

fn select_patch_and_stage_first_hunk() -> String {
    let downs = "\\033[B";
    format!("sleep 1; printf '{}\\r'; sleep 1; printf 'y\\n'", downs)
}

fn head_file(repo: &TestRepo, path: &str) -> String {
    let output = repo.git(&["show", &format!("HEAD:{path}")]);
    assert!(
        output.status.success(),
        "failed to read {path} from HEAD: {}",
        TestRepo::stderr(&output)
    );
    TestRepo::stdout(&output)
}

fn assert_status_contains(repo: &TestRepo, path: &str) {
    let status = repo.git(&["status", "--porcelain"]);
    assert!(status.status.success(), "{}", TestRepo::stderr(&status));
    let stdout = TestRepo::stdout(&status);
    assert!(
        stdout.contains(path),
        "expected status to contain {path}, got:\n{}",
        stdout
    );
}

fn assert_status_clean(repo: &TestRepo) {
    let status = repo.git(&["status", "--porcelain"]);
    assert!(status.status.success(), "{}", TestRepo::stderr(&status));
    assert!(
        TestRepo::stdout(&status).trim().is_empty(),
        "expected clean status, got:\n{}",
        TestRepo::stdout(&status)
    );
}

// =============================================================================
// stax modify
// =============================================================================

#[test]
fn modify_non_tty_bails_when_nothing_staged() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-modify-bail"]).assert_success();
    repo.create_file("feature.txt", "initial");
    repo.commit("Initial feature");

    // Create an unstaged change — menu would fire in a TTY, but we're piped.
    repo.create_file("feature.txt", "modified without staging");

    let output = repo.run_stax(&["modify"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No files staged"),
        "expected non-TTY bail with 'No files staged', got stderr: {}",
        stderr
    );
    assert!(
        stderr.contains("stax modify -a") || stderr.contains("git add"),
        "expected guidance pointing to `-a` or `git add`, got: {}",
        stderr
    );
}

#[test]
fn modify_dash_a_still_bypasses_menu() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-dash-a"]).assert_success();
    repo.create_file("feature.txt", "initial");
    repo.commit("Initial feature");

    let sha_before = repo.head_sha();

    repo.create_file("feature.txt", "modified");
    let output = repo.run_stax(&["modify", "-a"]);
    output.assert_success();

    assert_ne!(
        sha_before,
        repo.head_sha(),
        "expected amend to rewrite HEAD"
    );
}

#[test]
fn modify_proceeds_when_something_is_pre_staged() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-prestaged"]).assert_success();
    repo.create_file("feature.txt", "v1");
    repo.commit("Initial feature");

    let sha_before = repo.head_sha();

    // Stage one file manually; another change stays unstaged.
    repo.create_file("staged.txt", "staged content");
    repo.git(&["add", "staged.txt"]).assert_success();
    repo.create_file("feature.txt", "unstaged change");

    let output = repo.run_stax(&["modify"]);
    output.assert_success();

    assert_ne!(
        sha_before,
        repo.head_sha(),
        "pre-staged file should have been amended in"
    );

    // The unstaged change stays on disk but was not included in the amend.
    let diff = repo.git(&["diff", "--name-only"]);
    let diff_out = String::from_utf8_lossy(&diff.stdout);
    assert!(
        diff_out.lines().any(|l| l == "feature.txt"),
        "unstaged file should remain unstaged, got: {}",
        diff_out
    );
}

#[test]
fn modify_interactive_stage_all_amends_changes() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-menu-stage-all"]).assert_success();
    repo.create_file("feature.txt", "initial\n");
    repo.commit("Initial feature");
    let sha_before = repo.head_sha();

    repo.create_file("feature.txt", "modified through menu\n");

    let script = select_menu_option(0);
    let output = common::run_stax_in_script(&repo.path(), &["modify"], &script);
    output.assert_success();

    assert_ne!(sha_before, repo.head_sha(), "modify should amend HEAD");
    assert_eq!(head_file(&repo, "feature.txt"), "modified through menu\n");
    assert_status_clean(&repo);
}

#[test]
fn modify_interactive_patch_amends_selected_hunk() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-menu-patch"]).assert_success();
    repo.create_file("feature.txt", "initial\n");
    repo.commit("Initial feature");
    let sha_before = repo.head_sha();

    repo.create_file("feature.txt", "patched through menu\n");

    let script = select_patch_and_stage_first_hunk();
    let output = common::run_stax_in_script(&repo.path(), &["modify"], &script);
    output.assert_success();

    assert_ne!(sha_before, repo.head_sha(), "modify should amend HEAD");
    assert_eq!(head_file(&repo, "feature.txt"), "patched through menu\n");
    assert_status_clean(&repo);
}

#[test]
fn modify_interactive_continue_amends_message_only() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-menu-continue"]).assert_success();
    repo.create_file("feature.txt", "initial\n");
    repo.commit("Initial feature");
    let sha_before = repo.head_sha();

    repo.create_file("feature.txt", "left unstaged\n");

    let script = select_menu_option(2);
    let output = common::run_stax_in_script(
        &repo.path(),
        &["modify", "-m", "Updated message only"],
        &script,
    );
    output.assert_success();

    assert_ne!(sha_before, repo.head_sha(), "message-only amend rewrites HEAD");
    let subject = repo.git(&["log", "-1", "--pretty=%s"]);
    assert!(subject.status.success(), "{}", TestRepo::stderr(&subject));
    assert_eq!(TestRepo::stdout(&subject).trim(), "Updated message only");
    assert_status_contains(&repo, "feature.txt");
}

#[test]
fn modify_interactive_abort_keeps_head_and_worktree() {
    let repo = TestRepo::new();
    repo.run_stax(&["bc", "feature-menu-abort"]).assert_success();
    repo.create_file("feature.txt", "initial\n");
    repo.commit("Initial feature");
    let sha_before = repo.head_sha();

    repo.create_file("feature.txt", "left after abort\n");

    let script = select_menu_option(3);
    let output = common::run_stax_in_script(&repo.path(), &["modify"], &script);
    output.assert_success();
    output.assert_stdout_contains("Aborted");

    assert_eq!(sha_before, repo.head_sha(), "abort should not rewrite HEAD");
    assert_status_contains(&repo, "feature.txt");
}

// =============================================================================
// stax create -m
// =============================================================================

#[test]
fn create_m_non_tty_bails_when_nothing_staged() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    // Working tree has an unstaged change, nothing in the index.
    repo.create_file("feature.txt", "work in progress");

    let output = repo.run_stax(&["create", "-m", "feature message"]);
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No files staged"),
        "expected 'No files staged' guidance, got: {}",
        stderr
    );
    assert!(
        stderr.contains("stax create -a") || stderr.contains("git add"),
        "expected guidance pointing to `-a` or `git add`, got: {}",
        stderr
    );

    // No branch should have been created — abort is a clean no-op.
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b.contains("feature-message")),
        "no branch should exist after aborted create, got: {:?}",
        branches
    );
}

#[test]
fn create_am_flag_still_bypasses_menu() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("feature.txt", "work");

    let output = repo.run_stax(&["create", "-a", "-m", "feature message"]);
    output.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("feature-message")),
        "expected new branch, got: {:?}",
        branches
    );
}

#[test]
fn create_m_proceeds_when_something_is_pre_staged() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();

    repo.create_file("staged.txt", "content");
    repo.git(&["add", "staged.txt"]).assert_success();
    // Another change stays unstaged — `-m` without `-a` should commit only
    // what's in the index already.
    repo.create_file("unstaged.txt", "extra");

    let output = repo.run_stax(&["create", "-m", "feature message"]);
    output.assert_success();

    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("feature-message")),
        "expected new branch, got: {:?}",
        branches
    );

    // The unstaged file should still be unstaged on the new branch — it was
    // never added to the index.
    let status = repo.git(&["status", "--porcelain"]);
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_out.lines().any(|l| l.contains("unstaged.txt")),
        "unstaged file should still be unstaged, got: {}",
        status_out
    );
}

#[test]
fn create_m_interactive_stage_all_commits_changes() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("feature.txt", "created through menu\n");

    let script = select_menu_option(0);
    let output =
        common::run_stax_in_script(&repo.path(), &["create", "-m", "menu stage all"], &script);
    output.assert_success();

    assert!(repo.current_branch().contains("menu-stage-all"));
    assert_eq!(head_file(&repo, "feature.txt"), "created through menu\n");
    assert_status_clean(&repo);
}

#[test]
fn create_m_interactive_patch_commits_selected_hunk() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    repo.create_file("README.md", "# Test Repo\n\npatched through menu\n");

    let script = select_patch_and_stage_first_hunk();
    let output =
        common::run_stax_in_script(&repo.path(), &["create", "-m", "menu patch"], &script);
    output.assert_success();

    assert!(repo.current_branch().contains("menu-patch"));
    assert_eq!(
        head_file(&repo, "README.md"),
        "# Test Repo\n\npatched through menu\n"
    );
    assert_status_clean(&repo);
}

#[test]
fn create_m_interactive_empty_branch_keeps_worktree_changes() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    let main_sha = repo.head_sha();
    repo.create_file("feature.txt", "left for later\n");

    let script = select_menu_option(2);
    let output =
        common::run_stax_in_script(&repo.path(), &["create", "-m", "menu empty"], &script);
    output.assert_success();

    assert!(repo.current_branch().contains("menu-empty"));
    assert_eq!(repo.head_sha(), main_sha, "empty branch should not commit");
    assert_status_contains(&repo, "feature.txt");
}

#[test]
fn create_m_interactive_abort_leaves_no_branch() {
    let repo = TestRepo::new();
    repo.run_stax(&["init"]).assert_success();
    let main_sha = repo.head_sha();
    repo.create_file("feature.txt", "left after abort\n");

    let script = select_menu_option(3);
    let output =
        common::run_stax_in_script(&repo.path(), &["create", "-m", "menu abort"], &script);
    output.assert_success();
    output.assert_stdout_contains("Aborted");

    assert_eq!(repo.current_branch(), "main");
    assert_eq!(repo.head_sha(), main_sha, "abort should not commit");
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|branch| branch.contains("menu-abort")),
        "abort should not create a branch, got: {:?}",
        branches
    );
    assert_status_contains(&repo, "feature.txt");
}
