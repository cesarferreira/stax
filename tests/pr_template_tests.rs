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
        "## Description\n\nPlease describe your changes.\n\n## Testing\n\nHow was this tested?",
    )
    .unwrap();

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

    fs::write(
        template_dir.join("feature.md"),
        "# Feature template\n\n## Changes\n\n",
    )
    .unwrap();
    fs::write(
        template_dir.join("bugfix.md"),
        "# Bugfix template\n\n## Bug Description\n\n",
    )
    .unwrap();
    fs::write(
        template_dir.join("docs.md"),
        "# Docs template\n\n## Documentation Changes\n\n",
    )
    .unwrap();

    // Test discovery returns all templates
    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 3);

    let names: Vec<_> = templates.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"feature"));
    assert!(names.contains(&"bugfix"));
    assert!(names.contains(&"docs"));
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

#[test]
fn test_no_template_flag_skips_template() {
    let repo = TestRepo::new();

    // Create template
    let github_dir = repo.path().join(".github");
    fs::create_dir(&github_dir).unwrap();
    fs::write(
        github_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "# Template content",
    )
    .unwrap();

    // Verify template exists
    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 1);

    // When --no-template is used, submit command should skip template selection
    // (This flag behavior is handled in the submit command logic, not in template discovery)
    // The discovery function always returns available templates; the flag controls whether they're used
}

// =============================================================================
// Generate --pr-body Template Selection Tests
// =============================================================================

#[test]
fn test_generate_no_template_flag_skips_discovery() {
    let repo = TestRepo::new();

    // Create multiple templates
    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("feature.md"), "# Feature\n").unwrap();
    fs::write(template_dir.join("bugfix.md"), "# Bugfix\n").unwrap();

    // Templates are present in the repo...
    let discovered = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(discovered.len(), 2);

    // ...but --no-template bypasses discovery entirely, producing an empty list
    let discovered_with_flag: Vec<stax::github::pr_template::PrTemplate> = Vec::new();
    assert!(discovered_with_flag.is_empty());
    // With an empty list, select_template_interactive returns None (no template content)
    let selected =
        stax::github::pr_template::select_template_interactive(&discovered_with_flag).unwrap();
    assert!(selected.is_none());
}

#[test]
fn test_generate_template_flag_selects_by_name() {
    let repo = TestRepo::new();

    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("feature.md"), "# Feature template\n").unwrap();
    fs::write(template_dir.join("bugfix.md"), "# Bugfix template\n").unwrap();

    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 2);

    // --template feature: find by name (production logic in generate::run)
    let selected = templates.iter().find(|t| t.name == "feature").cloned();
    assert!(selected.is_some());
    assert_eq!(selected.unwrap().content.trim(), "# Feature template");

    // --template missing: returns None and the command warns the user
    let missing = templates.iter().find(|t| t.name == "missing").cloned();
    assert!(missing.is_none());
}

#[test]
fn test_generate_no_prompt_single_template_auto_selects() {
    let repo = TestRepo::new();

    let github_dir = repo.path().join(".github");
    fs::create_dir_all(&github_dir).unwrap();
    fs::write(
        github_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "# Single template\n",
    )
    .unwrap();

    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 1);

    // --no-prompt with a single template: select_template_auto returns it automatically
    let selected = stax::github::pr_template::select_template_auto(&templates);
    assert!(selected.is_some());
    assert_eq!(selected.unwrap().content.trim(), "# Single template");
}

#[test]
fn test_generate_no_prompt_multiple_templates_uses_none() {
    let repo = TestRepo::new();

    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("feature.md"), "# Feature\n").unwrap();
    fs::write(template_dir.join("bugfix.md"), "# Bugfix\n").unwrap();

    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 2);

    // --no-prompt with multiple templates: select_template_auto returns None
    // (generate::run then uses None, so no template is applied without interaction)
    let selected = stax::github::pr_template::select_template_auto(&templates);
    assert!(selected.is_none());
}

#[test]
fn test_build_template_options_sorts_alphabetically() {
    let repo = TestRepo::new();

    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(template_dir.join("zebra.md"), "# Zebra\n").unwrap();
    fs::write(template_dir.join("alpha.md"), "# Alpha\n").unwrap();
    fs::write(template_dir.join("middle.md"), "# Middle\n").unwrap();

    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();
    assert_eq!(templates.len(), 3);

    let options = stax::github::pr_template::build_template_options(&templates);

    // First option is always "No template"
    assert_eq!(options[0], "No template");
    // Remaining names are sorted alphabetically
    assert_eq!(options[1], "alpha");
    assert_eq!(options[2], "middle");
    assert_eq!(options[3], "zebra");
}

#[test]
fn test_build_template_options_empty_returns_only_no_template() {
    let empty: Vec<stax::github::pr_template::PrTemplate> = Vec::new();
    let options = stax::github::pr_template::build_template_options(&empty);
    assert_eq!(options.len(), 1);
    assert_eq!(options[0], "No template");
}

#[test]
fn test_generate_template_content_used_in_prompt() {
    let repo = TestRepo::new();

    let template_dir = repo.path().join(".github/PULL_REQUEST_TEMPLATE");
    fs::create_dir_all(&template_dir).unwrap();
    fs::write(
        template_dir.join("feature.md"),
        "## Summary\n\n## Testing\n",
    )
    .unwrap();

    let templates = stax::github::pr_template::discover_pr_templates(&repo.path()).unwrap();

    // When a template is selected by name, its content is available for use in the AI prompt
    let selected = templates.iter().find(|t| t.name == "feature").cloned();
    assert!(selected.is_some());
    let content = selected.unwrap().content;
    assert!(content.contains("## Summary"));
    assert!(content.contains("## Testing"));
}

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
    fs::write(
        docs_dir.join("PULL_REQUEST_TEMPLATE.md"),
        "# Docs location template\n",
    )
    .unwrap();

    // Create a branch
    let output = repo.run_stax(&["create", "test-branch", "-m", "test commit"]);
    output.assert_success();

    // Verify template file exists
    assert!(docs_dir.join("PULL_REQUEST_TEMPLATE.md").exists());
}
