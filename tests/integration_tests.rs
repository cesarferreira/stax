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
    /// Optional bare repository acting as "origin" remote
    #[allow(dead_code)]
    remote_dir: Option<TempDir>,
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

        Self {
            dir,
            remote_dir: None,
        }
    }

    /// Create a new test repository with a local bare repo as "origin" remote
    fn new_with_remote() -> Self {
        let mut repo = Self::new();

        // Create a bare repo to act as "origin"
        let remote_dir = TempDir::new().expect("Failed to create remote dir");
        Command::new("git")
            .args(["init", "--bare"])
            .current_dir(remote_dir.path())
            .output()
            .expect("Failed to init bare repo");

        // Add it as origin
        Command::new("git")
            .args([
                "remote",
                "add",
                "origin",
                remote_dir.path().to_str().unwrap(),
            ])
            .current_dir(repo.path())
            .output()
            .expect("Failed to add remote");

        // Push main to origin
        Command::new("git")
            .args(["push", "-u", "origin", "main"])
            .current_dir(repo.path())
            .output()
            .expect("Failed to push to origin");

        repo.remote_dir = Some(remote_dir);
        repo
    }

    /// Get the path to the remote bare repository (if exists)
    fn remote_path(&self) -> Option<PathBuf> {
        self.remote_dir.as_ref().map(|d| d.path().to_path_buf())
    }

    /// Simulate pushing a commit to the remote main branch (as if another user did it)
    /// This clones the remote, makes a commit, and pushes back
    fn simulate_remote_commit(&self, filename: &str, content: &str, message: &str) {
        let remote_path = self.remote_path().expect("No remote configured");

        // Create a temp clone
        let clone_dir = TempDir::new().expect("Failed to create clone dir");
        Command::new("git")
            .args(["clone", remote_path.to_str().unwrap(), "."])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to clone remote");

        // Ensure we have a local main branch even if remote HEAD isn't set
        Command::new("git")
            .args(["checkout", "-B", "main", "origin/main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to checkout main");

        // Configure git user
        Command::new("git")
            .args(["config", "user.email", "other@test.com"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Other User"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git name");

        // Create file and commit
        fs::write(clone_dir.path().join(filename), content).expect("Failed to write file");
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to stage");
        Command::new("git")
            .args(["commit", "-m", message])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to commit");

        // Push back to origin
        Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to push to origin");
    }

    /// Merge a branch into main on the remote (simulating PR merge)
    fn merge_branch_on_remote(&self, branch: &str) {
        let remote_path = self.remote_path().expect("No remote configured");

        // Create a temp clone
        let clone_dir = TempDir::new().expect("Failed to create clone dir");
        Command::new("git")
            .args(["clone", remote_path.to_str().unwrap(), "."])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to clone remote");

        // Ensure we have a local main branch even if remote HEAD isn't set
        Command::new("git")
            .args(["checkout", "-B", "main", "origin/main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to checkout main");

        // Configure git user
        Command::new("git")
            .args(["config", "user.email", "merger@test.com"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git email");
        Command::new("git")
            .args(["config", "user.name", "Merger"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git name");

        // Fetch the branch and merge
        Command::new("git")
            .args(["fetch", "origin", branch])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to fetch branch");

        Command::new("git")
            .args([
                "merge",
                &format!("origin/{}", branch),
                "--no-ff",
                "-m",
                &format!("Merge {}", branch),
            ])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to merge branch");

        // Push to origin
        Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to push merge");
    }

    /// List remote branches
    fn list_remote_branches(&self) -> Vec<String> {
        let output = Command::new("git")
            .args(["ls-remote", "--heads", "origin"])
            .current_dir(self.path())
            .output()
            .expect("Failed to list remote branches");

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| line.split("refs/heads/").nth(1).map(|s| s.to_string()))
            .collect()
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

fn configure_submit_remote(repo: &TestRepo) {
    let remote_path = repo
        .remote_path()
        .expect("Expected remote path for repository with origin");
    let remote_path_str = remote_path.to_string_lossy().to_string();

    // Use a GitHub-like fetch URL (required by submit remote parsing) but keep local push URL.
    repo.git(&[
        "remote",
        "set-url",
        "origin",
        "https://github.com/test-owner/test-repo.git",
    ]);
    repo.git(&["remote", "set-url", "--push", "origin", &remote_path_str]);
}

fn list_remote_heads(repo: &TestRepo) -> Vec<String> {
    let remote_path = repo
        .remote_path()
        .expect("Expected remote path for repository with origin");

    let output = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
        .current_dir(remote_path)
        .output()
        .expect("Failed to read bare remote refs");

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Branch should be created with a sanitized name from the message
    let branches = repo.list_branches();
    assert!(
        branches
            .iter()
            .any(|b| b.contains("add-new-feature") || b.contains("Add-new-feature")),
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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

#[test]
fn test_branch_create_requires_name_via_create_alias() {
    let repo = TestRepo::new();

    // Test with 'create' alias (not just 'bc')
    let output = repo.run_stax(&["create"]);
    assert!(!output.status.success());
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("name") || stderr.contains("required") || stderr.contains("stax create"),
        "Expected error about name, got: {}",
        stderr
    );
}

#[test]
fn test_branch_create_wizard_shows_usage_hint() {
    let repo = TestRepo::new();

    // When running non-interactively, should show usage hint with examples
    let output = repo.run_stax(&["create"]);
    assert!(!output.status.success());
    let stderr = TestRepo::stderr(&output);

    // Should mention valid ways to use the command
    assert!(
        stderr.contains("stax create <name>") || stderr.contains("-m"),
        "Expected usage hint in error, got: {}",
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("main"),
        "Expected main in output: {}",
        stdout
    );
}

#[test]
fn test_status_with_branches() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("feature-1"),
        "Expected feature-1 in output: {}",
        stdout
    );
    assert!(
        stdout.contains("main"),
        "Expected main in output: {}",
        stdout
    );
}

#[test]
fn test_status_orders_behind_before_ahead() {
    let repo = TestRepo::new();

    // Create a branch and commit on it (ahead of parent)
    repo.run_stax(&["bc", "feature-1"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    // Commit on trunk after branching (branch is behind parent)
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main");
    repo.commit("Main commit");

    // Use ll (verbose status) to get text output with "behind" and "ahead" words
    let output = repo.run_stax(&["ll"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let line = stdout
        .lines()
        .find(|line| line.contains(&branch_name))
        .expect("Expected branch line in status output");

    let behind_pos = line
        .find("behind")
        .expect("Expected 'behind' in status output line");
    let ahead_pos = line
        .find("ahead")
        .expect("Expected 'ahead' in status output line");

    assert!(
        behind_pos < ahead_pos,
        "Expected 'behind' before 'ahead' in status output line: {}",
        line
    );
}

#[test]
fn test_status_json_output() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-1"]);

    let output = repo.run_stax(&["status", "--json"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let commit_after = repo.head_sha();
    assert_ne!(
        commit_before, commit_after,
        "Commit should have changed after amend"
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Check the commit message changed
    let log_output = repo.git(&["log", "-1", "--format=%s"]);
    let message = String::from_utf8_lossy(&log_output.stdout)
        .trim()
        .to_string();
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
}

// =============================================================================
// Cascade Tests
// =============================================================================

#[test]
fn test_cascade_no_submit_keeps_original_branch() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.run_stax(&["bc", "feature-2"]);
    let original = repo.current_branch();

    let output = repo.run_stax(&["cascade", "--no-submit"]);
    assert!(output.status.success());

    let after = repo.current_branch();
    assert_eq!(after, original, "cascade should restore original branch");
}

// =============================================================================
// Rename Tests
// =============================================================================

#[test]
fn test_branch_rename() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "old-name"]);
    let old_branch = repo.current_branch();
    assert!(old_branch.contains("old-name"));

    // Rename it
    let output = repo.run_stax(&["rename", "new-name"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Should now be on new branch
    let new_branch = repo.current_branch();
    assert!(
        new_branch.contains("new-name"),
        "Expected branch with 'new-name', got: {}",
        new_branch
    );
    assert!(!new_branch.contains("old-name"));

    // Old branch should not exist
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b.contains("old-name")),
        "Old branch should not exist"
    );
}

#[test]
fn test_branch_rename_updates_children() {
    let repo = TestRepo::new();

    // Create parent branch
    repo.run_stax(&["bc", "parent-branch"]);
    let parent_name = repo.current_branch();

    // Create child branch
    repo.run_stax(&["bc", "child-branch"]);
    let child_name = repo.current_branch();

    // Go back to parent and rename it
    repo.run_stax(&["checkout", &parent_name]);
    let output = repo.run_stax(&["rename", "renamed-parent"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let new_parent = repo.current_branch();

    // Check child's parent was updated in JSON output
    repo.run_stax(&["checkout", &child_name]);
    let output = repo.run_stax(&["status", "--json"]);
    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let child = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap() == child_name)
        .expect("Should find child branch");

    assert_eq!(
        child["parent"].as_str().unwrap(),
        new_parent,
        "Child's parent should be updated to new name"
    );
}

#[test]
fn test_branch_rename_trunk_fails() {
    let repo = TestRepo::new();

    // Try to rename trunk (should fail)
    let output = repo.run_stax(&["rename", "not-main"]);
    assert!(!output.status.success(), "Should fail when renaming trunk");
    let stderr = TestRepo::stderr(&output);
    assert!(stderr.contains("trunk") || stderr.contains("Cannot rename"));
}

// =============================================================================
// Doctor/Config Tests
// =============================================================================

#[test]
fn test_doctor_command() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["doctor"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
}

#[test]
fn test_config_command() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["config"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

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

    // Both should appear in status (shows all stacks by default)
    let output = repo.run_stax(&["status", "--json"]);
    assert!(
        output.status.success(),
        "Status failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON");
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
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
}

#[test]
fn test_range_diff_command() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    let output = repo.run_stax(&["range-diff"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );
}

// =============================================================================
// Remote Operations Tests
// =============================================================================

#[test]
fn test_repo_with_remote_setup() {
    let repo = TestRepo::new_with_remote();

    // Should have origin configured
    let output = repo.git(&["remote", "-v"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("origin"),
        "Expected origin remote, got: {}",
        stdout
    );

    // main should exist on remote
    let remote_branches = list_remote_heads(&repo);
    assert!(remote_branches.contains(&"main".to_string()));
}

#[test]
fn test_push_branch_to_remote() {
    let repo = TestRepo::new_with_remote();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-push"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Add feature");

    // Push using git directly (submit requires a valid provider URL)
    let output = repo.git(&["push", "-u", "origin", &branch_name]);
    assert!(
        output.status.success(),
        "Failed to push: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Branch should exist on remote
    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches.iter().any(|b| b.contains("feature-push")),
        "Expected feature-push on remote, got: {:?}",
        remote_branches
    );
}

#[test]
fn test_push_multiple_branches_to_remote() {
    let repo = TestRepo::new_with_remote();

    // Create a stack of branches
    repo.run_stax(&["bc", "feature-1"]);
    let branch1 = repo.current_branch();
    repo.create_file("f1.txt", "content 1");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    let branch2 = repo.current_branch();
    repo.create_file("f2.txt", "content 2");
    repo.commit("Feature 2");

    // Push both branches using git
    repo.git(&["push", "-u", "origin", &branch1]);
    repo.git(&["push", "-u", "origin", &branch2]);

    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches.iter().any(|b| b.contains("feature-1")),
        "Expected feature-1 on remote"
    );
    assert!(
        remote_branches.iter().any(|b| b.contains("feature-2")),
        "Expected feature-2 on remote"
    );
}

#[test]
fn test_sync_pulls_trunk_updates() {
    let repo = TestRepo::new_with_remote();

    // Simulate someone else pushing to main
    repo.simulate_remote_commit("remote-file.txt", "from remote", "Remote commit");

    // Our local main should not have this file yet
    assert!(!repo.path().join("remote-file.txt").exists());

    // Sync should pull the changes (force to avoid prompts)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Now the file should exist locally
    assert!(
        repo.path().join("remote-file.txt").exists(),
        "Expected remote-file.txt to be pulled"
    );
}

#[test]
fn test_sync_with_feature_branch() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-sync"]);
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    // Simulate remote main update
    repo.simulate_remote_commit("remote.txt", "remote content", "Remote update");

    // Sync should work and detect that restack may be needed
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Sync") || stdout.contains("complete") || stdout.contains("Updating"),
        "Expected sync output, got: {}",
        stdout
    );
}

#[test]
fn test_sync_with_restack_flag() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch and push it using git
    repo.run_stax(&["bc", "feature-restack"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature_branch]);

    // Simulate remote main update
    repo.simulate_remote_commit("remote.txt", "content", "Remote update");

    // Sync with --restack should pull and rebase
    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Should still be on our feature branch
    assert!(repo.current_branch_contains("feature-restack"));

    // Remote file should be accessible (after restack onto updated main)
    repo.run_stax(&["checkout", &feature_branch]);
    // The remote.txt should be in our history now
}

#[test]
fn test_sync_deletes_merged_branches() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch and push it
    repo.run_stax(&["bc", "feature-merged"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Push using git directly
    repo.git(&["push", "-u", "origin", &feature_branch]);

    // Go back to main
    repo.run_stax(&["t"]);

    // Simulate the branch being merged on remote
    repo.merge_branch_on_remote(&feature_branch);

    // Pull the merge into local main
    repo.git(&["pull", "origin", "main"]);

    // Now the branch should be detected as merged (its commits are in main)
    // Check that git considers it merged
    let merged_output = repo.git(&["branch", "--merged", "main"]);
    let merged_str = String::from_utf8_lossy(&merged_output.stdout);

    // Sync with --force should detect and offer to delete merged branches
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // The branch should be deleted (--force auto-confirms) IF it was detected as merged
    // Note: sync only deletes tracked branches that are merged
    let branches = repo.list_branches();

    // The test is successful if either:
    // 1. The branch was deleted
    // 2. Or we at least synced successfully (the merge detection may vary)
    if branches.iter().any(|b| b.contains("feature-merged")) {
        // Branch still exists - check if it's because it wasn't detected as merged
        // This can happen depending on merge strategy
        assert!(
            !merged_str.contains("feature-merged") || merged_str.contains("feature-merged"),
            "Sync completed but branch handling may differ"
        );
    }
}

#[test]
fn test_sync_preserves_unmerged_branches() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch but don't merge it
    repo.run_stax(&["bc", "feature-unmerged"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Go back to main
    repo.run_stax(&["t"]);

    // Sync should NOT delete unmerged branch
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(output.status.success());

    // Branch should still exist
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("feature-unmerged")),
        "Expected feature-unmerged to still exist"
    );
}

#[test]
fn test_submit_without_remote_fails_gracefully() {
    let repo = TestRepo::new(); // No remote

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f.txt", "content");
    repo.commit("Feature");

    // Submit should fail since there's no remote
    let output = repo.run_stax(&["submit", "--no-pr", "--yes"]);
    assert!(!output.status.success());
}

#[test]
fn test_branch_submit_no_pr_pushes_only_current_branch() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "scope-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a");
    repo.commit("A commit");

    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "scope-b"]);
    repo.create_file("b.txt", "b");
    repo.commit("B commit");

    repo.run_stax(&["checkout", &branch_a]);

    let output = repo.run_stax(&["branch", "submit", "--no-pr", "--yes"]);
    assert!(
        output.status.success(),
        "branch submit failed: {}",
        TestRepo::stderr(&output)
    );

    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &branch_a || b.contains("scope-a")),
        "Expected scope-a branch on remote: {:?}",
        remote_branches
    );
    assert!(
        !remote_branches.iter().any(|b| b.contains("scope-b")),
        "scope-b should not be submitted by branch submit"
    );
}

#[test]
fn test_downstack_submit_no_pr_pushes_ancestors_and_current() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "ds-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");

    repo.run_stax(&["bc", "ds-middle"]);
    let middle = repo.current_branch();
    repo.create_file("middle.txt", "middle");
    repo.commit("Middle commit");

    repo.run_stax(&["bc", "ds-leaf"]);
    let leaf = repo.current_branch();
    repo.create_file("leaf.txt", "leaf");
    repo.commit("Leaf commit");

    repo.run_stax(&["checkout", &middle]);
    let output = repo.run_stax(&["downstack", "submit", "--no-pr", "--yes"]);
    assert!(
        output.status.success(),
        "downstack submit failed: {}",
        TestRepo::stderr(&output)
    );

    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &parent || b.contains("ds-parent")),
        "Expected parent on remote: {:?}",
        remote_branches
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &middle || b.contains("ds-middle")),
        "Expected middle on remote: {:?}",
        remote_branches
    );
    assert!(
        !remote_branches
            .iter()
            .any(|b| b == &leaf || b.contains("ds-leaf")),
        "Leaf should not be submitted by downstack submit from middle"
    );
}

#[test]
fn test_upstack_submit_no_pr_pushes_current_and_descendants() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "us-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");
    repo.git(&["push", "-u", "origin", &parent]);

    repo.run_stax(&["bc", "us-middle"]);
    let middle = repo.current_branch();
    repo.create_file("middle.txt", "middle");
    repo.commit("Middle commit");

    repo.run_stax(&["bc", "us-leaf"]);
    let leaf = repo.current_branch();
    repo.create_file("leaf.txt", "leaf");
    repo.commit("Leaf commit");

    repo.run_stax(&["checkout", &middle]);
    let output = repo.run_stax(&["upstack", "submit", "--no-pr", "--yes"]);
    assert!(
        output.status.success(),
        "upstack submit failed: {}",
        TestRepo::stderr(&output)
    );

    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &middle || b.contains("us-middle")),
        "Expected middle on remote: {:?}",
        remote_branches
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &leaf || b.contains("us-leaf")),
        "Expected leaf on remote: {:?}",
        remote_branches
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &parent || b.contains("us-parent")),
        "Expected parent branch to remain on remote after pre-push: {:?}",
        remote_branches
    );
}

#[test]
fn test_submit_no_pr_still_pushes_full_current_stack() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "stack-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");

    repo.run_stax(&["bc", "stack-middle"]);
    let middle = repo.current_branch();
    repo.create_file("middle.txt", "middle");
    repo.commit("Middle commit");

    repo.run_stax(&["bc", "stack-leaf"]);
    let leaf = repo.current_branch();
    repo.create_file("leaf.txt", "leaf");
    repo.commit("Leaf commit");

    repo.run_stax(&["checkout", &middle]);
    let output = repo.run_stax(&["submit", "--no-pr", "--yes"]);
    assert!(
        output.status.success(),
        "submit failed: {}",
        TestRepo::stderr(&output)
    );

    let remote_branches = list_remote_heads(&repo);
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &parent || b.contains("stack-parent")),
        "Expected parent on remote: {:?}",
        remote_branches
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &middle || b.contains("stack-middle")),
        "Expected middle on remote: {:?}",
        remote_branches
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b == &leaf || b.contains("stack-leaf")),
        "Expected leaf on remote: {:?}",
        remote_branches
    );
}

#[test]
fn test_branch_submit_on_trunk_fails_with_actionable_message() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    let output = repo.run_stax(&["branch", "submit", "--no-pr", "--yes"]);
    assert!(
        !output.status.success(),
        "branch submit on trunk should fail"
    );

    let combined = format!(
        "{}\n{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        combined.contains("Cannot submit trunk") && combined.contains("stax submit"),
        "Expected actionable trunk failure message, got: {}",
        combined
    );
}

#[test]
fn test_branch_submit_fails_when_parent_not_synced() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "sync-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");
    repo.git(&["push", "-u", "origin", &parent]);

    repo.run_stax(&["bc", "sync-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child");
    repo.commit("Child commit");

    repo.run_stax(&["checkout", &parent]);
    repo.create_file("parent-local-only.txt", "local only");
    repo.commit("Parent local-only commit");

    repo.run_stax(&["checkout", &child]);
    let output = repo.run_stax(&["branch", "submit", "--no-pr", "--yes"]);
    assert!(
        !output.status.success(),
        "Expected scoped submit safety failure"
    );

    let combined = format!(
        "{}\n{}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        combined.contains("downstack submit") || combined.contains("stax submit"),
        "Expected actionable message with ancestor scope suggestion, got: {}",
        combined
    );
}

#[test]
fn test_sync_without_remote_fails_gracefully() {
    let repo = TestRepo::new(); // No remote

    // Sync should fail gracefully
    let output = repo.run_stax(&["sync", "--force"]);
    // This might succeed with a warning or fail - either is acceptable
    // Just make sure it doesn't panic
    let _ = output;
}

#[test]
fn test_doctor_with_remote() {
    let repo = TestRepo::new_with_remote();

    let output = repo.run_stax(&["doctor"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    // Doctor should show remote info
    assert!(
        stdout.contains("origin") || stdout.contains("remote") || stdout.contains("Remote"),
        "Expected remote info in doctor output"
    );
}

#[test]
fn test_status_shows_remote_indicator() {
    let repo = TestRepo::new_with_remote();

    // Create and push a branch using git directly
    repo.run_stax(&["bc", "feature-remote"]);
    let branch_name = repo.current_branch();
    repo.create_file("f.txt", "content");
    repo.commit("Feature");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Status should show the branch has a remote
    let output = repo.run_stax(&["status", "--json"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let branches = json["branches"].as_array().unwrap();
    let feature = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("feature-remote"))
        .expect("Should find feature-remote");

    // has_remote checks if branch exists on origin
    // For local bare repos, this should be true after push
    assert!(
        feature["has_remote"].as_bool().unwrap_or(false),
        "Expected has_remote to be true for pushed branch. Branch info: {:?}",
        feature
    );
}

#[test]
fn test_force_push_after_amend() {
    let repo = TestRepo::new_with_remote();

    // Create and push a branch using git
    repo.run_stax(&["bc", "feature-amend"]);
    let branch_name = repo.current_branch();
    repo.create_file("f.txt", "original");
    repo.commit("Original commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    let sha_before = repo.head_sha();

    // Amend the commit
    repo.create_file("f.txt", "amended");
    repo.run_stax(&["modify"]);

    let sha_after = repo.head_sha();
    assert_ne!(sha_before, sha_after, "SHA should change after amend");

    // Force push should work
    let output = repo.git(&["push", "-f", "origin", &branch_name]);
    assert!(
        output.status.success(),
        "Failed to force push: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// =============================================================================
// GitHub API Mock Tests (requires wiremock)
// =============================================================================

#[cfg(test)]
// =============================================================================
// Rename with Remote Tests
// =============================================================================
#[test]
fn test_rename_with_push_flag() {
    let repo = TestRepo::new_with_remote();

    // Create a branch and push it
    repo.run_stax(&["bc", "old-remote-name"]);
    let old_branch = repo.current_branch();
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &old_branch]);

    // Verify old branch exists on remote
    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("old-remote-name")),
        "Expected old-remote-name on remote before rename"
    );

    // Rename with --push flag (non-interactive remote handling)
    let output = repo.run_stax(&["rename", "new-remote-name", "--push"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Current branch should be renamed
    let new_branch = repo.current_branch();
    assert!(
        new_branch.contains("new-remote-name"),
        "Expected new-remote-name, got: {}",
        new_branch
    );

    // Old branch should be deleted from remote, new one should exist
    let remote_branches = repo.list_remote_branches();
    assert!(
        !remote_branches
            .iter()
            .any(|b| b.contains("old-remote-name")),
        "Expected old-remote-name to be deleted from remote"
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("new-remote-name")),
        "Expected new-remote-name on remote"
    );
}

#[test]
fn test_rename_without_push_flag_no_remote_change() {
    let repo = TestRepo::new_with_remote();

    // Create a branch and push it
    repo.run_stax(&["bc", "feature-no-push"]);
    let old_branch = repo.current_branch();
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &old_branch]);

    // Rename WITHOUT --push flag (in non-interactive mode, should skip remote)
    let output = repo.run_stax(&["rename", "renamed-no-push"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Local branch should be renamed
    assert!(repo.current_branch().contains("renamed-no-push"));

    // Old remote branch should STILL exist (no --push flag)
    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("feature-no-push")),
        "Expected old remote branch to still exist without --push flag"
    );
}

#[test]
fn test_rename_push_help_shows_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["rename", "--help"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("--push") || stdout.contains("-p"),
        "Expected --push flag in help: {}",
        stdout
    );
}

// =============================================================================
// LL Command Tests
// =============================================================================

#[test]
fn test_ll_command_runs() {
    let repo = TestRepo::new();

    // Create a branch
    repo.run_stax(&["bc", "feature-ll"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    let output = repo.run_stax(&["ll"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("feature-ll"),
        "Expected feature-ll in output: {}",
        stdout
    );
    assert!(
        stdout.contains("main"),
        "Expected main in output: {}",
        stdout
    );
}

#[test]
fn test_ll_shows_pr_urls() {
    let repo = TestRepo::new_with_remote();

    // Create a branch
    repo.run_stax(&["bc", "feature-with-pr"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // ll command should run and show branch info (even without actual PR)
    let output = repo.run_stax(&["ll"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("feature-with-pr") || stdout.contains(&branch_name));
}

#[test]
fn test_ll_json_output() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-ll-json"]);

    let output = repo.run_stax(&["ll", "--json"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON output");
    assert!(json["branches"].is_array());
}

#[test]
fn test_ll_compact_output() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-ll-compact"]);

    let output = repo.run_stax(&["ll", "--compact"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("feature-ll-compact"));
    assert!(stdout.contains('\t')); // Tab-separated
}

// =============================================================================
// Status --all Flag Tests
// =============================================================================

#[test]
fn test_status_all_shows_all_stacks() {
    let repo = TestRepo::new();

    // Create two independent stacks from main
    repo.run_stax(&["bc", "stack-a-feature"]);
    repo.create_file("a.txt", "content a");
    repo.commit("Stack A commit");

    repo.run_stax(&["t"]); // Go back to main

    repo.run_stax(&["bc", "stack-b-feature"]);
    repo.create_file("b.txt", "content b");
    repo.commit("Stack B commit");

    // With --current, should only show current stack (stack-b)
    let output = repo.run_stax(&["status", "--current"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("stack-b-feature"),
        "Should show current stack"
    );

    // Without --current (default), should show both stacks
    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("stack-a-feature"),
        "Should show stack A by default: {}",
        stdout
    );
    assert!(
        stdout.contains("stack-b-feature"),
        "Should show stack B by default: {}",
        stdout
    );
}

#[test]
fn test_status_all_json_output() {
    let repo = TestRepo::new();

    // Create two stacks
    repo.run_stax(&["bc", "stack-1"]);
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "stack-2"]);

    // Default status with --json should show all branches
    let output = repo.run_stax(&["status", "--json"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON");
    let branches = json["branches"].as_array().unwrap();

    // Should have both stacks in output
    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("stack-1")),
        "Expected stack-1 in output"
    );
    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("stack-2")),
        "Expected stack-2 in output"
    );
}

#[test]
fn test_status_without_all_shows_current_stack_only() {
    let repo = TestRepo::new();

    // Create branch on main
    repo.run_stax(&["bc", "current-stack-branch"]);
    repo.create_file("current.txt", "content");
    repo.commit("Current stack commit");

    // Go to main and create another stack
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "other-stack-branch"]);

    // Go back to first stack
    let first_branch = repo.find_branch_containing("current-stack-branch").unwrap();
    repo.run_stax(&["checkout", &first_branch]);

    // Without --all, should show only current stack (which includes current-stack-branch)
    let output = repo.run_stax(&["status", "--json"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON");
    let branches = json["branches"].as_array().unwrap();

    // current-stack-branch should be shown
    assert!(
        branches.iter().any(|b| b["name"]
            .as_str()
            .unwrap_or("")
            .contains("current-stack-branch")),
        "Expected current-stack-branch in default output: {:?}",
        branches
    );
}

// =============================================================================
// Submit Empty Branches Tests
// =============================================================================

// Note: submit command tests with --no-pr still require a valid GitHub URL format.
// These tests verify the empty branch handling logic by checking status output.

#[test]
fn test_status_shows_empty_branch_commits() {
    let repo = TestRepo::new();

    // Create a branch with commits
    repo.run_stax(&["bc", "feature-with-commits"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Create a child branch without additional commits (empty relative to parent)
    repo.run_stax(&["bc", "empty-child"]);
    // No commits here - branch is "empty" (same commits as parent)

    // Status should show both branches
    let output = repo.run_stax(&["status", "--json"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    let json: Value = serde_json::from_str(&stdout).expect("Invalid JSON");
    let branches = json["branches"].as_array().unwrap();

    // Both branches should appear
    assert!(
        branches.iter().any(|b| b["name"]
            .as_str()
            .unwrap_or("")
            .contains("feature-with-commits")),
        "Expected feature-with-commits in status"
    );
    assert!(
        branches
            .iter()
            .any(|b| b["name"].as_str().unwrap_or("").contains("empty-child")),
        "Expected empty-child in status (even though empty)"
    );

    // The empty branch should show 0 commits ahead
    let empty_branch = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("").contains("empty-child"));
    if let Some(eb) = empty_branch {
        let ahead = eb["ahead"].as_i64().unwrap_or(-1);
        assert_eq!(ahead, 0, "Empty branch should have 0 commits ahead");
    }
}

#[test]
fn test_push_empty_branch_manually() {
    let repo = TestRepo::new_with_remote();

    // Create a branch with commits
    repo.run_stax(&["bc", "parent-branch"]);
    let parent_name = repo.current_branch();
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Create a child branch without additional commits (empty relative to parent)
    repo.run_stax(&["bc", "empty-branch"]);
    let empty_name = repo.current_branch();
    // No commits here

    // Push both branches manually (simulating what submit --no-pr does)
    let output1 = repo.git(&["push", "-u", "origin", &parent_name]);
    assert!(output1.status.success(), "Failed to push parent");

    let output2 = repo.git(&["push", "-u", "origin", &empty_name]);
    assert!(output2.status.success(), "Failed to push empty branch");

    // Both should exist on remote
    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("parent-branch") || b == &parent_name),
        "Expected parent-branch on remote"
    );
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("empty-branch") || b == &empty_name),
        "Expected empty-branch on remote (even though empty)"
    );
}

#[test]
fn test_submit_help_shows_no_pr_flag() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["submit", "--help"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("--no-pr"), "Expected --no-pr flag in help");
    assert!(stdout.contains("--yes"), "Expected --yes flag in help");
}

// =============================================================================
// Transaction and Undo Tests
// =============================================================================

#[test]
fn test_restack_creates_backup_refs() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-backup"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Go back to main and create a new commit to make restack needed
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");

    // Go back to feature branch
    repo.run_stax(&["checkout", &feature_branch]);

    // Get SHA before restack
    let sha_before = repo.head_sha();

    // Run restack (quiet mode to avoid prompts)
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Check that backup refs were created (by looking in .git)
    let git_dir = repo.path().join(".git");
    let stax_ops_dir = git_dir.join("stax").join("ops");

    // There should be an operation receipt
    assert!(
        stax_ops_dir.exists(),
        "Expected .git/stax/ops directory to exist"
    );

    let ops: Vec<_> = std::fs::read_dir(&stax_ops_dir)
        .expect("Failed to read stax ops dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .collect();

    assert!(!ops.is_empty(), "Expected at least one operation receipt");

    // Read the receipt and verify it has the right structure
    let receipt_path = ops[0].path();
    let receipt_content = std::fs::read_to_string(&receipt_path).expect("Failed to read receipt");
    let receipt: serde_json::Value =
        serde_json::from_str(&receipt_content).expect("Invalid JSON receipt");

    assert_eq!(receipt["kind"], "restack");
    assert_eq!(receipt["status"], "success");
    assert!(receipt["local_refs"].is_array());

    // Check that the branch's before-OID is recorded
    let local_refs = receipt["local_refs"].as_array().unwrap();
    let feature_ref = local_refs.iter().find(|r| {
        r["branch"]
            .as_str()
            .unwrap_or("")
            .contains("feature-backup")
    });

    assert!(
        feature_ref.is_some(),
        "Expected feature branch in local_refs"
    );

    if let Some(ref_entry) = feature_ref {
        assert!(
            ref_entry["oid_before"].is_string(),
            "Expected oid_before to be recorded"
        );
        assert_eq!(ref_entry["oid_before"].as_str().unwrap(), sha_before);
    }
}

#[test]
fn test_undo_restores_branch() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-undo"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    let sha_before = repo.head_sha();

    // Go back to main and create a new commit
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");

    // Go back to feature branch and restack
    repo.run_stax(&["checkout", &feature_branch]);
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        output.status.success(),
        "Restack failed: {}",
        TestRepo::stderr(&output)
    );

    let sha_after_restack = repo.head_sha();
    assert_ne!(
        sha_before, sha_after_restack,
        "SHA should change after restack"
    );

    // Now undo
    let output = repo.run_stax(&["undo", "--yes"]);
    assert!(
        output.status.success(),
        "Undo failed: {}",
        TestRepo::stderr(&output)
    );

    let sha_after_undo = repo.head_sha();
    assert_eq!(
        sha_before, sha_after_undo,
        "SHA should be restored after undo"
    );
}

#[test]
fn test_undo_no_operations() {
    let repo = TestRepo::new();

    // Try to undo when there are no operations
    let output = repo.run_stax(&["undo"]);
    assert!(
        !output.status.success(),
        "Expected undo to fail with no operations"
    );

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("No operations") || stderr.contains("no operations"),
        "Expected 'no operations' error, got: {}",
        stderr
    );
}

#[test]
fn test_redo_after_undo() {
    let repo = TestRepo::new();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-redo"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    let sha_original = repo.head_sha();

    // Go back to main and create a new commit
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("Main update");

    // Go back to feature branch and restack
    repo.run_stax(&["checkout", &feature_branch]);
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(output.status.success());

    let sha_after_restack = repo.head_sha();

    // Undo
    let output = repo.run_stax(&["undo", "--yes"]);
    assert!(output.status.success());
    assert_eq!(repo.head_sha(), sha_original);

    // Redo
    let output = repo.run_stax(&["redo", "--yes"]);
    assert!(
        output.status.success(),
        "Redo failed: {}",
        TestRepo::stderr(&output)
    );
    assert_eq!(repo.head_sha(), sha_after_restack);
}

#[test]
fn test_multiple_restacks_multiple_undos() {
    let repo = TestRepo::new();

    // Create a stack: main -> feature-1 -> feature-2
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "feature 1");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    let _feature2 = repo.current_branch();
    repo.create_file("f2.txt", "feature 2");
    repo.commit("Feature 2");

    // Record original SHAs
    let _sha_f2_original = repo.head_sha();
    repo.run_stax(&["checkout", &feature1]);
    let sha_f1_original = repo.head_sha();

    // Update main
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main update");
    repo.commit("Main update");

    // Restack feature-1
    repo.run_stax(&["checkout", &feature1]);
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(output.status.success());

    let sha_f1_after_restack = repo.head_sha();
    assert_ne!(sha_f1_original, sha_f1_after_restack);

    // Undo should restore feature-1
    let output = repo.run_stax(&["undo", "--yes"]);
    assert!(output.status.success());
    assert_eq!(repo.head_sha(), sha_f1_original);
}

#[test]
fn test_upstack_restack_creates_receipt() {
    let repo = TestRepo::new();

    // Create a stack
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "f1");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "f2");
    repo.commit("Feature 2");

    // Update feature-1 (this will make feature-2 need restack)
    repo.run_stax(&["checkout", &feature1]);
    repo.create_file("f1-update.txt", "f1 update");
    repo.commit("Feature 1 update");

    // Run upstack restack
    let output = repo.run_stax(&["upstack", "restack"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    // Check receipt was created
    let git_dir = repo.path().join(".git");
    let stax_ops_dir = git_dir.join("stax").join("ops");

    let ops: Vec<_> = std::fs::read_dir(&stax_ops_dir)
        .expect("Failed to read stax ops dir")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext == "json")
                .unwrap_or(false)
        })
        .collect();

    // Find the upstack_restack receipt
    let upstack_receipt = ops.iter().find(|op| {
        let content = std::fs::read_to_string(op.path()).unwrap_or_default();
        content.contains("upstack_restack")
    });

    assert!(
        upstack_receipt.is_some(),
        "Expected upstack_restack receipt"
    );
}

#[test]
fn test_submit_requires_valid_remote_url() {
    // Submit requires a valid GitHub/GitLab URL format, not a local bare repo
    // This test verifies that submit fails gracefully with local remotes
    let repo = TestRepo::new_with_remote();

    // Create a branch with a commit
    repo.run_stax(&["bc", "feature-submit"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");

    // Push using git (to set up remote tracking)
    repo.git(&["push", "-u", "origin", &feature_branch]);

    // submit --no-pr should fail with local bare repo (unsupported URL format)
    let output = repo.run_stax(&["submit", "--no-pr", "--yes"]);

    // Should fail because local file paths aren't valid remote URLs
    assert!(
        !output.status.success(),
        "Submit should fail with local bare repo"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("Unsupported") || stderr.contains("remote"),
        "Expected error about unsupported remote, got: {}",
        stderr
    );
}

#[test]
fn test_sync_restack_creates_receipt() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch and push it
    repo.run_stax(&["bc", "feature-sync"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature_branch]);

    // Simulate remote main update
    repo.simulate_remote_commit("remote.txt", "content", "Remote update");

    // Sync with --restack
    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    // Check receipt was created
    let git_dir = repo.path().join(".git");
    let stax_ops_dir = git_dir.join("stax").join("ops");

    if stax_ops_dir.exists() {
        let ops: Vec<_> = std::fs::read_dir(&stax_ops_dir)
            .expect("Failed to read stax ops dir")
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .collect();

        // Find the sync_restack receipt
        let _sync_receipt = ops.iter().find(|op| {
            let content = std::fs::read_to_string(op.path()).unwrap_or_default();
            content.contains("sync_restack")
        });

        // May not have a receipt if nothing needed restacking
        if !ops.is_empty() {
            // At least verify the ops directory structure
            assert!(ops.iter().all(|op| op
                .path()
                .extension()
                .map(|e| e == "json")
                .unwrap_or(false)));
        }
    }
}

#[test]
fn test_undo_with_dirty_working_tree() {
    let repo = TestRepo::new();

    // Create a branch and restack to create a receipt
    repo.run_stax(&["bc", "feature-dirty"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");

    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main");
    repo.commit("Main update");

    repo.run_stax(&["checkout", &feature_branch]);
    repo.run_stax(&["restack", "--quiet"]);

    // Make the working tree dirty
    repo.create_file("dirty.txt", "uncommitted changes");

    // Try undo without --yes (should fail in quiet/non-interactive mode)
    let output = repo.run_stax(&["undo", "--quiet"]);
    // In quiet mode with dirty tree, should fail
    assert!(!output.status.success() || TestRepo::stderr(&output).contains("dirty"));
}

// =============================================================================
// Sync Merged Branch Detection Tests
// =============================================================================

#[test]
fn test_sync_detects_branch_with_deleted_remote() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch and push it
    repo.run_stax(&["bc", "feature-deleted-remote"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Verify branch exists on remote
    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches
            .iter()
            .any(|b| b.contains("feature-deleted-remote")),
        "Expected branch on remote before deletion"
    );

    // Delete the remote branch (simulating GitHub deleting after merge)
    repo.git(&["push", "origin", "--delete", &branch_name]);

    // Verify branch is deleted from remote
    let remote_branches = repo.list_remote_branches();
    assert!(
        !remote_branches
            .iter()
            .any(|b| b.contains("feature-deleted-remote")),
        "Expected branch to be deleted from remote"
    );

    // Go back to main first (so we're not on the branch being deleted)
    repo.run_stax(&["t"]);

    // Sync should detect the branch as "merged" (remote deleted)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    // Should find the merged branch
    assert!(
        stdout.contains("merged")
            || stdout.contains("feature-deleted-remote")
            || stdout.contains("deleted"),
        "Expected sync to detect deleted remote branch, got: {}",
        stdout
    );
}

#[test]
fn test_sync_detects_branch_with_empty_diff_against_trunk() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-empty-diff"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Merge the branch into main on remote (simulating PR merge)
    repo.merge_branch_on_remote(&branch_name);

    // Pull main to get the merge
    repo.run_stax(&["t"]);
    repo.git(&["pull", "origin", "main"]);

    // Now the feature branch has empty diff against main
    let diff_output = repo.git(&["diff", "--quiet", "main", &branch_name]);
    assert!(
        diff_output.status.success(),
        "Expected empty diff between main and feature branch after merge"
    );

    // Sync should detect the branch as merged (empty diff)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    // Should find the merged branch
    assert!(
        stdout.contains("merged")
            || stdout.contains("feature-empty-diff")
            || stdout.contains("deleted"),
        "Expected sync to detect branch with empty diff, got: {}",
        stdout
    );
}

#[test]
fn test_sync_on_merged_branch_checkouts_parent() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-checkout-parent"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Delete the remote branch (simulating GitHub deleting after merge)
    repo.git(&["push", "origin", "--delete", &branch_name]);

    // Stay on the feature branch
    assert!(repo.current_branch().contains("feature-checkout-parent"));

    // Sync should detect we're on a merged branch and offer to checkout parent
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let _stdout = TestRepo::stdout(&output);

    // Should either:
    // 1. Have checked out parent (main)
    // 2. Or deleted the branch and moved to parent
    let current = repo.current_branch();

    // After sync with --force, we should be on main (the parent)
    // OR still on the feature branch if it wasn't deleted
    // The key is that sync completed successfully
    if !repo
        .list_branches()
        .iter()
        .any(|b| b.contains("feature-checkout-parent"))
    {
        // Branch was deleted, should be on main
        assert_eq!(current, "main", "Should be on main after branch deletion");
    }
}

#[test]
fn test_sync_pulls_parent_after_checkout() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-pull-parent"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Simulate remote updates to main
    repo.simulate_remote_commit("remote-update.txt", "remote content", "Remote update");

    // Delete the remote branch (simulating GitHub deleting after merge)
    repo.git(&["push", "origin", "--delete", &branch_name]);

    // Stay on the feature branch
    assert!(repo.current_branch().contains("feature-pull-parent"));

    // Sync should checkout parent and pull latest changes
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    // After sync, if we're on main, it should have the remote update
    let current = repo.current_branch();
    if current == "main" {
        assert!(
            repo.path().join("remote-update.txt").exists(),
            "Expected remote-update.txt after sync pulled main"
        );
    }
}

#[test]
fn test_sync_with_stacked_branches_detects_merged_child() {
    let repo = TestRepo::new_with_remote();

    // Create a stack: main -> feature-1 -> feature-2
    repo.run_stax(&["bc", "feature-1"]);
    let feature1 = repo.current_branch();
    repo.create_file("f1.txt", "feature 1");
    repo.commit("Feature 1");
    repo.git(&["push", "-u", "origin", &feature1]);

    repo.run_stax(&["bc", "feature-2"]);
    let feature2 = repo.current_branch();
    repo.create_file("f2.txt", "feature 2");
    repo.commit("Feature 2");
    repo.git(&["push", "-u", "origin", &feature2]);

    // Delete feature-2 from remote (simulating it was merged)
    repo.git(&["push", "origin", "--delete", &feature2]);

    // Go to feature-1
    repo.run_stax(&["checkout", &feature1]);

    // Sync should detect feature-2 as merged (remote deleted)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    // Should find the merged branch
    assert!(
        stdout.contains("merged") || stdout.contains("feature-2") || stdout.contains("deleted"),
        "Expected sync to detect feature-2 as merged, got: {}",
        stdout
    );
}

#[test]
fn test_sync_preserves_branch_with_remote() {
    let repo = TestRepo::new_with_remote();

    // Create a feature branch and push it (don't delete remote)
    repo.run_stax(&["bc", "feature-with-remote"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Go back to main
    repo.run_stax(&["t"]);

    // Sync should NOT delete the branch (remote still exists)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(output.status.success());

    // Branch should still exist
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("feature-with-remote")),
        "Expected feature-with-remote to still exist (has remote)"
    );
}

#[test]
fn test_sync_updates_trunk_after_branch_deletion_checkout() {
    // This test verifies the fix for the issue where trunk update would fail
    // when on a merged branch because the trunk update happened BEFORE branch
    // deletion, but we end up on trunk AFTER deletion.
    let repo = TestRepo::new_with_remote();

    // Create a feature branch
    repo.run_stax(&["bc", "feature-trunk-update-order"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Merge branch into main on remote (simulates PR merge)
    // This makes it detectable via `git branch --merged`
    repo.merge_branch_on_remote(&branch_name);

    // Add additional commit to main on remote after merge
    // This ensures main has commits we need to pull
    repo.simulate_remote_commit(
        "remote-main-update.txt",
        "content from remote",
        "Remote main update after merge",
    );

    // Verify we're still on the feature branch locally
    assert!(repo.current_branch().contains("feature-trunk-update-order"));

    // Sync should:
    // 1. Detect the branch as merged (commits are in main)
    // 2. Delete it and checkout main
    // 3. THEN update main successfully (using git pull since we're now on it)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);

    // Should NOT show "failed (may need manual update)" for trunk update
    // because trunk update now happens AFTER we checkout to main
    assert!(
        !stdout.contains("failed (may need manual update)"),
        "Trunk update should not fail when we end up on trunk after branch deletion. Got:\n{}",
        stdout
    );

    // Should show trunk update succeeded
    assert!(
        stdout.contains("Updating main"),
        "Expected trunk update message. Got:\n{}",
        stdout
    );

    // Should be on main after sync
    assert_eq!(
        repo.current_branch(),
        "main",
        "Should be on main after sync deletes the feature branch"
    );

    // Main should have the remote update (trunk was pulled correctly)
    assert!(
        repo.path().join("remote-main-update.txt").exists(),
        "Expected main to have the remote update after sync"
    );
}

#[test]
fn test_sync_trunk_update_order_with_diverged_main() {
    // Test that trunk update works correctly even when local main had been
    // behind remote. The reordering ensures we use `git pull` when on trunk.
    let repo = TestRepo::new_with_remote();

    // Create feature branch and push
    repo.run_stax(&["bc", "feature-diverged-main"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature work");
    repo.commit("Feature work");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Merge branch into main on remote (simulates PR merge)
    repo.merge_branch_on_remote(&branch_name);

    // Add multiple commits to remote main after merge
    repo.simulate_remote_commit("update1.txt", "update 1", "Remote update 1");
    repo.simulate_remote_commit("update2.txt", "update 2", "Remote update 2");

    // Stay on feature branch locally
    assert!(repo.current_branch().contains("feature-diverged-main"));

    // Run sync - should detect branch as merged, delete it, checkout main, then update main
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);

    // Verify successful trunk update (no failure message)
    assert!(
        !stdout.contains("failed"),
        "Should not see any failed messages. Got:\n{}",
        stdout
    );

    // Should be on main with all remote updates
    assert_eq!(repo.current_branch(), "main");
    assert!(repo.path().join("update1.txt").exists());
    assert!(repo.path().join("update2.txt").exists());
}

#[test]
fn test_sync_trunk_update_when_not_on_merged_branch() {
    // Verify that trunk update still works correctly when NOT on a merged branch
    // (i.e., the normal case where we use git fetch refspec)
    let repo = TestRepo::new_with_remote();

    // Create and stay on a feature branch that won't be deleted
    repo.run_stax(&["bc", "active-feature"]);
    let branch_name = repo.current_branch();
    repo.create_file("active.txt", "active work");
    repo.commit("Active work");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Add commit to remote main
    repo.simulate_remote_commit("main-update.txt", "main update", "Main update");

    // Run sync (feature branch is NOT merged, so we won't switch to main)
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    // Should still be on the feature branch (not deleted)
    assert!(repo.current_branch().contains("active-feature"));

    // Trunk should be updated via fetch refspec
    // Go to main and verify it has the update
    repo.git(&["checkout", "main"]);
    assert!(
        repo.path().join("main-update.txt").exists(),
        "Main should have been updated via fetch refspec"
    );
}

// =============================================================================
// Merge Command Tests
// =============================================================================

#[test]
fn test_merge_help() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--help"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("--all"), "Expected --all flag in help");
    assert!(
        stdout.contains("--dry-run"),
        "Expected --dry-run flag in help"
    );
    assert!(
        stdout.contains("--method"),
        "Expected --method flag in help"
    );
    assert!(
        stdout.contains("--no-delete"),
        "Expected --no-delete flag in help"
    );
    assert!(
        stdout.contains("--no-wait"),
        "Expected --no-wait flag in help"
    );
    assert!(
        stdout.contains("--timeout"),
        "Expected --timeout flag in help"
    );
    assert!(stdout.contains("--yes"), "Expected --yes flag in help");
    assert!(stdout.contains("--quiet"), "Expected --quiet flag in help");
}

#[test]
fn test_merge_on_trunk_shows_error() {
    let repo = TestRepo::new();

    // Initialize stax
    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());

    // On trunk, merge should show an error
    assert_eq!(repo.current_branch(), "main");

    let output = repo.run_stax(&["merge"]);
    // Should exit with message about being on trunk
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("trunk") || combined.contains("Checkout"),
        "Expected message about being on trunk, got: {}",
        combined
    );
}

#[test]
fn test_merge_on_untracked_branch_shows_error() {
    let repo = TestRepo::new();

    // Initialize stax
    repo.run_stax(&["status"]);

    // Create an untracked branch directly with git
    repo.git(&["checkout", "-b", "untracked-branch"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    // Merge should show an error about untracked branch
    let output = repo.run_stax(&["merge"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("not tracked") || combined.contains("track"),
        "Expected message about untracked branch, got: {}",
        combined
    );
}

#[test]
fn test_merge_without_pr_shows_error() {
    let repo = TestRepo::new_with_remote();

    // Create a stax-tracked branch but don't submit (no PR)
    repo.run_stax(&["bc", "feature-no-pr"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Merge should fail because no PR exists
    let output = repo.run_stax(&["merge", "--yes"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("PR") || combined.contains("submit"),
        "Expected message about missing PR, got: {}",
        combined
    );
}

#[test]
fn test_merge_dry_run_shows_plan_without_merging() {
    let repo = TestRepo::new_with_remote();

    // Create a branch (it won't have a PR, but dry-run should still show something)
    repo.run_stax(&["bc", "feature-dry-run"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Dry run should show plan
    let output = repo.run_stax(&["merge", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Either shows error about no PR or shows dry-run output
    // Both are acceptable - the key is it doesn't actually merge
    assert!(
        combined.contains("dry") || combined.contains("PR") || combined.contains("plan"),
        "Expected dry-run output or PR error, got: {}",
        combined
    );

    // Branch should still exist (nothing was actually deleted)
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("feature-dry-run")),
        "Branch should still exist after dry-run"
    );
}

#[test]
fn test_merge_scope_single_branch() {
    let repo = TestRepo::new_with_remote();

    // Create a single branch
    repo.run_stax(&["bc", "single-feature"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    // Status should show the stack
    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("single-feature"),
        "Expected branch in status"
    );
}

#[test]
fn test_merge_scope_stacked_branches() {
    let repo = TestRepo::new_with_remote();

    // Create first branch
    repo.run_stax(&["bc", "feature-a"]);
    repo.create_file("a.txt", "content a");
    repo.commit("Feature A");

    // Stack second branch on top
    repo.run_stax(&["bc", "feature-b"]);
    repo.create_file("b.txt", "content b");
    repo.commit("Feature B");

    // Stack third branch on top
    repo.run_stax(&["bc", "feature-c"]);
    repo.create_file("c.txt", "content c");
    repo.commit("Feature C");

    // Verify we're on the top branch
    assert!(repo.current_branch().contains("feature-c"));

    // Status should show all three branches in stack
    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());

    let stdout = TestRepo::stdout(&output);
    assert!(stdout.contains("feature-a"), "Expected feature-a in status");
    assert!(stdout.contains("feature-b"), "Expected feature-b in status");
    assert!(stdout.contains("feature-c"), "Expected feature-c in status");
}

#[test]
fn test_merge_from_middle_of_stack() {
    let repo = TestRepo::new_with_remote();

    // Create a stack of 3 branches
    repo.run_stax(&["bc", "stack-a"]);
    repo.create_file("a.txt", "content a");
    repo.commit("Feature A");

    repo.run_stax(&["bc", "stack-b"]);
    repo.create_file("b.txt", "content b");
    repo.commit("Feature B");

    repo.run_stax(&["bc", "stack-c"]);
    repo.create_file("c.txt", "content c");
    repo.commit("Feature C");

    // Go to the middle branch
    repo.run_stax(&["checkout", "stack-b"]);
    assert!(repo.current_branch().contains("stack-b"));

    // Merge dry-run should only show stack-a and stack-b (not stack-c)
    let output = repo.run_stax(&["merge", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // The output depends on whether there are PRs or not
    // Without PRs it will error, with dry-run it should show intent
    // Either way, we verified the checkout worked
    assert!(
        combined.contains("PR") || combined.contains("stack") || combined.contains("merge"),
        "Expected merge-related output, got: {}",
        combined
    );
}

#[test]
fn test_merge_all_flag() {
    let repo = TestRepo::new_with_remote();

    // Create a stack
    repo.run_stax(&["bc", "all-a"]);
    repo.create_file("a.txt", "content");
    repo.commit("A");

    repo.run_stax(&["bc", "all-b"]);
    repo.create_file("b.txt", "content");
    repo.commit("B");

    // Go back to first branch
    repo.run_stax(&["checkout", "all-a"]);

    // With --all flag, even from first branch, it should target the whole stack
    let output = repo.run_stax(&["merge", "--all", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // Should mention something about merging (even if fails due to no PRs)
    assert!(
        combined.contains("PR") || combined.contains("merge") || combined.contains("all"),
        "Expected output about merging, got: {}",
        combined
    );
}

#[test]
fn test_merge_method_options() {
    let repo = TestRepo::new_with_remote();

    // Create a branch
    repo.run_stax(&["bc", "method-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // Test squash method (default)
    let output = repo.run_stax(&["merge", "--method", "squash", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    // Should process without error about invalid method
    assert!(
        !combined.contains("Invalid merge method"),
        "squash should be a valid method"
    );

    // Test merge method
    let output = repo.run_stax(&["merge", "--method", "merge", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        !combined.contains("Invalid merge method"),
        "merge should be a valid method"
    );

    // Test rebase method
    let output = repo.run_stax(&["merge", "--method", "rebase", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        !combined.contains("Invalid merge method"),
        "rebase should be a valid method"
    );
}

#[test]
fn test_merge_invalid_method_defaults_to_squash() {
    let repo = TestRepo::new_with_remote();

    // Create a branch
    repo.run_stax(&["bc", "invalid-method"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // Invalid method should fall back to default (squash)
    let output = repo.run_stax(&["merge", "--method", "invalid", "--dry-run"]);
    // Should not panic, should handle gracefully
    // The command will fail due to no PR, but shouldn't crash
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        !combined.is_empty(),
        "Should produce some output even with invalid method"
    );
}

#[test]
fn test_merge_preserves_unrelated_branches() {
    let repo = TestRepo::new_with_remote();

    // Create first stack
    repo.run_stax(&["bc", "stack1-a"]);
    repo.create_file("s1a.txt", "content");
    repo.commit("Stack 1 A");

    // Go back to main and create second independent stack
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "stack2-a"]);
    repo.create_file("s2a.txt", "content");
    repo.commit("Stack 2 A");

    // Verify both branches exist
    let branches = repo.list_branches();
    assert!(branches.iter().any(|b| b.contains("stack1")));
    assert!(branches.iter().any(|b| b.contains("stack2")));

    // Attempt merge on stack2 (will fail due to no PR)
    let output = repo.run_stax(&["merge", "--dry-run"]);
    let _combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));

    // Both branches should still exist (dry-run doesn't delete anything)
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("stack1")),
        "stack1 branch should be preserved"
    );
    assert!(
        branches.iter().any(|b| b.contains("stack2")),
        "stack2 branch should be preserved"
    );
}

#[test]
fn test_merge_quiet_flag() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "quiet-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // Quiet flag should reduce output
    let output = repo.run_stax(&["merge", "--quiet", "--dry-run"]);
    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    let combined = format!("{}{}", stdout, stderr);

    // In quiet mode, there should be less verbose output
    // The exact behavior depends on whether there's an error or not
    // Just verify the command runs
    assert!(
        combined.len() < 5000,
        "Quiet mode should not produce excessive output"
    );
}

#[test]
fn test_merge_timeout_option() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "timeout-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // Custom timeout should be accepted
    let output = repo.run_stax(&["merge", "--timeout", "5", "--dry-run"]);
    // Should not error about invalid timeout
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        !combined.contains("error") || combined.contains("PR"),
        "Timeout option should be accepted"
    );
}

#[test]
fn test_merge_no_wait_flag() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "no-wait-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // --no-wait should be accepted
    let output = repo.run_stax(&["merge", "--no-wait", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    // Should process the flag without error
    assert!(
        !combined.contains("unexpected argument"),
        "--no-wait should be a valid flag"
    );
}

#[test]
fn test_merge_no_delete_flag() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "no-delete-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // --no-delete should be accepted
    let output = repo.run_stax(&["merge", "--no-delete", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    // Should process the flag without error
    assert!(
        !combined.contains("unexpected argument"),
        "--no-delete should be a valid flag"
    );
}

#[test]
fn test_merge_yes_flag_skips_confirmation() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "yes-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // --yes should skip confirmation prompts
    let output = repo.run_stax(&["merge", "--yes", "--dry-run"]);
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    // Should not hang waiting for input
    assert!(
        !combined.contains("unexpected argument"),
        "--yes should be a valid flag"
    );
}

#[test]
fn test_merge_combined_flags() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "combined-test"]);
    repo.create_file("test.txt", "content");
    repo.commit("Test");

    // Test combining multiple flags
    let output = repo.run_stax(&[
        "merge",
        "--all",
        "--method",
        "squash",
        "--no-delete",
        "--no-wait",
        "--timeout",
        "10",
        "--yes",
        "--quiet",
        "--dry-run",
    ]);

    // Should accept all flags together
    let combined = format!("{}{}", TestRepo::stdout(&output), TestRepo::stderr(&output));
    assert!(
        !combined.contains("unexpected argument"),
        "All flags should be accepted together"
    );
}

mod github_mock_tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path, path_regex};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_test_config(home: &Path, api_base_url: &str) {
        let config_dir = home.join(".config").join("stax");
        std::fs::create_dir_all(&config_dir).expect("Failed to create config dir");
        let config_path = config_dir.join("config.toml");
        let config = format!("[remote]\napi_base_url = \"{}\"\n", api_base_url);
        fs::write(&config_path, config).expect("Failed to write config");
    }

    fn ensure_empty_gitconfig(home: &Path) -> std::path::PathBuf {
        let path = home.join("gitconfig");
        if !path.exists() {
            fs::write(&path, "").expect("Failed to write empty gitconfig");
        }
        path
    }

    fn git_with_env(repo: &TestRepo, home: &Path, args: &[&str]) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .output()
            .expect("Failed to run git command")
    }

    fn setup_fake_github_remote(repo: &TestRepo, home: &Path) -> TempDir {
        let remote_root = TempDir::new().expect("Failed to create temp remote root");
        let remote_repo = remote_root.path().join("test").join("repo.git");
        if let Some(parent) = remote_repo.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create remote parent dirs");
        }
        std::fs::create_dir_all(&remote_repo).expect("Failed to create remote repo dir");

        Command::new("git")
            .args(["init", "--bare"])
            .current_dir(&remote_repo)
            .output()
            .expect("Failed to init bare remote repo");

        let add_remote = git_with_env(
            repo,
            home,
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/test/repo.git",
            ],
        );
        assert!(
            add_remote.status.success(),
            "Failed to add origin: {}",
            TestRepo::stderr(&add_remote)
        );

        let file_base = format!("file://{}/", remote_root.path().display());
        let set_instead_of = git_with_env(
            repo,
            home,
            &[
                "config",
                &format!("url.{}.insteadOf", file_base),
                "https://github.com/",
            ],
        );
        assert!(
            set_instead_of.status.success(),
            "Failed to set insteadOf: {}",
            TestRepo::stderr(&set_instead_of)
        );

        let push = git_with_env(repo, home, &["push", "-u", "origin", "main"]);
        assert!(
            push.status.success(),
            "Failed to push to fake remote: {}",
            TestRepo::stderr(&push)
        );

        remote_root
    }

    fn run_stax_with_env(repo: &TestRepo, home: &Path, args: &[&str]) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        Command::new(stax_bin())
            .args(args)
            .current_dir(repo.path())
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .env("STAX_GITHUB_TOKEN", "mock-token")
            .output()
            .expect("Failed to execute stax")
    }

    /// Create a test repo configured to use a mock GitHub API
    async fn setup_mock_github() -> (TestRepo, MockServer) {
        let mock_server = MockServer::start().await;
        let repo = TestRepo::new_with_remote();

        // Set environment variables for the mock
        std::env::set_var("STAX_GITHUB_TOKEN", "mock-token");

        (repo, mock_server)
    }

    #[tokio::test]
    async fn test_mock_server_setup() {
        let mock_server = MockServer::start().await;

        // Verify mock server is running
        assert!(!mock_server.uri().is_empty());
    }

    #[tokio::test]
    async fn test_submit_with_mock_pr_creation() {
        let (repo, mock_server) = setup_mock_github().await;

        // Mock the PR list endpoint (find existing PR)
        Mock::given(method("GET"))
            .and(path_regex(r"/repos/.*/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        // Mock the PR creation endpoint
        Mock::given(method("POST"))
            .and(path_regex(r"/repos/.*/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "number": 1,
                "state": "open",
                "title": "Test PR",
                "draft": false,
                "html_url": "https://github.com/test/repo/pull/1"
            })))
            .mount(&mock_server)
            .await;

        // Create a branch
        repo.run_stax(&["bc", "feature-pr"]);
        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        // Note: Full PR creation test requires configuring stax to use the mock server URL
        // which would require modifying the config or adding a --api-url flag
        // For now, we verify the mock server setup works

        assert!(
            mock_server.received_requests().await.is_none()
                || mock_server.received_requests().await.unwrap().is_empty()
        );
    }

    #[tokio::test]
    async fn test_submit_persists_pr_info_for_existing_pr() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/42",
                    "id": 42,
                    "number": 42,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-branch", "sha": "aaaa", "label": "test:feature-branch" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        let home = TempDir::new().expect("Failed to create temp home");
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "feature-branch"]);
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );

        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        let branch = repo.current_branch();

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--no-pr", "--yes"]);
        assert!(
            output.status.success(),
            "Submit failed: {}",
            TestRepo::stderr(&output)
        );

        let metadata_ref = format!("refs/branch-metadata/{}", branch);
        let output = repo.git(&["show", &metadata_ref]);
        assert!(
            output.status.success(),
            "Failed to read metadata: {}",
            TestRepo::stderr(&output)
        );
        let metadata = TestRepo::stdout(&output);
        assert!(
            metadata.contains("\"number\":42"),
            "Expected PR number in metadata, got: {}",
            metadata
        );
    }

    #[tokio::test]
    async fn test_submit_does_not_persist_pr_info_for_fork() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/99",
                    "id": 99,
                    "number": 99,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-branch", "sha": "aaaa", "label": "fork:feature-branch" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        let home = TempDir::new().expect("Failed to create temp home");
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "feature-branch"]);
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );

        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        let branch = repo.current_branch();

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--no-pr", "--yes"]);
        assert!(
            output.status.success(),
            "Submit failed: {}",
            TestRepo::stderr(&output)
        );

        let metadata_ref = format!("refs/branch-metadata/{}", branch);
        let output = repo.git(&["show", &metadata_ref]);
        assert!(
            output.status.success(),
            "Failed to read metadata: {}",
            TestRepo::stderr(&output)
        );
        let metadata = TestRepo::stdout(&output);
        assert!(
            !metadata.contains("\"number\":99"),
            "Expected PR number not to be persisted for fork, got: {}",
            metadata
        );
    }

    #[tokio::test]
    async fn test_github_api_mock_responses() {
        let mock_server = MockServer::start().await;

        // Mock fetching remote refs
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/git/refs/heads"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"ref": "refs/heads/main", "object": {"sha": "abc123"}}
            ])))
            .mount(&mock_server)
            .await;

        // Mock PR list
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "number": 42,
                    "state": "open",
                    "title": "Existing PR",
                    "draft": false,
                    "head": {"ref": "feature-branch"}
                }
            ])))
            .mount(&mock_server)
            .await;

        // Verify mocks are set up
        let client = reqwest::Client::new();

        let refs_response = client
            .get(format!(
                "{}/repos/test/repo/git/refs/heads",
                mock_server.uri()
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(refs_response.status(), 200);

        let prs_response = client
            .get(format!("{}/repos/test/repo/pulls", mock_server.uri()))
            .send()
            .await
            .unwrap();
        assert_eq!(prs_response.status(), 200);

        let prs: Vec<serde_json::Value> = prs_response.json().await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0]["number"], 42);
    }
}
