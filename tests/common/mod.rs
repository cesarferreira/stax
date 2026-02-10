//! Common test utilities for stax integration tests
//!
//! This module provides reusable test infrastructure including:
//! - `TestRepo` - Creates real temporary git repositories for testing
//! - Helper methods for common test scenarios
//! - Assertion utilities for test output

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Output};
use tempfile::TempDir;

/// Get path to compiled binary (built by cargo test)
pub fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

/// A test repository that creates a temporary git repo with proper initialization
pub struct TestRepo {
    dir: TempDir,
    /// Optional bare repository acting as "origin" remote
    #[allow(dead_code)]
    remote_dir: Option<TempDir>,
}

#[allow(dead_code)]
impl TestRepo {
    /// Create a new test repository with git init and an initial commit on main
    pub fn new() -> Self {
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
    pub fn new_with_remote() -> Self {
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
    pub fn remote_path(&self) -> Option<PathBuf> {
        self.remote_dir.as_ref().map(|d| d.path().to_path_buf())
    }

    /// Simulate pushing a commit to the remote main branch (as if another user did it)
    /// This clones the remote, makes a commit, and pushes back
    pub fn simulate_remote_commit(&self, filename: &str, content: &str, message: &str) {
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
    pub fn merge_branch_on_remote(&self, branch: &str) {
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
    pub fn list_remote_branches(&self) -> Vec<String> {
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
    pub fn find_branch_containing(&self, pattern: &str) -> Option<String> {
        self.list_branches()
            .into_iter()
            .find(|b| b.contains(pattern))
    }

    /// Check if current branch name contains the given substring
    pub fn current_branch_contains(&self, pattern: &str) -> bool {
        self.current_branch().contains(pattern)
    }

    /// Get the path to the test repository
    pub fn path(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    /// Run a stax command in this repository
    pub fn run_stax(&self, args: &[&str]) -> Output {
        Command::new(stax_bin())
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to execute stax")
    }

    /// Run a stax command in a specific directory
    pub fn run_stax_in(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new(stax_bin())
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("Failed to execute stax")
    }

    /// Get stdout as string from output
    pub fn stdout(output: &Output) -> String {
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Get stderr as string from output
    pub fn stderr(output: &Output) -> String {
        String::from_utf8_lossy(&output.stderr).to_string()
    }

    /// Create a file in the repository
    pub fn create_file(&self, name: &str, content: &str) {
        let file_path = self.path().join(name);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        fs::write(file_path, content).expect("Failed to write file");
    }

    /// Create a commit with all staged changes
    pub fn commit(&self, message: &str) {
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
    pub fn current_branch(&self) -> String {
        let output = Command::new("git")
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(self.path())
            .output()
            .expect("Failed to get current branch");

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get list of all branches
    pub fn list_branches(&self) -> Vec<String> {
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
    pub fn get_commit_sha(&self, reference: &str) -> String {
        let output = Command::new("git")
            .args(["rev-parse", reference])
            .current_dir(self.path())
            .output()
            .expect("Failed to get commit SHA");

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get the HEAD commit SHA
    pub fn head_sha(&self) -> String {
        self.get_commit_sha("HEAD")
    }

    /// Run a raw git command
    pub fn git(&self, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to run git command")
    }

    /// Run a raw git command in a specific directory
    pub fn git_in(&self, cwd: &Path, args: &[&str]) -> Output {
        Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("Failed to run git command")
    }

    // =========================================================================
    // New Helper Methods
    // =========================================================================

    /// Create a stack of branches with commits
    /// Returns the list of actual branch names created (may include prefix)
    pub fn create_stack(&self, names: &[&str]) -> Vec<String> {
        let mut created_branches = Vec::new();

        for name in names.iter() {
            let output = self.run_stax(&["bc", name]);
            assert!(
                output.status.success(),
                "Failed to create branch {}: {}",
                name,
                Self::stderr(&output)
            );

            let branch_name = self.current_branch();
            created_branches.push(branch_name);

            // Add a unique file and commit for each branch
            self.create_file(&format!("{}.txt", name), &format!("content for {}", name));
            self.commit(&format!("Commit for {}", name));

            // Verify we created the branch
            assert!(
                self.current_branch_contains(name),
                "Expected branch containing '{}', got '{}'",
                name,
                self.current_branch()
            );
        }

        created_branches
    }

    /// Navigate to the top of the stack
    pub fn navigate_to_top(&self) -> Output {
        self.run_stax(&["top"])
    }

    /// Navigate to the bottom of the stack (first branch above trunk)
    pub fn navigate_to_bottom(&self) -> Output {
        self.run_stax(&["bottom"])
    }

    /// Navigate up the stack by count (default 1)
    pub fn navigate_up(&self, count: Option<usize>) -> Output {
        match count {
            Some(n) => self.run_stax(&["up", &n.to_string()]),
            None => self.run_stax(&["up"]),
        }
    }

    /// Navigate down the stack by count (default 1)
    pub fn navigate_down(&self, count: Option<usize>) -> Output {
        match count {
            Some(n) => self.run_stax(&["down", &n.to_string()]),
            None => self.run_stax(&["down"]),
        }
    }

    /// Create a rebase conflict scenario
    /// Returns the branch name that will have a conflict when restacked
    pub fn create_conflict_scenario(&self) -> String {
        // Create a feature branch
        self.run_stax(&["bc", "conflict-branch"]);
        let branch_name = self.current_branch();

        // Modify a file on the feature branch
        self.create_file("conflict.txt", "feature content\nline 2\nline 3");
        self.commit("Feature changes");

        // Go back to main and make conflicting changes
        self.run_stax(&["t"]);
        self.create_file("conflict.txt", "main content\nline 2\nline 3");
        self.commit("Main changes");

        // Go back to the feature branch (it now needs restack and will conflict)
        self.run_stax(&["checkout", &branch_name]);

        branch_name
    }

    /// Check if there's an active rebase in progress
    pub fn has_rebase_in_progress(&self) -> bool {
        let git_dir = self.path().join(".git");
        git_dir.join("rebase-merge").exists() || git_dir.join("rebase-apply").exists()
    }

    /// Abort any in-progress rebase
    pub fn abort_rebase(&self) {
        let _ = self.git(&["rebase", "--abort"]);
    }

    /// Resolve conflicts by accepting "ours" version and continue
    pub fn resolve_conflicts_ours(&self) {
        // Stage all files (accepting current state)
        self.git(&["add", "-A"]);
    }

    /// Get status JSON output parsed
    pub fn get_status_json(&self) -> Value {
        let output = self.run_stax(&["status", "--json"]);
        assert!(
            output.status.success(),
            "Status failed: {}",
            Self::stderr(&output)
        );
        serde_json::from_str(&Self::stdout(&output)).expect("Invalid JSON from status")
    }

    /// Get the parent of the current branch from stax metadata
    pub fn get_current_parent(&self) -> Option<String> {
        let json = self.get_status_json();
        let current = self.current_branch();

        json["branches"]
            .as_array()
            .and_then(|branches| {
                branches
                    .iter()
                    .find(|b| b["name"].as_str() == Some(&current))
            })
            .and_then(|branch| branch["parent"].as_str())
            .map(|s| s.to_string())
    }

    /// Get the children of a branch from stax metadata
    pub fn get_children(&self, branch: &str) -> Vec<String> {
        let json = self.get_status_json();

        json["branches"]
            .as_array()
            .map(|branches| {
                branches
                    .iter()
                    .filter(|b| b["parent"].as_str() == Some(branch))
                    .filter_map(|b| b["name"].as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

// =============================================================================
// Output Assertion Helpers
// =============================================================================

/// Extension trait for fluent assertions on command Output
#[allow(dead_code)]
pub trait OutputAssertions {
    fn assert_success(&self) -> &Self;
    fn assert_failure(&self) -> &Self;
    fn assert_stdout_contains(&self, s: &str) -> &Self;
    fn assert_stderr_contains(&self, s: &str) -> &Self;
    fn assert_stdout_not_contains(&self, s: &str) -> &Self;
}

#[allow(dead_code)]
impl OutputAssertions for Output {
    fn assert_success(&self) -> &Self {
        assert!(
            self.status.success(),
            "Expected success but got failure.\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
        self
    }

    fn assert_failure(&self) -> &Self {
        assert!(
            !self.status.success(),
            "Expected failure but got success.\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        );
        self
    }

    fn assert_stdout_contains(&self, s: &str) -> &Self {
        let stdout = String::from_utf8_lossy(&self.stdout);
        assert!(
            stdout.contains(s),
            "Expected stdout to contain '{}', got:\n{}",
            s,
            stdout
        );
        self
    }

    fn assert_stderr_contains(&self, s: &str) -> &Self {
        let stderr = String::from_utf8_lossy(&self.stderr);
        assert!(
            stderr.contains(s),
            "Expected stderr to contain '{}', got:\n{}",
            s,
            stderr
        );
        self
    }

    fn assert_stdout_not_contains(&self, s: &str) -> &Self {
        let stdout = String::from_utf8_lossy(&self.stdout);
        assert!(
            !stdout.contains(s),
            "Expected stdout NOT to contain '{}', but it did:\n{}",
            s,
            stdout
        );
        self
    }
}

// =============================================================================
// Test for the test infrastructure itself
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_common_repo_setup() {
        let repo = TestRepo::new();
        assert!(repo.path().exists());
        assert_eq!(repo.current_branch(), "main");
        assert!(repo.list_branches().contains(&"main".to_string()));
    }

    #[test]
    fn test_common_create_stack() {
        let repo = TestRepo::new();
        let branches = repo.create_stack(&["feature-a", "feature-b"]);

        assert_eq!(branches.len(), 2);
        assert!(branches[0].contains("feature-a"));
        assert!(branches[1].contains("feature-b"));

        // Should be on the last created branch
        assert!(repo.current_branch_contains("feature-b"));
    }

    #[test]
    fn test_output_assertions() {
        let repo = TestRepo::new();

        let output = repo.run_stax(&["status"]);
        output.assert_success().assert_stdout_contains("main");

        let output = repo.run_stax(&["checkout", "nonexistent"]);
        output.assert_failure();
    }
}
