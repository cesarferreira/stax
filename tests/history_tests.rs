use crate::common;

use common::{OutputAssertions, TestRepo};

#[test]
fn history_prints_one_line_per_commit_with_date_refs_and_committer() {
    let repo = TestRepo::new();
    repo.create_file("a.txt", "a");
    repo.commit("Add a");
    repo.git(&["tag", "v1.0.0"]).assert_success();

    let output = repo.run_stax(&["history"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "one line per commit: {stdout}");

    let short = repo.git(&["rev-parse", "--short", "HEAD"]);
    let short = TestRepo::stdout(&short).trim().to_string();
    let date = repo.git(&["log", "-1", "--format=%cs", "HEAD"]);
    let date = TestRepo::stdout(&date).trim().to_string();
    let committer = repo.git(&["log", "-1", "--format=%cn", "HEAD"]);
    let committer = TestRepo::stdout(&committer).trim().to_string();

    assert_eq!(
        lines[0],
        format!("{short} {date} (HEAD -> main, tag: v1.0.0) Add a [{committer}]")
    );
    assert!(
        !lines[1].contains('('),
        "undecorated commits have no ref list: {}",
        lines[1]
    );
    assert!(!stdout.contains('\x1b'), "no color when piped: {stdout:?}");
}

#[test]
fn history_ignores_user_log_config() {
    let repo = TestRepo::new();
    for (key, value) in [
        ("format.pretty", "fuller"),
        ("log.date", "iso"),
        ("log.decorate", "no"),
        ("log.showSignature", "true"),
        ("color.ui", "always"),
    ] {
        repo.git(&["config", key, value]).assert_success();
    }

    let output = repo.run_stax(&["hist"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("(HEAD -> main)"), "{stdout}");
    assert!(!stdout.contains("Author:"), "{stdout}");
    assert!(!stdout.contains('\x1b'), "{stdout:?}");
}

#[test]
fn history_passes_extra_args_to_git_log() {
    let repo = TestRepo::new();
    repo.create_file("a.txt", "a");
    repo.commit("Add a");
    repo.create_file("b.txt", "b");
    repo.commit("Add b");

    let output = repo.run_stax(&["history", "-n", "1"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("Add b"), "{stdout}");

    let output = repo.run_stax(&["history", "--", "a.txt"]);
    output.assert_success();
    let stdout = TestRepo::stdout(&output);
    assert_eq!(stdout.lines().count(), 1, "{stdout}");
    assert!(stdout.contains("Add a"), "{stdout}");
}

#[test]
fn history_fails_on_unknown_revision() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["history", "no-such-rev"]);
    output.assert_failure();
}
