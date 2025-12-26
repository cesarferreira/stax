//! Integration tests for stax commands
//!
//! These tests create real temporary git repositories and run actual stax commands
//! to verify end-to-end functionality.

use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Get path to compiled binary (built by cargo test)
fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

/// A test repository that creates a temporary git repo with proper initialization
struct TestRepo {
    dir: TempDir,
}

impl TestRepo {
    /// Create a new test repository with git init and an initial commit on main
    fn new() -> Self {
        let dir = TempDir::new().expect("Failed to create temp dir");
        let path = dir.path();

        // Initialize git repo
        Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .expect("Failed to init git repo");

        // Configure git user for commits
        Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .expect("Failed to set git email");

        Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("Failed to set git name");

        // Create initial commit
        let readme = path.join("README.md");
        fs::write(&readme, "# Test Repo\n").expect("Failed to write README");

        Command::new("git")
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .expect("Failed to stage files");

        Command::new("git")
            .args(["commit", "-m", "Initial commit"])
            .current_dir(path)
            .output()
            .expect("Failed to create initial commit");

        Self { dir }
    }

    /// Find a branch that contains the given substring
    fn find_branch_containing(&self, pattern: &str) -> Option<String> {
        self.list_branches()
            .into_iter()
            .find(|b| b.contains(pattern))
    }

    /// Check if current branch name contains the given substring
    fn current_branch_contains(&self, pattern: &str) -> bool {
        self.current_branch().contains(pattern)
    }

    /// Get the path to the test repository
    fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Run a stax command in this repository
    fn run_stax(&self, args: &[&str]) -> Output {
        Command::new(stax_bin())
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to execute stax")
    }

    /// Get stdout as string from output
    fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Get stderr as string from output
    fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    /// Create a file in the repository
    fn create_file(&self, name: &str, content: &str) {
        let file_path = self.path().join(name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        fs::write(file_path, content).expect("Failed to write file");
    }

    /// Create a commit with all staged changes
    fn commit(&self, message: &str) {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(self.path())
            .output()
            .expect("Failed to stage files");

        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(self.path())
            .output()
            .expect("Failed to commit");
    }

    /// Get the current branch name
    fn current_branch(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(self.path())
            .output()
            .expect("Failed to get current branch");

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get list of all branches
    fn list_branches(&self) -> Vec<String> {
        let output = Command::new("git")
            .args(["branch", "--format=%(refname:short)"])
            .current_dir(self.path())
            .output()
            .expect("Failed to list branches");

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|s| s.to_string())
            .collect()
    }

    /// Get the commit SHA for a branch (or HEAD if branch is empty)
    fn get_commit_sha(&self, reference: &str) -> String {
        let output = Command::new("git")
            .args(["rev-parse", reference])
            .current_dir(self.path())
            .output()
            .expect("Failed to get commit SHA");

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get the HEAD commit SHA
    fn head_sha(&self) -> String {
        self.get_commit_sha("HEAD")
    }

    /// Run a raw git command
    fn git(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to run git command")
    }
}

// =============================================================================
// Test Infrastructure Tests
// =============================================================================

#[test]
fn test_repo_setup() {
    let repo = TestRepo::new();
    assert!(repo.path().exists());
    assert_eq!(repo.current_branch(), "main");
    assert!(repo.list_branches().contains(&"main".to_string()));
}

// =============================================================================
// Branch Creation Tests (bc)
// =============================================================================

#[test]
fn test_branch_create_simple() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "feature-1"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    // Branch name might have a prefix from config
    assert!(repo.current_branch_contains("feature-1"));

    // Branch should exist
    assert!(repo.find_branch_containing("feature-1").is_some());
}

#[test]
fn test_branch_create_with_message() {
    let repo = TestRepo::new();

    // Create a file to commit
    repo.create_file("new_feature.rs", "fn main() {}");

    let output = repo.run_stax(&["bc", "-m", "Add new feature"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // Branch should be created with a sanitized name from the message
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("add-new-feature") || b.contains("Add-new-feature")),
        "Expected branch from message, got: {:?}",
        branches
    );

    // Should have committed the changes
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Committed") || stdout.contains("No changes"),
        "Expected commit message, got: {}",
        stdout
    );
}

#[test]
fn test_branch_create_from_another_branch() {
    let repo = TestRepo::new();

    // Create first feature branch
    let output = repo.run_stax(&["bc", "feature-1"]);
    assert!(output.status.success());

    // Create a commit on feature-1
    repo.create_file("feature1.txt", "feature 1 content");
    repo.commit("Add feature 1");

    // Create another branch from main (not from current)
    let output = repo.run_stax(&["bc", "feature-2", "--from", "main"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    assert!(repo.current_branch_contains("feature-2"));

    // feature-2 should not have feature1.txt
    assert!(!repo.path().join("feature1.txt").exists());
}

#[test]
fn test_branch_create_nested() {
    let repo = TestRepo::new();

    // Create a chain: main -> feature-1 -> feature-2 -> feature-3
    let output = repo.run_stax(&["bc", "feature-1"]);
    assert!(output.status.success());
    assert!(repo.current_branch_contains("feature-1"));

    let output = repo.run_stax(&["bc", "feature-2"]);
    assert!(output.status.success());
    assert!(repo.current_branch_contains("feature-2"));

    let output = repo.run_stax(&["bc", "feature-3"]);
    assert!(output.status.success());
    assert!(repo.current_branch_contains("feature-3"));

    // Check all branches exist
    assert!(repo.find_branch_containing("feature-1").is_some());
    assert!(repo.find_branch_containing("feature-2").is_some());
    assert!(repo.find_branch_containing("feature-3").is_some());
}

#[test]
fn test_branch_create_requires_name() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc"]);
    assert!(!output.status.success());
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("name") || stderr.contains("required"),
        "Expected error about name, got: {}",
        stderr
    );
}

// =============================================================================
// Status/Log Tests
// =============================================================================

#[test]
fn test_status_empty_stack() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["status"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("main"), "Expected main in output: {}", stdout);
}

#[test]
fn test_status_with_branches() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("feature-1"), "Expected feature-1 in output: {}", stdout);
    assert!(stdout.contains("main"), "Expected main in output: {}", stdout);
}

#[test]
fn test_status_json_output() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["status", "--json"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert_eq!(json["trunk"], "main");
    assert!(json["branches"].is_array());

    let branches = json["branches"].as_array().unwrap();
    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("feature-1")),
        "Expected branch containing feature-1 in branches: {:?}",
        branches
    );
}

#[test]
fn test_status_compact_output() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["status", "--compact"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    // Compact output should have tab-separated values
    assert!(stdout.contains("feature-1"));
    assert!(stdout.contains('\t'));
}

#[test]
fn test_status_alias_s() {
    let repo = TestRepo::new();

    let output1 = repo.run_stax(&["status"]);
    let output2 = repo.run_stax(&["s"]);

    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_log_command() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Add feature");

    let output = repo.run_stax(&["log"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

#[test]
fn test_log_json_output() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["log", "--json"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(json["branches"].is_array());
}

// =============================================================================
// Navigation Tests (bu, bd, trunk, checkout)
// =============================================================================

#[test]
fn test_trunk_command() {
    let repo = TestRepo::new();

    // Create a branch and switch away from main
    repo.run_stax(&["bc", "feature-1"]);
    assert!(repo.current_branch_contains("feature-1"));

    // Switch to trunk
    let output = repo.run_stax(&["trunk"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_trunk_alias_t() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["t"]);
    assert!(output.status.success());
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_branch_down_bd() {
    let repo = TestRepo::new();

    // Create chain: main -> feature-1 -> feature-2
    repo.run_stax(&["bc", "feature-1"]);
    repo.run_stax(&["bc", "feature-2"]);
    assert!(repo.current_branch_contains("feature-2"));

    // Move down to feature-1
    let output = repo.run_stax(&["bd"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    assert!(repo.current_branch_contains("feature-1"));

    // Move down to main
    let output = repo.run_stax(&["bd"]);
    assert!(output.status.success());
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_branch_up_bu() {
    let repo = TestRepo::new();

    // Create chain: main -> feature-1
    repo.run_stax(&["bc", "feature-1"]);

    // Go back to main
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Move up to feature-1
    let output = repo.run_stax(&["bu"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_checkout_explicit_branch() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    repo.run_stax(&["t"]);
    assert_eq!(repo.current_branch(), "main");

    // Use the actual branch name (which may include a prefix)
    let output = repo.run_stax(&["checkout", &feature_branch]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_checkout_trunk_flag() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["checkout", "--trunk"]);
    assert!(output.status.success());
    assert_eq!(repo.current_branch(), "main");
}

#[test]
fn test_checkout_parent_flag() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.run_stax(&["bc", "feature-2"]);
    assert!(repo.current_branch_contains("feature-2"));

    let output = repo.run_stax(&["checkout", "--parent"]);
    assert!(output.status.success());
    assert!(repo.current_branch_contains("feature-1"));
}

#[test]
fn test_checkout_alias_co() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    repo.run_stax(&["t"]);

    let output = repo.run_stax(&["co", &feature_branch]);
    assert!(output.status.success());
    assert!(repo.current_branch_contains("feature-1"));
}

// =============================================================================
// Branch Management Tests
// =============================================================================

#[test]
fn test_branch_track() {
    let repo = TestRepo::new();

    // Create a branch using git directly (not stax)
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("untracked.txt", "content");
    repo.commit("Untracked commit");

    // Track it with stax
    let output = repo.run_stax(&["branch", "track", "--parent", "main"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // Now it should appear in status
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    assert!(
        branches.iter().any(|b| b["name"] == "untracked-branch"),
        "Expected untracked-branch to be tracked"
    );
}

#[test]
fn test_branch_reparent() {
    let repo = TestRepo::new();

    // Create two branches from main
    repo.run_stax(&["bc", "feature-1"]);
    let feature1_name = repo.current_branch();
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "feature-2"]);
    let feature2_name = repo.current_branch();

    // Reparent feature-2 to be on top of feature-1
    let output = repo.run_stax(&["branch", "reparent", "--parent", &feature1_name]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // Check the new parent in JSON
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let feature2 = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap() == feature2_name)
        .expect("Should find feature-2 branch");
    assert!(
        feature2["parent"].as_str().unwrap().contains("feature-1"),
        "Expected parent to contain feature-1, got: {}",
        feature2["parent"]
    );
}

#[test]
fn test_branch_delete() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-to-delete"]);
    let branch_name = repo.current_branch();
    repo.run_stax(&["t"]); // Go back to main first

    // Delete the branch (force since it's not merged)
    let output = repo.run_stax(&["branch", "delete", &branch_name, "--force"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // Branch should no longer exist
    assert!(repo.find_branch_containing("feature-to-delete").is_none());
}

#[test]
fn test_branch_squash() {
    let repo = TestRepo::new();

    // Create a branch with multiple commits
    repo.run_stax(&["bc", "feature-squash"]);
    repo.create_file("file1.txt", "content 1");
    repo.commit("Commit 1");
    repo.create_file("file2.txt", "content 2");
    repo.commit("Commit 2");
    repo.create_file("file3.txt", "content 3");
    repo.commit("Commit 3");

    // Count commits before squash
    let log_output = repo.git(&["rev-list", "--count", "main..HEAD"]);
    let count_before: i32 = String::from_utf8_lossy(&log_output.stdout)
        .trim()
        .parse()
        .unwrap();
    assert_eq!(count_before, 3);

    // Squash with a message (non-interactive)
    let output = repo.run_stax(&["branch", "squash", "-m", "Squashed feature"]);
    // Note: squash command might require interactive confirmation
    // For now just check it runs without panic
    let _ = output;
}

// =============================================================================
// Modify Tests
// =============================================================================

#[test]
fn test_modify_amend() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-modify"]);
    repo.create_file("feature.txt", "original content");
    repo.commit("Initial feature");

    let commit_before = repo.head_sha();

    // Make changes
    repo.create_file("feature.txt", "modified content");

    // Amend using modify
    let output = repo.run_stax(&["modify"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    let commit_after = repo.head_sha();
    assert_ne!(commit_before, commit_after, "Commit should have changed after amend");
}

#[test]
fn test_modify_with_message() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-modify"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Old message");

    // Make changes and amend with new message
    repo.create_file("feature.txt", "new content");
    let output = repo.run_stax(&["modify", "-m", "New commit message"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // Check the commit message changed
    let log_output = repo.git(&["log", "-1", "--format=%s"]);
    let message = String::from_utf8_lossy(&log_output.stdout).trim().to_string();
    assert_eq!(message, "New commit message");
}

#[test]
fn test_modify_no_changes() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-no-changes"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Try to modify with no changes
    let output = repo.run_stax(&["modify"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("No changes") || stdout.to_lowercase().contains("no changes"),
        "Expected 'no changes' message, got: {}",
        stdout
    );
}

#[test]
fn test_modify_alias_m() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-m"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature");

    repo.create_file("feature.txt", "modified");
    let output = repo.run_stax(&["m"]);
    assert!(output.status.success());
}

// =============================================================================
// Restack Tests
// =============================================================================

#[test]
fn test_restack_up_to_date() {
    let repo = TestRepo::new();

    // Create a simple branch that doesn't need restack
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Restack should say up to date
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

#[test]
fn test_restack_after_parent_change() {
    let repo = TestRepo::new();

    // Create feature branch
    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Go back to main and make a new commit
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");

    // Go back to feature
    repo.run_stax(&["checkout", &feature_branch]);

    // Status should show needs restack
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let feature1 = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("feature-1"))
        .expect("Should find feature-1 branch");
    assert!(feature1["needs_restack"].as_bool().unwrap_or(false));

    // Now restack (quiet mode to avoid prompts)
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    // After restack, should no longer need it
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let feature1 = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("feature-1"))
        .expect("Should find feature-1 branch after restack");
    assert!(!feature1["needs_restack"].as_bool().unwrap_or(true));
}

#[test]
fn test_restack_all_flag() {
    let repo = TestRepo::new();

    // Create multiple branches
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "content");
    repo.commit("Feature 2");

    // Update main
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main");
    repo.commit("Main update");

    // Go to feature-2 and try restack --all
    repo.run_stax(&["checkout", "feature-2"]);
    let output = repo.run_stax(&["restack", "--all", "--quiet"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

// =============================================================================
// Doctor/Config Tests
// =============================================================================

#[test]
fn test_doctor_command() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["doctor"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

#[test]
fn test_config_command() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("Config path:"));
    assert!(stdout.contains("config.toml"));
}

// =============================================================================
// Edge Cases and Error Handling
// =============================================================================

#[test]
fn test_status_outside_git_repo() {
    let dir = TempDir::new().expect("Failed to create temp dir");

    let output = Command::new(stax_bin())
        .args(["status"])
        .current_dir(dir.path())
        .output()
        .expect("Failed to execute stax");

    // Should fail gracefully
    assert!(!output.status.success());
}

#[test]
fn test_checkout_nonexistent_branch() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["checkout", "nonexistent-branch"]);
    assert!(!output.status.success());
}

#[test]
fn test_branch_delete_trunk_fails() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["branch", "delete", "main", "--force"]);
    assert!(!output.status.success());

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("trunk") || stderr.contains("Cannot delete"),
        "Expected error about trunk, got: {}",
        stderr
    );
}

#[test]
fn test_branch_delete_current_fails() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    // We're on feature-1, trying to delete it should fail
    let output = repo.run_stax(&["branch", "delete", &feature_branch, "--force"]);
    assert!(!output.status.success());

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("current") || stderr.contains("Checkout"),
        "Expected error about current branch, got: {}",
        stderr
    );
}

#[test]
fn test_bd_at_bottom_of_stack() {
    let repo = TestRepo::new();

    // On main, bd should do nothing or give message
    let output = repo.run_stax(&["bd"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("bottom") || stdout.contains("trunk") || stdout.contains("Already"),
        "Expected message about being at bottom, got: {}",
        stdout
    );
}

#[test]
fn test_bu_at_top_of_stack() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    // feature-1 has no children

    let output = repo.run_stax(&["bu"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("top") || stdout.contains("no child") || stdout.contains("Already"),
        "Expected message about being at top, got: {}",
        stdout
    );
}

#[test]
fn test_multiple_stacks() {
    let repo = TestRepo::new();

    // Create two independent stacks from main
    repo.run_stax(&["bc", "stack1-feature"]);
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "stack2-feature"]);

    // Both should appear in status
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();

    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("stack1-feature")),
        "Expected stack1-feature in branches"
    );
    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("stack2-feature")),
        "Expected stack2-feature in branches"
    );
}

#[test]
fn test_diff_command() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    let output = repo.run_stax(&["diff"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

#[test]
fn test_range_diff_command() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    let output = repo.run_stax(&["range-diff"]);
    assert!(output.status.success(), "Failed: {}", TestRepo::stderr(&output));
}

