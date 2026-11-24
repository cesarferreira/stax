//! Integration tests for `stax status --remote` / `stax ls --remote`.

use crate::common::{OutputAssertions, TestRepo};
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

fn run_git_in(cwd: &Path, args: &[&str]) -> String {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_CONFIG_GLOBAL", null)
        .env("GIT_CONFIG_SYSTEM", null)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Run `f` inside a fresh clone of the repo's remote, standing in for "the other machine".
fn with_other_clone<T>(repo: &TestRepo, f: impl FnOnce(&Path) -> T) -> T {
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = TempDir::new().expect("temp clone");
    run_git_in(
        clone_dir.path(),
        &["clone", remote_path.to_str().unwrap(), "."],
    );
    run_git_in(
        clone_dir.path(),
        &["config", "user.email", "other@test.com"],
    );
    run_git_in(clone_dir.path(), &["config", "user.name", "Other User"]);
    f(clone_dir.path())
}

/// Add a commit to `branch` on the remote from the other machine; returns the new SHA.
fn advance_remote(repo: &TestRepo, branch: &str, file: &str) -> String {
    with_other_clone(repo, |dir| {
        run_git_in(
            dir,
            &["checkout", "-B", branch, &format!("origin/{}", branch)],
        );
        std::fs::write(dir.join(file), "other machine").expect("write");
        run_git_in(dir, &["add", "-A"]);
        run_git_in(dir, &["commit", "-m", "Update from other machine"]);
        run_git_in(dir, &["push", "origin", branch]);
        run_git_in(dir, &["rev-parse", "HEAD"])
    })
}

/// Create a tracked branch with one commit and push it.
fn pushed_branch(repo: &TestRepo, name: &str) -> String {
    repo.run_stax(&["bc", name]);
    let branch = repo.current_branch();
    repo.create_file(&format!("{}.txt", name), name);
    repo.commit(&format!("commit {}", name));
    repo.git(&["push", "-u", "origin", &branch]);
    branch
}

fn line_for<'a>(stdout: &'a str, branch: &str) -> &'a str {
    stdout
        .lines()
        .find(|l| l.contains(branch))
        .unwrap_or_else(|| panic!("no line for {} in:\n{}", branch, stdout))
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for n in chars.by_ref() {
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn plain_stdout(out: &std::process::Output) -> String {
    strip_ansi(&TestRepo::stdout(out))
}

#[test]
fn status_remote_reports_in_sync_unpushed_and_no_remote() {
    let repo = TestRepo::new_with_remote();
    let a = pushed_branch(&repo, "feature-a");
    repo.git(&["checkout", "main"]);
    let b = pushed_branch(&repo, "feature-b");
    repo.create_file("more.txt", "more");
    repo.commit("local only");
    repo.git(&["checkout", "main"]);
    repo.run_stax(&["bc", "feature-c"]);
    let c = repo.current_branch();

    let out = repo.run_stax(&["ls", "--remote"]);
    out.assert_success();
    let stdout = plain_stdout(&out);
    assert!(
        line_for(&stdout, &a).contains("[origin: in sync]"),
        "{stdout}"
    );
    assert!(
        line_for(&stdout, &b).contains("[origin: 1↑ unpushed]"),
        "{stdout}"
    );
    assert!(
        line_for(&stdout, &c).contains("[origin: no remote]"),
        "{stdout}"
    );
    assert!(line_for(&stdout, "main").contains("in sync"), "{stdout}");
    assert!(!stdout.contains("st rs --get"), "{stdout}");
}

#[test]
fn status_remote_fetches_and_reports_behind_from_other_machine() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    let local_before = repo.get_commit_sha(&branch);
    let remote_sha = advance_remote(&repo, &branch, "other.txt");

    let out = repo.run_stax(&["status", "--remote"]);
    out.assert_success();
    let stdout = plain_stdout(&out);
    assert!(
        line_for(&stdout, &branch).contains("[origin: 1↓ behind]"),
        "{stdout}"
    );
    assert!(stdout.contains("st rs --get --all-stacks"), "{stdout}");
    assert_eq!(
        repo.get_commit_sha(&format!("origin/{}", branch)),
        remote_sha
    );
    assert_eq!(repo.get_commit_sha(&branch), local_before);
}

#[test]
fn status_remote_reports_diverged_and_is_read_only() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    with_other_clone(&repo, |dir| {
        run_git_in(
            dir,
            &["checkout", "-B", &branch, &format!("origin/{}", branch)],
        );
        run_git_in(dir, &["commit", "--amend", "-m", "x"]);
        run_git_in(dir, &["push", "--force", "origin", &branch]);
    });
    repo.create_file("local.txt", "local");
    repo.commit("new local commit");

    let snapshot = |repo: &TestRepo| {
        (
            String::from_utf8_lossy(
                &repo
                    .git(&["for-each-ref", "refs/heads", "refs/branch-metadata"])
                    .stdout,
            )
            .to_string(),
            String::from_utf8_lossy(&repo.git(&["symbolic-ref", "HEAD"]).stdout).to_string(),
            String::from_utf8_lossy(&repo.git(&["status", "--porcelain"]).stdout).to_string(),
        )
    };
    let before = snapshot(&repo);

    let out = repo.run_stax(&["status", "--remote"]);
    out.assert_success();
    let stdout = plain_stdout(&out);

    let counts = String::from_utf8_lossy(
        &repo
            .git(&[
                "rev-list",
                "--left-right",
                "--count",
                &format!("origin/{}...{}", branch, branch),
            ])
            .stdout,
    )
    .trim()
    .to_string();
    let mut parts = counts.split_whitespace();
    let behind = parts.next().unwrap();
    let ahead = parts.next().unwrap();
    assert!(
        line_for(&stdout, &branch).contains(&format!("[origin: diverged {ahead}↑/{behind}↓]")),
        "{stdout}"
    );
    assert_eq!(before, snapshot(&repo));
}

#[test]
fn status_remote_json_adds_parity_fields() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    advance_remote(&repo, &branch, "other.txt");

    let out = repo.run_stax(&["status", "--json", "--remote"]);
    out.assert_success();
    let json: serde_json::Value = serde_json::from_str(&TestRepo::stdout(&out)).expect("json");
    assert_eq!(json["remote"]["name"], "origin");
    assert_eq!(json["remote"]["fetched"], true);
    let branches = json["branches"].as_array().unwrap();
    let feature = branches
        .iter()
        .find(|b| b["name"] == branch.as_str())
        .unwrap();
    assert_eq!(feature["remote_parity"]["state"], "behind");
    assert_eq!(feature["remote_parity"]["behind"], 1);
    assert_eq!(feature["remote_parity"]["ahead"], 0);
    assert_eq!(
        feature["remote_parity"]["remote_ref"],
        format!("origin/{}", branch)
    );
    assert!(feature.get("ahead").is_some() && feature.get("behind").is_some());
    let trunk = branches.iter().find(|b| b["is_trunk"] == true).unwrap();
    assert_eq!(trunk["remote_parity"]["state"], "in_sync");
}

#[test]
fn status_remote_json_has_remote_true_when_fetch_creates_tracking_ref() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    repo.git(&[
        "update-ref",
        "-d",
        &format!("refs/remotes/origin/{}", branch),
    ]);
    let remote_sha = advance_remote(&repo, &branch, "other.txt");

    let out = repo.run_stax(&["status", "--json", "--remote"]);
    out.assert_success();
    let json: serde_json::Value = serde_json::from_str(&TestRepo::stdout(&out)).expect("json");
    let feature = json["branches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == branch.as_str())
        .unwrap();
    assert_eq!(feature["has_remote"], true, "{json}");
    assert_eq!(feature["remote_parity"]["state"], "behind", "{json}");
    assert_eq!(
        repo.get_commit_sha(&format!("origin/{}", branch)),
        remote_sha
    );
}

#[test]
fn status_remote_offline_warns_and_uses_cached_refs() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    let gone = TempDir::new().unwrap();
    let bad = gone.path().join("does-not-exist");
    repo.git(&["remote", "set-url", "origin", bad.to_str().unwrap()]);

    let out = repo.run_stax(&["status", "--remote"]);
    out.assert_success();
    let stderr = TestRepo::stderr(&out);
    assert!(stderr.contains("could not fetch from 'origin'"), "{stderr}");
    assert!(stderr.contains("cached"), "{stderr}");
    let stdout = plain_stdout(&out);
    assert!(
        line_for(&stdout, &branch).contains("[origin: in sync]"),
        "{stdout}"
    );
}

#[test]
fn status_remote_without_configured_remote_errors_cleanly() {
    let repo = TestRepo::new();
    let out = repo.run_stax(&["status", "--remote"]);
    assert!(!out.status.success());
    assert!(
        TestRepo::stderr(&out).contains("--remote needs a git remote named 'origin'"),
        "{}",
        TestRepo::stderr(&out)
    );
}

#[test]
fn status_remote_deleted_remote_branch_shows_no_remote() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    with_other_clone(&repo, |dir| {
        run_git_in(dir, &["push", "origin", "--delete", &branch]);
    });

    let out = repo.run_stax(&["status", "--remote"]);
    out.assert_success();
    let stdout = plain_stdout(&out);
    assert!(
        line_for(&stdout, &branch).contains("[origin: no remote]"),
        "{stdout}"
    );
    assert!(!TestRepo::stderr(&out).contains("could not fetch"));
    assert!(repo.list_branches().contains(&branch));
}

#[test]
fn status_remote_no_fetch_uses_cached_refs() {
    let repo = TestRepo::new_with_remote();
    let branch = pushed_branch(&repo, "feature");
    let cached = repo.get_commit_sha(&format!("origin/{}", branch));
    advance_remote(&repo, &branch, "other.txt");

    let out = repo.run_stax(&["status", "--remote", "--no-fetch"]);
    out.assert_success();
    let stdout = plain_stdout(&out);
    assert!(
        line_for(&stdout, &branch).contains("[origin: in sync]"),
        "{stdout}"
    );
    assert_eq!(repo.get_commit_sha(&format!("origin/{}", branch)), cached);

    let out = repo.run_stax(&["status", "--json", "--remote", "--no-fetch"]);
    out.assert_success();
    let json: serde_json::Value = serde_json::from_str(&TestRepo::stdout(&out)).expect("json");
    assert_eq!(json["remote"]["fetched"], false);
}

#[test]
fn status_remote_current_scope_fetches_only_shown_branches() {
    let repo = TestRepo::new_with_remote();
    let a = pushed_branch(&repo, "stack-a");
    repo.git(&["checkout", "main"]);
    let b = pushed_branch(&repo, "stack-b");
    let b_cached = repo.get_commit_sha(&format!("origin/{}", b));
    advance_remote(&repo, &a, "a.txt");
    advance_remote(&repo, &b, "b.txt");
    repo.git(&["checkout", &a]);

    let out = repo.run_stax(&["status", "--remote", "--current"]);
    out.assert_success();
    let stdout = plain_stdout(&out);
    assert!(!stdout.contains(&b), "{stdout}");
    assert!(line_for(&stdout, &a).contains("1↓ behind"), "{stdout}");
    assert!(stdout.contains("st rs --get"), "{stdout}");
    assert!(!stdout.contains("--all-stacks"), "{stdout}");
    assert_eq!(repo.get_commit_sha(&format!("origin/{}", b)), b_cached);
}

#[test]
fn status_json_without_remote_is_unchanged() {
    let repo = TestRepo::new_with_remote();
    pushed_branch(&repo, "feature");

    let out = repo.run_stax(&["status", "--json"]);
    out.assert_success();
    let json: serde_json::Value = serde_json::from_str(&TestRepo::stdout(&out)).expect("json");
    assert!(json.get("remote").is_none());
    for b in json["branches"].as_array().unwrap() {
        assert!(b.get("remote_parity").is_none());
        assert_eq!(b.as_object().unwrap().len(), 17);
    }
    let out = repo.run_stax(&["status"]);
    assert!(!plain_stdout(&out).contains("[origin:"));
}
