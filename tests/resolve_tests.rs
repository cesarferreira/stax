use crate::common;

use common::{OutputAssertions, TestRepo};
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn install_fake_codex(repo: &TestRepo, script: &str) -> (PathBuf, String) {
    let bin_dir = repo.path().join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("Failed to create fake-bin directory");

    let codex_path = bin_dir.join("codex");
    fs::write(&codex_path, script).expect("Failed to write fake codex script");

    #[cfg(unix)]
    {
        let mut perms = fs::metadata(&codex_path)
            .expect("Failed to stat fake codex script")
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&codex_path, perms).expect("Failed to chmod fake codex script");
    }

    let old_path = std::env::var("PATH").unwrap_or_default();
    let path_env = format!("{}:{}", bin_dir.display(), old_path);
    (bin_dir, path_env)
}

fn setup_single_conflict(repo: &TestRepo) {
    repo.create_conflict_scenario();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(
        repo.has_rebase_in_progress(),
        "Expected an in-progress rebase conflict"
    );
}

fn setup_two_round_conflict(repo: &TestRepo) {
    repo.run_stax(&["bc", "multi-conflict"]).assert_success();
    let branch = repo.current_branch();

    repo.create_file("a.txt", "feature a\n");
    repo.commit("Feature adds a");
    repo.create_file("b.txt", "feature b\n");
    repo.commit("Feature adds b");

    repo.run_stax(&["t"]).assert_success();
    repo.create_file("a.txt", "main a\n");
    repo.create_file("b.txt", "main b\n");
    repo.commit("Main adds both files");

    repo.run_stax(&["checkout", &branch]).assert_success();
    let _ = repo.run_stax(&["restack", "--yes", "--quiet"]);
    assert!(
        repo.has_rebase_in_progress(),
        "Expected an in-progress rebase conflict"
    );
}

#[test]
fn test_resolve_no_rebase_in_progress() {
    let repo = TestRepo::new();
    repo.create_stack(&["feature-1"]);

    let output = repo.run_stax(&["resolve"]);
    output.assert_success();
    output.assert_stdout_contains("No rebase in progress.");
}

#[test]
fn test_resolve_success_single_round_with_fake_codex() {
    let repo = TestRepo::new();
    setup_single_conflict(&repo);

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
cat >/dev/null
printf '%s' '{"resolutions":[{"path":"conflict.txt","content":"resolved content\nline 2\nline 3\n"}]}'
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "3"],
        &[("PATH", path_env.as_str())],
    );
    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "Rebase should be completed after successful resolve"
    );
}

#[test]
fn test_resolve_invalid_json_fails_and_keeps_rebase_in_progress() {
    let repo = TestRepo::new();
    setup_single_conflict(&repo);

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
cat >/dev/null
printf '%s' 'not-json'
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "3"],
        &[("PATH", path_env.as_str())],
    );
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("not valid JSON"),
        "Expected invalid JSON error, got: {}",
        combined
    );
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should still be in progress after JSON parse failure"
    );
}

#[test]
fn test_resolve_rejects_non_conflicted_file_paths() {
    let repo = TestRepo::new();
    setup_single_conflict(&repo);

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
cat >/dev/null
printf '%s' '{"resolutions":[{"path":"not-conflicted.txt","content":"anything"}]}'
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "3"],
        &[("PATH", path_env.as_str())],
    );
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("non-conflicted file"),
        "Expected guardrail error, got: {}",
        combined
    );
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should still be in progress after guardrail failure"
    );
}

#[test]
fn test_resolve_rejects_unrelated_side_effect_file_changes() {
    let repo = TestRepo::new();
    setup_single_conflict(&repo);

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
cat >/dev/null
echo 'side effect' > unexpected-side-effect.txt
printf '%s' '{"resolutions":[{"path":"conflict.txt","content":"resolved content\nline 2\nline 3\n"}]}'
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "3"],
        &[("PATH", path_env.as_str())],
    );
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("Detected edits outside conflicted files"),
        "Expected side-effect guardrail error, got: {}",
        combined
    );
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should still be in progress after side-effect guardrail failure"
    );
}

#[test]
fn test_resolve_retries_invalid_response_then_succeeds() {
    let repo = TestRepo::new();
    setup_single_conflict(&repo);

    // First invocation emits invalid JSON; the marker flips the second
    // invocation to a valid response, exercising the repair loop. The marker
    // lives outside the repo working tree so the conflicted-only side-effect
    // guard doesn't flag it.
    let marker = std::env::temp_dir().join("stax-resolve-retry.marker");
    let _ = fs::remove_file(&marker);
    let marker_env = marker.display().to_string();

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
cat >/dev/null
if [ -f "$STAX_RETRY_MARKER" ]; then
  printf '%s' '{"resolutions":[{"path":"conflict.txt","content":"resolved content\nline 2\nline 3\n"}]}'
else
  : > "$STAX_RETRY_MARKER"
  printf '%s' 'not-json'
fi
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "3"],
        &[
            ("PATH", path_env.as_str()),
            ("STAX_RETRY_MARKER", marker_env.as_str()),
        ],
    );
    let _ = fs::remove_file(&marker);
    output.assert_success();
    assert!(
        !repo.has_rebase_in_progress(),
        "Rebase should complete after the agent repairs its response"
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("asking the agent to fix it"),
        "Expected a repair-attempt notice, got: {}",
        stdout
    );
}

#[test]
fn test_resolve_max_rounds_exhaustion() {
    let repo = TestRepo::new();
    setup_two_round_conflict(&repo);

    let (_bin_dir, path_env) = install_fake_codex(
        &repo,
        r#"#!/bin/sh
INPUT="$(cat)"
FILE="$(printf '%s\n' "$INPUT" | sed -n 's/^FILE: //p' | head -n 1)"
if [ -z "$FILE" ]; then
  FILE="a.txt"
fi
printf '{"resolutions":[{"path":"%s","content":"resolved by fake agent\\n"}]}' "$FILE"
"#,
    );

    let output = repo.run_stax_with_env(
        &["resolve", "--agent", "codex", "--max-rounds", "1"],
        &[("PATH", path_env.as_str())],
    );
    output.assert_failure();

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("Reached max rounds (1)"),
        "Expected max-rounds error, got: {}",
        combined
    );
    assert!(
        repo.has_rebase_in_progress(),
        "Rebase should still be in progress after max rounds exhaustion"
    );
}
