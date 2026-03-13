mod common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn clean_home(repo: &TestRepo) -> String {
    let home = repo.path().join(".test-home");
    fs::create_dir_all(home.join(".config")).expect("create clean home");
    home.to_string_lossy().into_owned()
}

fn linked_worktree_dirs(repo: &TestRepo) -> Vec<PathBuf> {
    let root = repo.path().join(".worktrees");
    if !root.exists() {
        return Vec::new();
    }

    fs::read_dir(root)
        .expect("read .worktrees")
        .map(|entry| entry.expect("read dir entry").path())
        .collect()
}

#[test]
fn wt_create_without_name_creates_random_lane() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    let out = repo.run_stax_with_env(&["wt", "c"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktrees = linked_worktree_dirs(&repo);
    assert_eq!(worktrees.len(), 1, "expected one linked worktree");

    let slug = worktrees[0]
        .file_name()
        .expect("worktree dir name")
        .to_string_lossy()
        .into_owned();

    assert!(
        slug.contains('-')
            && slug
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-'),
        "expected kebab-case random slug, got {}",
        slug
    );
    assert!(
        repo.list_branches()
            .iter()
            .any(|branch| branch.ends_with(&slug)),
        "expected a branch ending with '{}', got {:?}",
        slug,
        repo.list_branches()
    );

    let gitignore = fs::read_to_string(repo.path().join(".gitignore")).unwrap_or_default();
    assert!(
        gitignore.contains(".worktrees"),
        "expected .gitignore to contain .worktrees, got:\n{}",
        gitignore
    );
}

#[test]
fn wt_create_reuses_existing_worktree_target() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["create", "feature-lane"], &[("HOME", home.as_str())])
        .assert_success();
    repo.run_stax_with_env(&["checkout", "main"], &[("HOME", home.as_str())])
        .assert_success();

    repo.run_stax_with_env(&["wt", "c", "feature-lane"], &[("HOME", home.as_str())])
        .assert_success();
    let before = linked_worktree_dirs(&repo);
    assert_eq!(before.len(), 1);
    assert!(before[0].ends_with("feature-lane"));

    let out = repo.run_stax_with_env(&["wt", "c", "feature-lane"], &[("HOME", home.as_str())]);
    out.assert_success();

    let after = linked_worktree_dirs(&repo);
    assert_eq!(after.len(), 1, "should not create a duplicate worktree");
    assert!(
        TestRepo::stderr(&out).contains("Opening"),
        "expected existing worktree handoff, got stderr:\n{}",
        TestRepo::stderr(&out)
    );
}

#[test]
fn wt_create_with_agent_launches_in_new_worktree() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);
    let bin_dir = repo.path().join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("create fake bin");
    let log_path = repo.path().join("codex.log");
    let codex_path = bin_dir.join("codex");
    fs::write(
        &codex_path,
        r#"#!/bin/sh
printf 'cwd=%s\n' "$PWD" > "$STAX_TEST_LOG"
for arg in "$@"; do
  printf 'arg=%s\n' "$arg" >> "$STAX_TEST_LOG"
done
"#,
    )
    .expect("write fake codex");
    let mut perms = fs::metadata(&codex_path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&codex_path, perms).expect("chmod fake codex");

    let path_env = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let log_str = log_path.to_string_lossy().into_owned();

    let out = repo.run_stax_with_env(
        &[
            "wt",
            "c",
            "launch-me",
            "--agent",
            "codex",
            "--",
            "fix flaky test",
        ],
        &[
            ("HOME", home.as_str()),
            ("PATH", path_env.as_str()),
            ("STAX_TEST_LOG", log_str.as_str()),
        ],
    );
    out.assert_success();

    let log = fs::read_to_string(&log_path).expect("read codex log");
    assert!(
        log.contains(".worktrees/launch-me"),
        "expected launch cwd inside new worktree, got:\n{}",
        log
    );
    assert!(
        log.contains("arg=fix flaky test"),
        "expected trailing args to reach the agent, got:\n{}",
        log
    );
}

#[test]
fn wt_ls_stays_compact_and_wt_ll_shows_status() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "status-lane"], &[("HOME", home.as_str())])
        .assert_success();

    let ls = repo.run_stax_with_env(&["wt", "ls"], &[("HOME", home.as_str())]);
    ls.assert_success();
    let ls_stdout = TestRepo::stdout(&ls);
    assert!(ls_stdout.contains("NAME"));
    assert!(ls_stdout.contains("BRANCH"));
    assert!(ls_stdout.contains("PATH"));
    assert!(
        !ls_stdout.contains("STATUS"),
        "default ls should stay compact:\n{}",
        ls_stdout
    );

    let ll = repo.run_stax_with_env(&["wt", "ll"], &[("HOME", home.as_str())]);
    ll.assert_success();
    let ll_stdout = TestRepo::stdout(&ll);
    assert!(ll_stdout.contains("STATUS"));
    assert!(ll_stdout.contains("managed"));
}

#[test]
fn wt_prune_cleans_stale_git_worktree_entries() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "prune-me"], &[("HOME", home.as_str())])
        .assert_success();
    let worktree_path = repo.path().join(".worktrees").join("prune-me");
    fs::remove_dir_all(&worktree_path).expect("manually delete worktree path");

    let ll = repo.run_stax_with_env(&["wt", "ll"], &[("HOME", home.as_str())]);
    ll.assert_success();
    assert!(
        TestRepo::stdout(&ll).contains("prunable"),
        "expected prunable status before prune, got:\n{}",
        TestRepo::stdout(&ll)
    );

    let prune = repo.run_stax_with_env(&["wt", "prune"], &[("HOME", home.as_str())]);
    prune.assert_success();
    assert!(
        TestRepo::stdout(&prune).contains("Pruned"),
        "expected prune summary, got:\n{}",
        TestRepo::stdout(&prune)
    );

    let ls = repo.run_stax_with_env(&["wt", "ls"], &[("HOME", home.as_str())]);
    ls.assert_success();
    assert!(
        !TestRepo::stdout(&ls).contains("prune-me"),
        "expected stale worktree to be removed from git bookkeeping"
    );
}

#[test]
fn wt_remove_without_name_removes_current_worktree() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "remove-me"], &[("HOME", home.as_str())])
        .assert_success();
    let worktree_path = repo.path().join(".worktrees").join("remove-me");

    let out = repo.run_stax_in_with_env(&worktree_path, &["wt", "rm"], &[("HOME", home.as_str())]);
    out.assert_success();
    assert!(
        !worktree_path.exists(),
        "expected current worktree directory to be removed"
    );
}

#[test]
fn wt_restack_only_touches_stax_managed_worktrees() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "managed-lane"], &[("HOME", home.as_str())])
        .assert_success();
    repo.git(&["branch", "raw-side"]).assert_success();
    let raw_path = repo.path().join("raw-side");
    repo.git(&["worktree", "add", raw_path.to_str().unwrap(), "raw-side"])
        .assert_success();

    let out = repo.run_stax_with_env(&["wt", "rs"], &[("HOME", home.as_str())]);
    out.assert_success();
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("managed-lane"),
        "expected managed lane to be restacked, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("raw-side"),
        "expected unmanaged raw worktree to be skipped, got:\n{}",
        stdout
    );
}
