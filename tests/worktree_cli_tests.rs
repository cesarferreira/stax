mod common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

fn clean_home(repo: &TestRepo) -> String {
    let home = repo.path().join(".test-home");
    fs::create_dir_all(home.join(".config").join("stax")).expect("create clean home");
    home.to_string_lossy().into_owned()
}

fn default_worktree_root(repo: &TestRepo, home: &str) -> PathBuf {
    let repo_name = repo
        .path()
        .file_name()
        .expect("repo dir name")
        .to_string_lossy()
        .into_owned();
    PathBuf::from(home)
        .join(".stax")
        .join("worktrees")
        .join(repo_name)
}

fn linked_worktree_dirs(root: &PathBuf) -> Vec<PathBuf> {
    if !root.exists() {
        return Vec::new();
    }

    fs::read_dir(root)
        .expect("read worktree root")
        .map(|entry| entry.expect("read dir entry").path())
        .filter(|path| path.is_dir())
        .collect()
}

fn write_worktree_config(home: &str, root_dir: &str) {
    let config_dir = PathBuf::from(home).join(".config").join("stax");
    fs::create_dir_all(&config_dir).expect("create config dir");
    fs::write(
        config_dir.join("config.toml"),
        format!("[worktree]\nroot_dir = \"{}\"\n", root_dir),
    )
    .expect("write config.toml");
}

fn write_executable(path: &PathBuf, content: &str) {
    fs::write(path, content).expect("write executable");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod executable");
}

fn linked_worktree_dirs_default(repo: &TestRepo, home: &str) -> Vec<PathBuf> {
    let root = default_worktree_root(repo, home);
    if !root.exists() {
        return Vec::new();
    }

    linked_worktree_dirs(&root)
}

fn setup_fake_tmux_env(repo: &TestRepo) -> (PathBuf, String, String, String) {
    let bin_dir = repo.path().join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("create fake bin");

    let tmux_log = repo.path().join("tmux.log");
    let agent_log = repo.path().join("agent.log");
    let tmux_state = repo.path().join("tmux-state");
    fs::create_dir_all(&tmux_state).expect("create tmux state");

    write_executable(
        &bin_dir.join("tmux"),
        r#"#!/bin/sh
set -eu
cmd="${1:-}"
if [ "$cmd" = "-V" ]; then
  echo "tmux 3.4"
  exit 0
fi
shift || true
log="${STAX_TMUX_LOG:?}"
state="${STAX_TMUX_STATE_DIR:?}"
printf 'cmd=%s args=%s\n' "$cmd" "$*" >> "$log"
case "$cmd" in
  has-session)
    if [ "${1:-}" = "-t" ]; then
      session="${2:-}"
    else
      session="${1:-}"
    fi
    if [ -f "$state/$session" ]; then
      exit 0
    fi
    exit 1
    ;;
  new-session)
    detached=0
    session=""
    while [ "$#" -gt 0 ]; do
      case "$1" in
        -d)
          detached=1
          shift
          ;;
        -s)
          session="$2"
          shift 2
          ;;
        *)
          break
          ;;
      esac
    done
    : > "$state/$session"
    printf 'new=%s detached=%s\n' "$session" "$detached" >> "$log"
    if [ "$#" -gt 0 ]; then
      "$@"
    fi
    ;;
  attach-session)
    if [ "${1:-}" = "-t" ]; then
      session="${2:-}"
    else
      session="${1:-}"
    fi
    printf 'attach=%s\n' "$session" >> "$log"
    ;;
  switch-client)
    if [ "${1:-}" = "-t" ]; then
      session="${2:-}"
    else
      session="${1:-}"
    fi
    printf 'switch=%s\n' "$session" >> "$log"
    ;;
  *)
    echo "unsupported tmux command: $cmd" >&2
    exit 1
    ;;
esac
"#,
    );

    write_executable(
        &bin_dir.join("codex"),
        r#"#!/bin/sh
set -eu
log="${STAX_AGENT_LOG:?}"
printf 'cwd=%s\n' "$PWD" >> "$log"
for arg in "$@"; do
  printf 'arg=%s\n' "$arg" >> "$log"
done
"#,
    );

    let path_env = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    (
        bin_dir,
        path_env,
        tmux_log.to_string_lossy().into_owned(),
        agent_log.to_string_lossy().into_owned(),
    )
}

#[test]
fn wt_without_subcommand_prints_help_noninteractive() {
    let repo = TestRepo::new();

    let out = repo.run_stax(&["wt"]);
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Usage: worktree [COMMAND]"),
        "expected worktree help output, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Create or enter a worktree lane"),
        "expected worktree subcommand help, got:\n{}",
        stdout
    );
}

#[test]
fn wt_create_without_name_creates_random_lane() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    let out = repo.run_stax_with_env(&["wt", "c"], &[("HOME", home.as_str())]);
    out.assert_success();
    let stderr = TestRepo::stderr(&out);
    assert!(
        stderr.contains("You're in a new copy of"),
        "expected conductor-style creation message, got:\n{}",
        stderr
    );
    assert!(
        stderr.contains("and copied"),
        "expected copied-files summary, got:\n{}",
        stderr
    );

    let worktrees = linked_worktree_dirs_default(&repo, &home);
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
        !gitignore.contains(".worktrees"),
        "default external worktree root should not touch .gitignore, got:\n{}",
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
    let before = linked_worktree_dirs_default(&repo, &home);
    assert_eq!(before.len(), 1);
    assert!(before[0].ends_with("feature-lane"));

    let out = repo.run_stax_with_env(&["wt", "c", "feature-lane"], &[("HOME", home.as_str())]);
    out.assert_success();

    let after = linked_worktree_dirs_default(&repo, &home);
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
    assert!(
        TestRepo::stderr(&out).contains("Branched"),
        "expected branch source summary, got:\n{}",
        TestRepo::stderr(&out)
    );

    let log = fs::read_to_string(&log_path).expect("read codex log");
    let expected_worktree_root = default_worktree_root(&repo, &home);
    assert!(
        log.contains(
            expected_worktree_root
                .join("launch-me")
                .to_string_lossy()
                .as_ref()
        ),
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
fn wt_tmux_creates_then_attaches_existing_session() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);
    let (_bin_dir, path_env, tmux_log, agent_log) = setup_fake_tmux_env(&repo);
    let tmux_state = repo.path().join("tmux-state");
    let tmux_state_str = tmux_state.to_string_lossy().into_owned();

    let out = repo.run_stax_with_env(
        &[
            "wt",
            "c",
            "tmux-lane",
            "--agent",
            "codex",
            "--tmux",
            "--",
            "fix flaky test",
        ],
        &[
            ("HOME", home.as_str()),
            ("PATH", path_env.as_str()),
            ("STAX_TMUX_LOG", tmux_log.as_str()),
            ("STAX_TMUX_STATE_DIR", tmux_state_str.as_str()),
            ("STAX_AGENT_LOG", agent_log.as_str()),
        ],
    );
    out.assert_success();

    let out = repo.run_stax_with_env(
        &["wt", "go", "tmux-lane", "--agent", "codex", "--tmux"],
        &[
            ("HOME", home.as_str()),
            ("PATH", path_env.as_str()),
            ("STAX_TMUX_LOG", tmux_log.as_str()),
            ("STAX_TMUX_STATE_DIR", tmux_state_str.as_str()),
            ("STAX_AGENT_LOG", agent_log.as_str()),
        ],
    );
    out.assert_success();

    let tmux_log_contents = fs::read_to_string(&tmux_log).expect("read tmux log");
    assert!(
        tmux_log_contents.contains("new=tmux-lane"),
        "expected tmux session to be created with worktree name, got:\n{}",
        tmux_log_contents
    );
    assert!(
        tmux_log_contents.contains("attach=tmux-lane"),
        "expected existing session to attach on revisit, got:\n{}",
        tmux_log_contents
    );

    let agent_log_contents = fs::read_to_string(&agent_log).expect("read agent log");
    assert_eq!(
        agent_log_contents.matches("cwd=").count(),
        1,
        "expected agent to launch only once when tmux session is first created, got:\n{}",
        agent_log_contents
    );
    assert!(
        agent_log_contents.contains("arg=fix flaky test"),
        "expected initial agent args to be preserved inside tmux session, got:\n{}",
        agent_log_contents
    );
}

#[test]
fn wt_tmux_switches_client_when_already_inside_tmux() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);
    let (_bin_dir, path_env, tmux_log, agent_log) = setup_fake_tmux_env(&repo);
    let tmux_state = repo.path().join("tmux-state");
    let tmux_state_str = tmux_state.to_string_lossy().into_owned();

    let out = repo.run_stax_with_env(
        &["wt", "c", "inside-tmux", "--tmux"],
        &[
            ("HOME", home.as_str()),
            ("PATH", path_env.as_str()),
            ("STAX_TMUX_LOG", tmux_log.as_str()),
            ("STAX_TMUX_STATE_DIR", tmux_state_str.as_str()),
            ("STAX_AGENT_LOG", agent_log.as_str()),
            ("TMUX", "1"),
        ],
    );
    out.assert_success();

    let tmux_log_contents = fs::read_to_string(&tmux_log).expect("read tmux log");
    assert!(
        tmux_log_contents.contains("new=inside-tmux detached=1"),
        "expected detached session creation inside tmux, got:\n{}",
        tmux_log_contents
    );
    assert!(
        tmux_log_contents.contains("switch=inside-tmux"),
        "expected switch-client inside tmux, got:\n{}",
        tmux_log_contents
    );
}

#[test]
fn wt_create_prints_cd_hint_when_shell_env_is_set_without_shell_wrapper() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    let out = repo.run_stax_with_env(
        &["wt", "c", "shell-env-lane"],
        &[("HOME", home.as_str()), ("STAX_SHELL_INTEGRATION", "1")],
    );
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    let expected_path = default_worktree_root(&repo, &home).join("shell-env-lane");
    assert!(
        stdout.contains(expected_path.to_string_lossy().as_ref()),
        "expected direct cd hint when shell wrapper is not active, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("cd "),
        "expected cd hint when shell wrapper is not active, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Current shell did not move automatically."),
        "expected explicit non-navigation warning, got:\n{}",
        stdout
    );
}

#[test]
fn wt_go_prints_cd_hint_when_shell_env_is_set_without_shell_wrapper() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "go-shell-env"], &[("HOME", home.as_str())])
        .assert_success();

    let out = repo.run_stax_with_env(
        &["wt", "go", "go-shell-env"],
        &[("HOME", home.as_str()), ("STAX_SHELL_INTEGRATION", "1")],
    );
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    let expected_path = default_worktree_root(&repo, &home).join("go-shell-env");
    assert!(
        stdout.contains(expected_path.to_string_lossy().as_ref()),
        "expected direct cd hint when shell wrapper is not active, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("cd "),
        "expected cd hint when shell wrapper is not active, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("Current shell did not move automatically."),
        "expected explicit non-navigation warning, got:\n{}",
        stdout
    );
}

#[test]
fn wt_create_without_shell_integration_suggests_install() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    let out = repo.run_stax_with_env(&["wt", "c", "install-shell"], &[("HOME", home.as_str())]);
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Current shell did not move automatically."),
        "expected explicit non-navigation warning, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("stax shell-setup --install"),
        "expected shell integration install hint, got:\n{}",
        stdout
    );
}

#[test]
fn wt_go_without_shell_integration_suggests_install() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "go-install-shell"], &[("HOME", home.as_str())])
        .assert_success();

    let out = repo.run_stax_with_env(
        &["wt", "go", "go-install-shell"],
        &[("HOME", home.as_str())],
    );
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Current shell did not move automatically."),
        "expected explicit non-navigation warning, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("stax shell-setup --install"),
        "expected shell integration install hint, got:\n{}",
        stdout
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
fn wt_create_respects_explicit_repo_local_root_dir() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);
    write_worktree_config(&home, ".worktrees");

    let out = repo.run_stax_with_env(&["wt", "c", "local-root"], &[("HOME", home.as_str())]);
    out.assert_success();

    let worktrees = linked_worktree_dirs(&repo.path().join(".worktrees"));
    assert_eq!(worktrees.len(), 1, "expected repo-local worktree root");
    assert!(worktrees[0].ends_with("local-root"));

    let gitignore = fs::read_to_string(repo.path().join(".gitignore")).unwrap_or_default();
    assert!(
        gitignore.contains(".worktrees"),
        "expected explicit repo-local root to update .gitignore, got:\n{}",
        gitignore
    );
}

#[test]
fn wt_prune_cleans_stale_git_worktree_entries() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "prune-me"], &[("HOME", home.as_str())])
        .assert_success();
    let worktree_path = default_worktree_root(&repo, &home).join("prune-me");
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
fn wt_cleanup_prunes_and_removes_safe_candidates() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "merged-lane"], &[("HOME", home.as_str())])
        .assert_success();
    repo.run_stax_with_env(&["wt", "c", "prune-me"], &[("HOME", home.as_str())])
        .assert_success();

    let worktree_root = default_worktree_root(&repo, &home);
    let merged_path = worktree_root.join("merged-lane");
    let prune_path = worktree_root.join("prune-me");
    fs::remove_dir_all(&prune_path).expect("manually delete prune-me worktree");

    let detached_path = repo.path().join("detached-raw");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        detached_path.to_str().unwrap(),
        "main",
    ])
    .assert_success();

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--no-ff", "merged-lane", "-m", "Merge merged-lane"])
        .assert_success();

    let out = repo.run_stax_with_env(&["wt", "cleanup", "--yes"], &[("HOME", home.as_str())]);
    out.assert_success()
        .assert_stdout_contains("Pruned")
        .assert_stdout_contains("Found 2 cleanup candidates:")
        .assert_stdout_contains("Removed  worktree 'merged-lane'")
        .assert_stdout_contains("Removed  worktree 'detached-raw'");

    assert!(
        !merged_path.exists(),
        "expected merged managed worktree to be removed"
    );
    assert!(
        !detached_path.exists(),
        "expected detached worktree to be removed"
    );

    let show_ref = repo.git(&["show-ref", "--verify", "--quiet", "refs/heads/merged-lane"]);
    assert!(
        show_ref.status.success(),
        "cleanup should not delete merged branch refs by default"
    );

    let ls = repo.run_stax_with_env(&["wt", "ls"], &[("HOME", home.as_str())]);
    ls.assert_success();
    let stdout = TestRepo::stdout(&ls);
    assert!(
        !stdout.contains("merged-lane"),
        "expected merged-lane worktree to disappear from ls"
    );
    assert!(
        !stdout.contains("detached-raw"),
        "expected detached worktree to disappear from ls"
    );
    assert!(
        !stdout.contains("prune-me"),
        "expected stale worktree bookkeeping to be pruned"
    );
}

#[test]
fn wt_cleanup_dry_run_previews_without_applying() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "merged-lane"], &[("HOME", home.as_str())])
        .assert_success();
    repo.run_stax_with_env(&["wt", "c", "prune-me"], &[("HOME", home.as_str())])
        .assert_success();

    let worktree_root = default_worktree_root(&repo, &home);
    let merged_path = worktree_root.join("merged-lane");
    let prune_path = worktree_root.join("prune-me");
    fs::remove_dir_all(&prune_path).expect("manually delete prune-me worktree");

    let detached_path = repo.path().join("detached-raw");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        detached_path.to_str().unwrap(),
        "main",
    ])
    .assert_success();

    repo.run_stax(&["checkout", "main"]).assert_success();
    repo.git(&["merge", "--no-ff", "merged-lane", "-m", "Merge merged-lane"])
        .assert_success();

    let out = repo.run_stax_with_env(&["wt", "cleanup", "--dry-run"], &[("HOME", home.as_str())]);
    out.assert_success()
        .assert_stdout_contains("Would prune 1 stale entry:")
        .assert_stdout_contains("Found 2 cleanup candidates:")
        .assert_stdout_contains("Dry run only. No changes made.");

    assert!(
        merged_path.exists(),
        "dry-run should not remove merged managed worktree"
    );
    assert!(
        !prune_path.exists(),
        "fixture should keep the stale path absent on disk"
    );
    assert!(
        detached_path.exists(),
        "dry-run should not remove detached worktree"
    );

    let ll = repo.run_stax_with_env(&["wt", "ll"], &[("HOME", home.as_str())]);
    ll.assert_success();
    let stdout = TestRepo::stdout(&ll);
    assert!(
        stdout.contains("merged-lane"),
        "dry-run should leave merged-lane worktree registered"
    );
    assert!(
        stdout.contains("detached-raw"),
        "dry-run should leave detached worktree registered"
    );
    assert!(
        stdout.contains("prune-me"),
        "dry-run should leave stale bookkeeping registered"
    );
}

#[test]
fn wt_cleanup_skips_dirty_and_current_candidates_without_force() {
    let repo = TestRepo::new();

    let dirty_path = repo.path().join("dirty-detached");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        dirty_path.to_str().unwrap(),
        "main",
    ])
    .assert_success();
    fs::write(dirty_path.join("scratch.txt"), "dirty\n").expect("write dirty scratch file");

    let current_path = repo.path().join("current-detached");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        current_path.to_str().unwrap(),
        "main",
    ])
    .assert_success();

    let out = repo.run_stax_in(&current_path, &["wt", "cleanup", "--yes"]);
    out.assert_success()
        .assert_stdout_contains("Skipping 2 unsafe candidates:")
        .assert_stdout_contains("dirty")
        .assert_stdout_contains("current");

    assert!(
        dirty_path.exists(),
        "dirty detached worktree should be preserved without --force"
    );
    assert!(
        current_path.exists(),
        "current detached worktree should never be removed"
    );
}

#[test]
fn wt_cleanup_force_removes_dirty_detached_candidates() {
    let repo = TestRepo::new();

    let dirty_path = repo.path().join("force-detached");
    repo.git(&[
        "worktree",
        "add",
        "--detach",
        dirty_path.to_str().unwrap(),
        "main",
    ])
    .assert_success();
    fs::write(dirty_path.join("scratch.txt"), "dirty\n").expect("write dirty scratch file");

    let out = repo.run_stax(&["wt", "cleanup", "--force", "--yes"]);
    out.assert_success()
        .assert_stdout_contains("Removed  worktree 'force-detached'");

    assert!(
        !dirty_path.exists(),
        "expected --force cleanup to remove dirty detached worktree"
    );
}

#[test]
fn wt_remove_without_name_removes_current_worktree() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "remove-me"], &[("HOME", home.as_str())])
        .assert_success();
    let worktree_path = default_worktree_root(&repo, &home).join("remove-me");

    let out = repo.run_stax_in_with_env(&worktree_path, &["wt", "rm"], &[("HOME", home.as_str())]);
    out.assert_success();
    assert!(
        !worktree_path.exists(),
        "expected current worktree directory to be removed"
    );
}

#[test]
fn wt_remove_delete_branch_from_current_worktree_removes_branch_too() {
    let repo = TestRepo::new();
    let home = clean_home(&repo);

    repo.run_stax_with_env(&["wt", "c", "remove-branch"], &[("HOME", home.as_str())])
        .assert_success();
    let worktree_path = default_worktree_root(&repo, &home).join("remove-branch");

    let out = repo.run_stax_in_with_env(
        &worktree_path,
        &["wt", "rm", "--delete-branch"],
        &[("HOME", home.as_str())],
    );
    out.assert_success()
        .assert_stdout_contains("Deleted branch 'remove-branch'")
        .assert_stdout_contains("Removed  worktree 'remove-branch'");

    assert!(
        !worktree_path.exists(),
        "expected current worktree directory to be removed"
    );

    let branch_ref = "refs/heads/remove-branch";
    let show_ref = repo.git(&["show-ref", "--verify", "--quiet", branch_ref]);
    assert!(
        !show_ref.status.success(),
        "expected branch '{}' to be deleted",
        branch_ref
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
