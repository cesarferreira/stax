//! Integration tests for `stax branch track --all-prs` command

mod common;
use common::{OutputAssertions, TestRepo};

/// Test that --all-prs flag is recognized by the CLI
#[test]
fn test_track_all_prs_flag_recognized() {
    let repo = TestRepo::new_with_remote();

    // Running --all-prs should not fail with "unrecognized flag"
    let output = repo.run_stax(&["branch", "track", "--all-prs"]);
    let stderr = TestRepo::stderr(&output);

    // The command may fail due to missing GitHub token, but the flag should be recognized
    assert!(
        !stderr.contains("unexpected argument") && !stderr.contains("unrecognized"),
        "Flag --all-prs should be recognized, got: {}",
        stderr
    );
}

/// Test that --all-prs and --parent flags conflict
#[test]
fn test_track_all_prs_conflicts_with_parent() {
    let repo = TestRepo::new_with_remote();

    let output = repo.run_stax(&["branch", "track", "--all-prs", "--parent", "main"]);

    // Should fail because --all-prs conflicts with --parent
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflict"),
        "Expected conflict error, got: {}",
        stderr
    );
}

/// Test that --all-prs fails gracefully without GitHub token or proper remote
#[test]
fn test_track_all_prs_no_token() {
    let repo = TestRepo::new_with_remote();

    // Ensure no token is set
    std::env::remove_var("GITHUB_TOKEN");
    std::env::remove_var("STAX_GITHUB_TOKEN");

    let output = repo.run_stax(&["branch", "track", "--all-prs"]);

    // Should fail - the command requires GitHub integration
    // It may fail due to:
    // 1. Missing token (if remote URL is valid GitHub URL)
    // 2. Invalid remote URL format (test uses local path as remote)
    output.assert_failure();

    let stderr = TestRepo::stderr(&output);
    let stdout = TestRepo::stdout(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should have some error, either about token, auth, or remote URL
    assert!(
        combined.contains("token")
            || combined.contains("auth")
            || combined.contains("Token")
            || combined.contains("remote")
            || combined.contains("URL"),
        "Expected error about GitHub integration, got stdout: {}\nstderr: {}",
        stdout,
        stderr
    );
}

/// Test help text includes --all-prs
#[test]
fn test_track_help_includes_all_prs() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "track", "--help"]);
    output.assert_success();

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--all-prs"),
        "Help should mention --all-prs flag, got: {}",
        stdout
    );
    assert!(
        stdout.contains("open PRs"),
        "Help should describe what --all-prs does, got: {}",
        stdout
    );
}

/// Test that existing track command still works without --all-prs
#[test]
fn test_track_single_branch_still_works() {
    let repo = TestRepo::new();

    // Create an untracked branch with git directly
    repo.git(&["checkout", "-b", "untracked-feature"]);
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Track it with stax
    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    output.assert_success();

    // Verify it's now tracked
    let json = repo.get_status_json();
    let branches = json["branches"].as_array().expect("Expected branches array");
    let tracked = branches
        .iter()
        .any(|b| b["name"].as_str() == Some("untracked-feature"));
    assert!(tracked, "Branch should be tracked after running track command");

    // Verify parent is correct
    let feature = branches
        .iter()
        .find(|b| b["name"].as_str() == Some("untracked-feature"))
        .expect("Branch not found");
    assert_eq!(
        feature["parent"].as_str(),
        Some("main"),
        "Parent should be main"
    );
}

/// Test that already tracked branches are handled correctly
#[test]
fn test_track_already_tracked_branch() {
    let repo = TestRepo::new();

    // Create a tracked branch with stax
    repo.create_stack(&["tracked-feature"]);

    // Go back to that branch
    repo.run_stax(&["checkout", &repo.find_branch_containing("tracked-feature").unwrap()]);

    // Try to track it again (should fail gracefully)
    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("already tracked") || stdout.contains("reparent"),
        "Should indicate branch is already tracked, got: {}",
        stdout
    );
}
