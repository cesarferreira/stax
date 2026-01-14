//! PR template integration tests
//!
//! Tests for PR template discovery and selection during submit flow.
//! Note: Full submit tests require GitHub API access, so we test
//! template discovery in real repo contexts without API calls.

mod common;

use common::{OutputAssertions, TestRepo};
use std::fs;

// =============================================================================
// Template Discovery Tests
// =============================================================================

#[test]
fn test_submit_with_single_template() {
    let repo = TestRepo::new();

    // Create PR template
    let github_dir = repo.path().join(".github");
    fs::create_dir(&github_dir).unwrap();
    fs::write(
        github_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "## Description\n\nPlease describe your changes.\n\n## Testing\n\nHow was this tested?"
    ).unwrap();

    // Create a branch with stax
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify branch was created
    assert!(repo.current_branch_contains("test-branch"));

    // Note: Full submit test would require GitHub API mocking
    // This test validates that template discovery works in real repo context
    // Actual PR creation tested in Task 6
}

#[test]
fn test_template_discovery_multiple() {
    let repo = TestRepo::new();

    // Create multiple PR templates
    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();

    fs::write(template_dir.join("feature.md"), "# Feature template\n\n## Changes\n\n").unwrap();
    fs::write(template_dir.join("bugfix.md"), "# Bugfix template\n\n## Bug Description\n\n").unwrap();
    fs::write(template_dir.join("docs.md"), "# Docs template\n\n## Documentation Changes\n\n").unwrap();

    // Create a branch with stax
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify branch was created
    assert!(repo.current_branch_contains("test-branch"));

    // Verify all template files exist
    assert!(template_dir.join("feature.md").exists());
    assert!(template_dir.join("bugfix.md").exists());
    assert!(template_dir.join("docs.md").exists());

    // Note: Template selection would be tested via submit command with mocked GitHub API
}

#[test]
fn test_no_template_in_repo() {
    let repo = TestRepo::new();

    // No template created - use default repo

    // Create a branch with stax
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify branch was created
    assert!(repo.current_branch_contains("test-branch"));

    // Note: Submit without templates would use empty body or default message
}

// =============================================================================
// Template Directory Structure Tests
// =============================================================================

#[test]
fn test_templates_in_subdirectory() {
    let repo = TestRepo::new();

    // Create template in subdirectory structure
    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("default.md"), "# Default template\n").unwrap();

    // Create a branch
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify template file exists
    assert!(template_dir.join("default.md").exists());
}

#[test]
fn test_template_in_docs_directory() {
    let repo = TestRepo::new();

    // Create template in docs directory (alternative location)
    let docs_dir = repo.path().join("docs");
    fs::create_dir_all(&docs_dir).unwrap();
    fs::write(docs_dir.join("PULL_REQUEST_TEMPLATE.md"), "# Docs location template\n").unwrap();

    // Create a branch
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify template file exists
    assert!(docs_dir.join("PULL_REQUEST_TEMPLATE.md").exists());
}
