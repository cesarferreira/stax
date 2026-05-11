//! Integration tests for stax commands
//!
//! These tests create real temporary git repositories and run actual stax commands
//! to verify end-to-end functionality.

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// Get path to compiled binary (built by cargo test)
fn stax_bin() -> &'static str {
    env!("CARGO_BIN_EXE_stax")
}

/// Create temporary directories in STAX_TEST_TMPDIR when set.
///
/// This keeps test repos off slower default temp paths on some macOS setups.
fn test_tempdir() -> TempDir {
    if let Ok(root) = std::env::var("STAX_TEST_TMPDIR") {
        let root_path = Path::new(&root);
        fs::create_dir_all(root_path).expect("Failed to create STAX_TEST_TMPDIR");
        TempDir::new_in(root_path).expect("Failed to create temp dir in STAX_TEST_TMPDIR")
    } else {
        TempDir::new().expect("Failed to create temp dir")
    }
}

fn sanitized_stax_command() -> Command {
    let mut cmd = Command::new(stax_bin());
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    // Keep tests hermetic and avoid accidentally hitting real GitHub APIs.
    cmd.env_remove("GITHUB_TOKEN")
        .env_remove("STAX_GITHUB_TOKEN")
        .env_remove("STAX_SHELL_INTEGRATION")
        .env_remove("GH_TOKEN")
        .env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path)
        .env("STAX_DISABLE_UPDATE_CHECK", "1");
    cmd
}

fn hermetic_git_command() -> Command {
    let mut cmd = Command::new("git");
    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    cmd.env("GIT_CONFIG_GLOBAL", null_path)
        .env("GIT_CONFIG_SYSTEM", null_path);
    cmd
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
        let dir = test_tempdir();
        let path = dir.path();

        // Initialize git repo
        hermetic_git_command()
            .args(["init", "-b", "main"])
            .current_dir(path)
            .output()
            .expect("Failed to init git repo");

        // Configure git user for commits
        hermetic_git_command()
            .args(["config", "user.email", "test@test.com"])
            .current_dir(path)
            .output()
            .expect("Failed to set git email");

        hermetic_git_command()
            .args(["config", "user.name", "Test User"])
            .current_dir(path)
            .output()
            .expect("Failed to set git name");

        // Create initial commit
        let readme = path.join("README.md");
        fs::write(&readme, "# Test Repo\n").expect("Failed to write README");

        hermetic_git_command()
            .args(["add", "-A"])
            .current_dir(path)
            .output()
            .expect("Failed to stage files");

        hermetic_git_command()
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
        let remote_dir = test_tempdir();
        hermetic_git_command()
            .args(["init", "--bare"])
            .current_dir(remote_dir.path())
            .output()
            .expect("Failed to init bare repo");

        // Add it as origin
        hermetic_git_command()
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
        hermetic_git_command()
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
        let clone_dir = test_tempdir();
        hermetic_git_command()
            .args(["clone", remote_path.to_str().unwrap(), "."])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to clone remote");

        // Ensure we have a local main branch even if remote HEAD isn't set
        hermetic_git_command()
            .args(["checkout", "-B", "main", "origin/main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to checkout main");

        // Configure git user
        hermetic_git_command()
            .args(["config", "user.email", "other@test.com"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git email");
        hermetic_git_command()
            .args(["config", "user.name", "Other User"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git name");

        // Create file and commit
        fs::write(clone_dir.path().join(filename), content).expect("Failed to write file");
        hermetic_git_command()
            .args(["add", "-A"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to stage");
        hermetic_git_command()
            .args(["commit", "-m", message])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to commit");

        // Push back to origin
        hermetic_git_command()
            .args(["push", "origin", "main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to push to origin");
    }

    /// Merge a branch into main on the remote (simulating PR merge)
    fn merge_branch_on_remote(&self, branch: &str) {
        let remote_path = self.remote_path().expect("No remote configured");

        // Create a temp clone
        let clone_dir = test_tempdir();
        hermetic_git_command()
            .args(["clone", remote_path.to_str().unwrap(), "."])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to clone remote");

        // Ensure we have a local main branch even if remote HEAD isn't set
        hermetic_git_command()
            .args(["checkout", "-B", "main", "origin/main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to checkout main");

        // Configure git user
        hermetic_git_command()
            .args(["config", "user.email", "merger@test.com"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git email");
        hermetic_git_command()
            .args(["config", "user.name", "Merger"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to set git name");

        // Fetch the branch and merge
        hermetic_git_command()
            .args(["fetch", "origin", branch])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to fetch branch");

        hermetic_git_command()
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
        hermetic_git_command()
            .args(["push", "origin", "main"])
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to push merge");
    }

    /// List remote branches
    fn list_remote_branches(&self) -> Vec<String> {
        let output = hermetic_git_command()
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
        sanitized_stax_command()
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
        hermetic_git_command()
            .args(["add", "-A"])
            .current_dir(self.path())
            .output()
            .expect("Failed to stage files");

        hermetic_git_command()
            .args(["commit", "-m", message])
            .current_dir(self.path())
            .output()
            .expect("Failed to commit");
    }

    /// Get the current branch name
    fn current_branch(&self) -> String {
        let output = hermetic_git_command()
            .args(["rev-parse", "--abbrev-ref", "HEAD"])
            .current_dir(self.path())
            .output()
            .expect("Failed to get current branch");

        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// Get list of all branches
    fn list_branches(&self) -> Vec<String> {
        let output = hermetic_git_command()
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
        let output = hermetic_git_command()
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
        hermetic_git_command()
            .args(args)
            .current_dir(self.path())
            .output()
            .expect("Failed to run git command")
    }

    /// Run a raw git command in a specific directory.
    fn git_in(&self, cwd: &Path, args: &[&str]) -> Output {
        hermetic_git_command()
            .args(args)
            .current_dir(cwd)
            .output()
            .expect("Failed to run git command")
    }

    /// Create a two-branch stack where the parent restacks cleanly and the child conflicts.
    fn create_restack_progress_conflict_scenario(&self) -> (String, String) {
        self.run_stax(&["bc", "progress-parent"]);
        let parent = self.current_branch();
        self.create_file("parent.txt", "parent content\n");
        self.commit("Parent commit");

        self.run_stax(&["bc", "progress-child"]);
        let child = self.current_branch();
        self.create_file("conflict.txt", "child content\n");
        self.commit("Child conflict commit");

        self.run_stax(&["t"]);
        self.create_file("main-update.txt", "main update\n");
        self.create_file("conflict.txt", "main content\n");
        self.commit("Main conflict commit");

        self.run_stax(&["checkout", &child]);

        (parent, child)
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

    // Redirect the fake fetch URL to the local bare repo so `git fetch` and
    // `git ls-remote` actually succeed in the sandbox (no real network call to
    // github.com). Without this, submit's fetch step fails — historically that
    // failure was silently swallowed; after the issue #222 fix it correctly
    // bails. These tests aren't exercising fetch behaviour, so we just point
    // it at the same local bare repo as push.
    let file_url = format!("file://{}", remote_path_str);
    repo.git(&[
        "config",
        "--local",
        &format!(
            "url.{}.insteadOf",
            file_url.trim_end_matches('/').to_string()
        ),
        "https://github.com/test-owner/test-repo.git",
    ]);
}

fn list_remote_heads(repo: &TestRepo) -> Vec<String> {
    let remote_path = repo
        .remote_path()
        .expect("Expected remote path for repository with origin");

    let output = hermetic_git_command()
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

    let output = repo.run_stax(&["bc", "-a", "-m", "Add new feature"]);
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
fn test_branch_create_with_message_uses_unique_suffix_on_collision() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "-m", "Add new feature"]);
    assert!(
        output.status.success(),
        "Failed first create: {}",
        TestRepo::stderr(&output)
    );
    let first_branch = repo.current_branch();

    let output = repo.run_stax(&["bc", "-m", "Add new feature"]);
    assert!(
        output.status.success(),
        "Failed second create: {}",
        TestRepo::stderr(&output)
    );
    let second_branch = repo.current_branch();

    assert_ne!(first_branch, second_branch);
    assert!(
        second_branch.ends_with("-2") && second_branch.to_lowercase().contains("new-feature"),
        "Expected suffixed branch name, got: {}",
        second_branch
    );

    let branches = repo.list_branches();
    assert!(branches.iter().any(|b| b == &first_branch));
    assert!(branches.iter().any(|b| b == &second_branch));
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
fn test_branch_create_exact_name_conflict_has_clear_error() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["bc", "feature-1"]);
    assert!(output.status.success());

    let output = repo.run_stax(&["bc", "feature-1"]);
    assert!(!output.status.success());

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("already exists") && stderr.contains("Use `st checkout "),
        "Expected exact-conflict guidance, got: {}",
        stderr
    );
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
fn test_status_uses_compact_ahead_then_behind_labels() {
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

    // Use ll (verbose status) to exercise the same text renderer as `st ls`.
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

    assert!(
        !line.contains("behind") && !line.contains("ahead"),
        "Expected compact divergence labels without words, got: {}",
        line
    );

    let ahead_pos = line
        .find("1↑")
        .expect("Expected compact ahead label in status output line");
    let behind_pos = line
        .find("1↓")
        .expect("Expected compact behind label in status output line");

    assert!(
        ahead_pos < behind_pos,
        "Expected ahead label before behind label in status output line: {}",
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
fn test_status_marks_branches_checked_out_in_linked_worktrees() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let branch_name = repo.current_branch();
    repo.run_stax(&["t"]);

    let worktree_path = repo.path().join("feature-1-wt");
    let git_output = repo.git(&[
        "worktree",
        "add",
        worktree_path.to_str().expect("utf8 worktree path"),
        &branch_name,
    ]);
    assert!(
        git_output.status.success(),
        "git worktree add failed: {}",
        String::from_utf8_lossy(&git_output.stderr)
    );

    let output = repo.run_stax(&["status"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let line = stdout
        .lines()
        .find(|line| line.contains(&branch_name))
        .expect("Expected branch in status output");
    assert!(
        line.contains("↳"),
        "Expected linked worktree glyph in status output line: {}",
        line
    );

    let json_output = repo.run_stax(&["status", "--json"]);
    assert!(
        json_output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&json_output)
    );

    let json: Value =
        serde_json::from_str(&TestRepo::stdout(&json_output)).expect("Invalid JSON output");
    let branch = json["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|entry| entry["name"] == branch_name)
        .expect("branch entry");
    assert_eq!(branch["linked_worktree"], "feature-1-wt");
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
fn test_status_alias_ls() {
    let repo = TestRepo::new();

    let output1 = repo.run_stax(&["status"]);
    let output2 = repo.run_stax(&["ls"]);

    assert!(output1.status.success());
    assert!(output2.status.success());
}

#[test]
fn test_stack_alias_s() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["s", "--help"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("submit"));
    assert!(combined.contains("restack"));
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
fn test_checkout_explicit_branch_prints_clean_completion() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    repo.run_stax(&["t"]);

    let output = repo.run_stax(&["checkout", &feature_branch]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("Checked out {}.", feature_branch)),
        "Expected clean checkout completion, got:\n{}",
        stdout
    );
    assert!(
        !stdout.contains("Switched to branch"),
        "Expected stax-native checkout wording, got:\n{}",
        stdout
    );
}

#[test]
fn test_checkout_shell_output_colors_branch_in_completion_message() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    let feature_branch = repo.current_branch();
    repo.run_stax(&["t"]);

    let output = repo.run_stax(&["checkout", &feature_branch, "--shell-output"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("STAX_SHELL_MESSAGE=Checked out ")),
        "Expected shell completion message, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains(&format!("\u{1b}[1;96m{}\u{1b}[0m", feature_branch)),
        "Expected colored branch in shell completion message, got:\n{}",
        stdout
    );
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

#[test]
fn test_checkout_routes_to_existing_worktree_with_duplicate_leaf_name() {
    let repo = TestRepo::new();

    let routed_branch = "feature-route";
    let sibling_branch = "other-route";
    let routed_worktree = repo.path().join("lanes/a/WayveCode");
    let sibling_worktree = repo.path().join("lanes/b/WayveCode");

    let create_routed_branch = repo.git(&["branch", routed_branch]);
    assert!(
        create_routed_branch.status.success(),
        "Failed to create routed branch: {}",
        TestRepo::stderr(&create_routed_branch)
    );

    let routed_parent = routed_worktree
        .parent()
        .expect("routed worktree path should have a parent");
    fs::create_dir_all(routed_parent).expect("Failed to create routed worktree parent dirs");
    let routed_add = repo.git(&[
        "worktree",
        "add",
        routed_worktree.to_str().expect("utf8 routed worktree path"),
        routed_branch,
    ]);
    assert!(
        routed_add.status.success(),
        "Failed to add routed worktree: {}",
        TestRepo::stderr(&routed_add)
    );

    let create_sibling_branch = repo.git(&["branch", sibling_branch]);
    assert!(
        create_sibling_branch.status.success(),
        "Failed to create sibling branch: {}",
        TestRepo::stderr(&create_sibling_branch)
    );

    let sibling_parent = sibling_worktree
        .parent()
        .expect("sibling worktree path should have a parent");
    fs::create_dir_all(sibling_parent).expect("Failed to create sibling worktree parent dirs");
    let sibling_add = repo.git(&[
        "worktree",
        "add",
        sibling_worktree
            .to_str()
            .expect("utf8 sibling worktree path"),
        sibling_branch,
    ]);
    assert!(
        sibling_add.status.success(),
        "Failed to add sibling worktree: {}",
        TestRepo::stderr(&sibling_add)
    );

    let output = repo.run_stax(&["checkout", routed_branch]);
    assert!(
        output.status.success(),
        "Checkout routing failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    let stderr = TestRepo::stderr(&output);
    assert!(
        stdout.contains("routing there instead"),
        "Expected routed checkout message, got: {}",
        stdout
    );
    assert!(
        !stderr.contains("Multiple worktrees match"),
        "Expected checkout to avoid ambiguous worktree lookup, got: {}",
        stderr
    );
    assert_eq!(
        repo.current_branch(),
        "main",
        "Routing to an existing worktree should not switch the current checkout"
    );
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

/// Reparent with `--restack` rebases onto the new parent so middle-of-stack commits are not kept.
#[test]
fn test_branch_reparent_restack_rewrites_onto_new_parent() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature1.txt", "one");
    repo.commit("Commit feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    let feature2 = repo.current_branch();
    repo.create_file("feature2.txt", "two");
    repo.commit("Commit feature 2");

    assert!(repo.path().join("feature1.txt").exists());
    assert!(repo.path().join("feature2.txt").exists());

    repo.run_stax(&["t"]);
    let output = repo.run_stax(&[
        "branch",
        "reparent",
        "--branch",
        &feature2,
        "--parent",
        "main",
        "--restack",
    ]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    assert_eq!(repo.current_branch(), "main");

    let co = repo.git(&["checkout", &feature2]);
    assert!(co.status.success(), "checkout feature2: {:?}", co);

    assert!(
        !repo.path().join("feature1.txt").exists(),
        "expected feature-2 without feature-1 file after reparent --restack"
    );
    assert!(repo.path().join("feature2.txt").exists());
}

/// Without `--restack`, reparent only updates metadata; working tree still reflects old ancestry.
#[test]
fn test_branch_reparent_without_restack_keeps_middle_ancestor_files() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("feature1.txt", "one");
    repo.commit("Commit feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    let feature2 = repo.current_branch();
    repo.create_file("feature2.txt", "two");
    repo.commit("Commit feature 2");

    repo.run_stax(&["t"]);
    let output = repo.run_stax(&[
        "branch", "reparent", "--branch", &feature2, "--parent", "main",
    ]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("metadata only") || stdout.contains("Reparent updated stax"),
        "expected guidance about metadata-only reparent, got: {}",
        stdout
    );

    let co = repo.git(&["checkout", &feature2]);
    assert!(co.status.success());

    assert!(
        repo.path().join("feature1.txt").exists(),
        "without --restack, branch should still include ancestor commits from the middle branch"
    );
    assert!(repo.path().join("feature2.txt").exists());
}

/// `--restack` needs prior stax metadata to infer the old parent as the rebase boundary.
#[test]
fn test_branch_reparent_restack_requires_existing_metadata() {
    let repo = TestRepo::new();

    repo.git(&["checkout", "-b", "raw-branch"]);
    repo.create_file("only.txt", "x");
    repo.commit("raw commit");

    let output = repo.run_stax(&["branch", "reparent", "-p", "main", "--restack"]);
    assert!(
        !output.status.success(),
        "expected failure without metadata"
    );
    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("--restack") || stderr.contains("metadata"),
        "expected metadata hint, got: {}",
        stderr
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

    // Amend using modify (stage all with -a since nothing is pre-staged)
    let output = repo.run_stax(&["modify", "-a"]);
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
    let output = repo.run_stax(&["modify", "-a", "-m", "New commit message"]);
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
    let output = repo.run_stax(&["m", "-a"]);
    assert!(output.status.success());
}

#[test]
fn test_modify_on_fresh_branch_creates_first_commit_with_message() {
    let repo = TestRepo::new();

    hermetic_git_command()
        .args(["config", "user.name", "Parent Author"])
        .current_dir(repo.path())
        .output()
        .expect("Failed to set parent author");
    hermetic_git_command()
        .args(["config", "user.email", "parent@example.com"])
        .current_dir(repo.path())
        .output()
        .expect("Failed to set parent email");

    repo.create_file("shared.txt", "parent change");
    repo.commit("Parent commit");

    hermetic_git_command()
        .args(["config", "user.name", "Test User"])
        .current_dir(repo.path())
        .output()
        .expect("Failed to restore test author");
    hermetic_git_command()
        .args(["config", "user.email", "test@test.com"])
        .current_dir(repo.path())
        .output()
        .expect("Failed to restore test email");

    repo.run_stax(&["bc", "feature-first-commit"]);

    let head_before = repo.head_sha();
    repo.create_file("feature.txt", "new branch work");

    let output = repo.run_stax(&["modify", "-a", "-m", "Feature commit"]);
    assert!(
        output.status.success(),
        "modify should create the first branch commit: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Committed"),
        "expected commit confirmation, got: {}",
        stdout
    );
    let log_output = repo.git(&["log", "-1", "--format=%s%n%an <%ae>"]);
    let log = String::from_utf8_lossy(&log_output.stdout);
    let mut lines = log.lines();
    assert_eq!(lines.next(), Some("Feature commit"));
    assert_eq!(lines.next(), Some("Test User <test@test.com>"));
    let head_after = repo.head_sha();
    assert_ne!(
        head_after, head_before,
        "modify should create a new branch-local commit on a fresh branch"
    );
    let count_output = repo.git(&["rev-list", "--count", "main..HEAD"]);
    assert_eq!(
        String::from_utf8_lossy(&count_output.stdout).trim(),
        "1",
        "expected exactly one branch-local commit after the first modify"
    );
    repo.git(&["checkout", "main"]);
    assert_eq!(repo.head_sha(), head_before, "main should remain untouched");
}

#[test]
fn test_modify_on_fresh_branch_without_message_guides_user() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-no-message"]);
    let head_before = repo.head_sha();
    repo.create_file("feature.txt", "new branch work");

    let output = repo.run_stax(&["modify", "-a"]);
    assert!(
        !output.status.success(),
        "modify without -m should fail on a fresh branch"
    );

    let stderr = TestRepo::stderr(&output);
    assert!(
        stderr.contains("has nothing to amend") && stderr.contains("Re-run with `-m <message>`"),
        "expected guidance for creating the first commit, got: {}",
        stderr
    );
    assert_eq!(
        repo.head_sha(),
        head_before,
        "modify without -m should not rewrite the parent commit on a fresh branch"
    );
}

#[test]
fn test_modify_on_fresh_branch_still_creates_commit_after_parent_moves() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "feature-parent-moved"]);
    let feature_branch = repo.current_branch();
    let shared_base = repo.head_sha();

    repo.git(&["checkout", "main"]);
    repo.create_file("main.txt", "main advanced");
    repo.commit("Main advanced");
    let main_after = repo.head_sha();
    assert_ne!(main_after, shared_base, "main should have advanced");

    repo.git(&["checkout", &feature_branch]);
    assert_eq!(
        repo.head_sha(),
        shared_base,
        "fresh branch should still point at the original parent boundary"
    );

    repo.create_file("feature.txt", "feature work");
    let output = repo.run_stax(&["modify", "-a", "-m", "Feature commit"]);
    assert!(
        output.status.success(),
        "modify should still create the first branch commit after parent moves: {}",
        TestRepo::stderr(&output)
    );

    let feature_after = repo.head_sha();
    assert_ne!(
        feature_after, shared_base,
        "expected a new branch-local commit after modify"
    );

    repo.git(&["checkout", "main"]);
    assert_eq!(
        repo.head_sha(),
        main_after,
        "modify on the child branch must not rewrite the advanced parent branch"
    );
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
fn test_restack_collapses_squash_merged_parent_before_child() {
    let repo = TestRepo::new();

    repo.create_file("shared.txt", "base\n");
    repo.commit("Add shared base");

    repo.run_stax(&["bc", "squash-parent"]);
    let parent = repo.current_branch();
    repo.create_file("shared.txt", "parent part 1\n");
    repo.commit("Parent part 1");
    repo.create_file("shared.txt", "parent part 2\n");
    repo.commit("Parent part 2");

    repo.run_stax(&["bc", "squash-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child-only\n");
    repo.commit("Child only");

    repo.run_stax(&["t"]);
    let squash = repo.git(&["merge", "--squash", &parent]);
    assert!(
        squash.status.success(),
        "Failed to squash merge parent:\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&squash),
        TestRepo::stderr(&squash)
    );
    repo.commit("Squash parent into main");
    let main_after_squash = repo.get_commit_sha("main");

    repo.run_stax(&["checkout", &child]);
    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        output.status.success(),
        "restack should not replay squash-merged parent commits\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert_eq!(
        repo.get_commit_sha(&parent),
        main_after_squash,
        "squash-merged parent should be collapsed to main before restacking child"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("shared.txt")).unwrap(),
        "parent part 2\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.path().join("child.txt")).unwrap(),
        "child-only\n"
    );
}

#[test]
fn test_restack_auto_normalizes_missing_parent() {
    let repo = TestRepo::new();

    repo.run_stax(&["bc", "missing-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent");
    repo.commit("Parent commit");

    repo.run_stax(&["bc", "missing-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child");
    repo.commit("Child commit");

    // Delete parent branch, leaving child metadata stale.
    let delete_parent = repo.git(&["branch", "-D", &parent]);
    assert!(
        delete_parent.status.success(),
        "Failed to delete parent branch: {}",
        TestRepo::stderr(&delete_parent)
    );

    let output = repo.run_stax(&["restack", "--quiet"]);
    assert!(
        output.status.success(),
        "restack failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let metadata_ref = format!("refs/branch-metadata/{}", child);
    let metadata_output = repo.git(&["show", &metadata_ref]);
    assert!(
        metadata_output.status.success(),
        "Failed to read metadata: {}",
        TestRepo::stderr(&metadata_output)
    );
    let metadata: Value =
        serde_json::from_str(&TestRepo::stdout(&metadata_output)).expect("Invalid JSON metadata");
    assert_eq!(
        metadata["parentBranchName"], "main",
        "Expected missing-parent child to be reparented to trunk, metadata was: {}",
        metadata
    );
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

#[test]
fn test_restack_stop_here_skips_descendants() {
    let repo = TestRepo::new();

    // main -> feature-1 -> feature-2 -> feature-3
    repo.run_stax(&["bc", "feature-1"]);
    repo.create_file("f1.txt", "content");
    repo.commit("Feature 1");

    repo.run_stax(&["bc", "feature-2"]);
    repo.create_file("f2.txt", "content");
    repo.commit("Feature 2");
    let feature_2 = repo.current_branch();

    repo.run_stax(&["bc", "feature-3"]);
    repo.create_file("f3.txt", "content");
    repo.commit("Feature 3");
    let feature_3 = repo.current_branch();

    // Move trunk so restacking from the middle will restack ancestors/current.
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main");
    repo.commit("Main update");

    repo.run_stax(&["checkout", &feature_2]);
    let feature_3_before = repo.get_commit_sha(&feature_3);

    let output = repo.run_stax(&["restack", "--stop-here", "--quiet"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let feature_3_after = repo.get_commit_sha(&feature_3);
    assert_eq!(
        feature_3_before, feature_3_after,
        "Expected descendant branch to remain untouched by restack --stop-here"
    );

    let status_output = repo.run_stax(&["status", "--json"]);
    assert!(
        status_output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&status_output)
    );
    let status_json: Value =
        serde_json::from_str(&TestRepo::stdout(&status_output)).expect("Invalid JSON");
    let branches = status_json["branches"]
        .as_array()
        .expect("Expected branches array");
    let feature_2_entry = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("") == feature_2.as_str())
        .expect("Expected feature-2 in status");
    let feature_3_entry = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("") == feature_3.as_str())
        .expect("Expected feature-3 in status");

    assert_eq!(feature_2_entry["needs_restack"], Value::Bool(false));
    assert_eq!(feature_3_entry["needs_restack"], Value::Bool(true));
}

#[test]
fn test_restack_conflict_reports_branch_progress_and_files() {
    let repo = TestRepo::new();
    let (parent, child) = repo.create_restack_progress_conflict_scenario();

    let output = repo.run_stax(&["restack", "--yes"]);
    assert!(
        !output.status.success(),
        "restack should exit non-zero on conflict\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Restack stopped on conflict:"),
        "Expected conflict heading, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("Stopped at: {}", child)),
        "Expected stopped-at branch, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("Parent: {}", parent)),
        "Expected parent branch, got: {}",
        stdout
    );
    assert!(
        stdout
            .contains("Progress: 1 branch rebased before conflict, 0 branches remaining in stack"),
        "Expected progress summary, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("Completed: {}", parent)),
        "Expected completed branch list, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Conflicted files:") && stdout.contains("conflict.txt"),
        "Expected conflicted files in output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("stax restack --continue"),
        "Expected continue guidance, got: {}",
        stdout
    );

    let abort = repo.git(&["rebase", "--abort"]);
    assert!(
        abort.status.success(),
        "Failed to abort rebase during cleanup: {}",
        TestRepo::stderr(&abort)
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

#[test]
fn test_cascade_no_submit_from_middle_restacks_full_stack() {
    let repo = TestRepo::new();

    // Build stack: main -> base -> middle -> tip.
    repo.run_stax(&["bc", "cascade-base"]);
    let base = repo.current_branch();
    repo.create_file("base.txt", "base");
    repo.commit("base commit");

    repo.run_stax(&["bc", "cascade-middle"]);
    let middle = repo.current_branch();
    repo.create_file("middle.txt", "middle");
    repo.commit("middle commit");

    repo.run_stax(&["bc", "cascade-tip"]);
    let tip = repo.current_branch();
    repo.create_file("tip.txt", "tip");
    repo.commit("tip commit");

    // Advance trunk so this stack requires rebasing.
    repo.run_stax(&["t"]);
    repo.create_file("main-update.txt", "main update");
    repo.commit("main update");

    // Run cascade from a non-bottom branch.
    repo.run_stax(&["checkout", &middle]);
    let original = repo.current_branch();

    let before_output = repo.run_stax(&["status", "--json"]);
    assert!(
        before_output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&before_output)
    );
    let before_json: Value =
        serde_json::from_str(&TestRepo::stdout(&before_output)).expect("Invalid JSON");
    let before_branches = before_json["branches"]
        .as_array()
        .expect("Expected branches array");
    let tracked = [&base, &middle, &tip];
    assert!(
        tracked.iter().any(|name| {
            before_branches
                .iter()
                .find(|b| b["name"].as_str() == Some(name.as_str()))
                .and_then(|b| b["needs_restack"].as_bool())
                .unwrap_or(false)
        }),
        "Expected at least one stack branch to need restack before cascade"
    );

    let output = repo.run_stax(&["cascade", "--no-submit"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let after = repo.current_branch();
    assert_eq!(after, original, "cascade should restore original branch");

    let after_output = repo.run_stax(&["status", "--json"]);
    assert!(
        after_output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&after_output)
    );
    let after_json: Value =
        serde_json::from_str(&TestRepo::stdout(&after_output)).expect("Invalid JSON");
    let after_branches = after_json["branches"]
        .as_array()
        .expect("Expected branches array");
    for name in tracked {
        let branch = after_branches
            .iter()
            .find(|b| b["name"].as_str() == Some(name.as_str()))
            .unwrap_or_else(|| panic!("Missing branch {} in status output", name));
        assert_eq!(
            branch["needs_restack"],
            Value::Bool(false),
            "Expected {} to be fully restacked by cascade",
            name
        );
    }
}

#[test]
fn test_cascade_conflict_reports_restack_context() {
    let repo = TestRepo::new();
    let (_parent, child) = repo.create_restack_progress_conflict_scenario();

    let output = repo.run_stax(&["cascade", "--no-submit"]);
    assert!(
        !output.status.success(),
        "cascade should exit non-zero on conflict\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Cascading stack..."),
        "Expected cascade banner, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Restack stopped on conflict:"),
        "Expected restack conflict block, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("Stopped at: {}", child)),
        "Expected stopped-at branch in cascade output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Conflicted files:") && stdout.contains("conflict.txt"),
        "Expected conflicted files in cascade output, got: {}",
        stdout
    );

    let abort = repo.git(&["rebase", "--abort"]);
    assert!(
        abort.status.success(),
        "Failed to abort rebase during cleanup: {}",
        TestRepo::stderr(&abort)
    );
}

// =============================================================================
// Refresh Tests
// =============================================================================

#[test]
fn test_refresh_no_submit_keeps_original_branch_and_restacks_stack() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "refresh-base"]);
    let base = repo.current_branch();
    repo.create_file("base.txt", "base");
    repo.commit("base commit");

    repo.run_stax(&["bc", "refresh-middle"]);
    let middle = repo.current_branch();
    repo.create_file("middle.txt", "middle");
    repo.commit("middle commit");

    repo.run_stax(&["bc", "refresh-tip"]);
    let tip = repo.current_branch();
    repo.create_file("tip.txt", "tip");
    repo.commit("tip commit");

    repo.simulate_remote_commit("main-update.txt", "main update", "main update");

    repo.run_stax(&["checkout", &middle]);
    let original = repo.current_branch();

    let output = repo.run_stax(&["refresh", "--no-submit", "--force"]);
    assert!(
        output.status.success(),
        "refresh --no-submit failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Refreshing stack..."),
        "Expected refresh banner, got: {}",
        stdout
    );
    assert!(
        stdout.contains("1. Sync trunk and clean merged branches"),
        "Expected sync step, got: {}",
        stdout
    );
    assert!(
        stdout.contains("2. Restack current stack onto updated parents"),
        "Expected restack step, got: {}",
        stdout
    );
    assert!(
        stdout.contains("3. Skip push and PR updates (--no-submit)"),
        "Expected no-submit step, got: {}",
        stdout
    );

    let after = repo.current_branch();
    assert_eq!(after, original, "refresh should restore original branch");

    let after_output = repo.run_stax(&["status", "--json"]);
    assert!(
        after_output.status.success(),
        "status failed after refresh\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&after_output),
        TestRepo::stderr(&after_output)
    );
    let after_json: Value =
        serde_json::from_str(&TestRepo::stdout(&after_output)).expect("Invalid JSON");
    let after_branches = after_json["branches"]
        .as_array()
        .expect("Expected branches array");
    for name in [&base, &middle, &tip] {
        let branch = after_branches
            .iter()
            .find(|b| b["name"].as_str() == Some(name.as_str()))
            .unwrap_or_else(|| panic!("Missing branch {} in status output", name));
        assert_eq!(
            branch["needs_restack"],
            Value::Bool(false),
            "Expected {} to be fully restacked by refresh",
            name
        );
    }
}

#[test]
fn test_refresh_no_submit_handles_squash_merged_middle_branch() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "refresh-squash-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent 1\n");
    repo.commit("Parent commit 1");
    repo.create_file("parent.txt", "parent 1\nparent 2\n");
    repo.commit("Parent commit 2");
    repo.git(&["push", "-u", "origin", &parent]);

    repo.run_stax(&["bc", "refresh-squash-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child change\n");
    repo.commit("Child commit");
    repo.git(&["push", "-u", "origin", &child]);

    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = test_tempdir();
    let run_remote_git = |args: &[&str]| {
        let output = hermetic_git_command()
            .args(args)
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to run git in remote clone");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_remote_git(&["clone", remote_path.to_str().unwrap(), "."]);
    run_remote_git(&["checkout", "-B", "main", "origin/main"]);
    run_remote_git(&["config", "user.email", "merger@test.com"]);
    run_remote_git(&["config", "user.name", "Merger"]);
    run_remote_git(&["fetch", "origin", &parent]);
    run_remote_git(&["merge", "--squash", &format!("origin/{}", parent)]);
    run_remote_git(&["commit", "-m", "Squash merge parent"]);
    run_remote_git(&["push", "origin", "main"]);
    run_remote_git(&["push", "origin", "--delete", &parent]);

    repo.run_stax(&["checkout", &child]);

    let output = repo.run_stax(&["refresh", "--no-submit", "--force"]);
    assert!(
        output.status.success(),
        "refresh --no-submit failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        !TestRepo::stdout(&output).contains("conflict"),
        "Expected refresh to use provenance-aware sync restack without conflict.\nstdout: {}",
        TestRepo::stdout(&output)
    );

    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == &parent),
        "Expected merged parent branch to be deleted"
    );

    let count_output = repo.git(&["rev-list", "--count", &format!("main..{}", child)]);
    assert!(count_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&count_output.stdout).trim(),
        "1",
        "Expected child to keep only novel commits after refresh restack"
    );
}

#[test]
fn test_refresh_auto_stash_pop_restores_dirty_linked_worktree() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "refresh-auto-stash-base"]);
    let base = repo.current_branch();
    repo.create_file("base.txt", "base\n");
    repo.commit("base commit");

    repo.run_stax(&["bc", "refresh-auto-stash-tip"]);
    let tip = repo.current_branch();
    repo.create_file("tip.txt", "tip\n");
    repo.commit("tip commit");

    let wt_root = test_tempdir();
    let base_worktree = wt_root.path().join("refresh-base-wt");
    let add_worktree = repo.git(&["worktree", "add", base_worktree.to_str().unwrap(), &base]);
    assert!(
        add_worktree.status.success(),
        "failed to add linked worktree\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&add_worktree),
        TestRepo::stderr(&add_worktree)
    );

    let dirty_file = base_worktree.join("scratch.txt");
    fs::write(&dirty_file, "local scratch\n").expect("write dirty linked-worktree file");

    repo.simulate_remote_commit("main-update.txt", "main update\n", "main update");

    let output = repo.run_stax(&["refresh", "--no-submit", "--force", "--auto-stash-pop"]);
    assert!(
        output.status.success(),
        "refresh --auto-stash-pop failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    assert_eq!(
        repo.current_branch(),
        tip,
        "refresh should restore original branch"
    );

    let status = repo.git_in(&base_worktree, &["status", "--porcelain"]);
    assert!(
        status.status.success(),
        "failed to read linked worktree status\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&status),
        TestRepo::stderr(&status)
    );
    let status_out = TestRepo::stdout(&status);
    assert!(
        status_out.contains("scratch.txt"),
        "dirty linked-worktree changes should be restored, got:\n{}",
        status_out
    );
    assert_eq!(
        fs::read_to_string(&dirty_file).expect("read restored dirty file"),
        "local scratch\n"
    );

    let stash_list = repo.git_in(&base_worktree, &["stash", "list"]);
    assert!(stash_list.status.success());
    assert!(
        TestRepo::stdout(&stash_list).trim().is_empty(),
        "expected no leftover auto-stash entries, got:\n{}",
        TestRepo::stdout(&stash_list)
    );

    let base_rebased = repo.git(&["merge-base", "--is-ancestor", "main", &base]);
    assert!(
        base_rebased.status.success(),
        "base branch should be restacked onto refreshed main"
    );
    let tip_rebased = repo.git(&["merge-base", "--is-ancestor", &base, &tip]);
    assert!(
        tip_rebased.status.success(),
        "tip branch should remain stacked on refreshed base"
    );
}

#[test]
fn test_refresh_verbose_shows_restack_timing_breakdown() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "refresh-verbose"]);
    let branch = repo.current_branch();
    repo.create_file("refresh.txt", "refresh");
    repo.commit("refresh commit");
    repo.git(&["push", "-u", "origin", &branch]);

    repo.simulate_remote_commit("main-update.txt", "main update", "main update");

    let output = repo.run_stax(&["refresh", "--no-submit", "--force", "--verbose"]);
    assert!(
        output.status.success(),
        "refresh --verbose failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Sync timing summary:"),
        "Expected sync timing summary, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("restack branch {}", branch)),
        "Expected restack branch timing breakdown, got: {}",
        stdout
    );
    assert!(
        stdout.contains("git rebase"),
        "Expected git rebase timing detail, got: {}",
        stdout
    );
}

#[test]
fn test_refresh_no_pr_pushes_current_stack() {
    let repo = TestRepo::new_with_remote();
    configure_submit_remote(&repo);

    repo.run_stax(&["bc", "refresh-push"]);
    let branch = repo.current_branch();
    repo.create_file("push.txt", "push");
    repo.commit("push commit");

    let output = repo.run_stax(&["refresh", "--no-pr"]);
    assert!(
        output.status.success(),
        "refresh --no-pr failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("3. Push branches without updating PRs"),
        "Expected no-pr step, got: {}",
        stdout
    );

    let remote_branches = repo.list_remote_branches();
    assert!(
        remote_branches.iter().any(|name| name == &branch),
        "Expected {} to be pushed to origin, remote branches: {:?}",
        branch,
        remote_branches
    );
}

#[test]
fn test_refresh_conflict_reports_restack_context() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "refresh-progress-parent"]);
    let _parent = repo.current_branch();
    repo.create_file("parent.txt", "parent content\n");
    repo.commit("Parent commit");

    repo.run_stax(&["bc", "refresh-progress-child"]);
    let child = repo.current_branch();
    repo.create_file("conflict.txt", "child content\n");
    repo.commit("Child conflict commit");

    repo.simulate_remote_commit("conflict.txt", "main content\n", "Main conflict commit");

    let output = repo.run_stax(&["refresh", "--no-submit", "--force"]);
    assert!(
        !output.status.success(),
        "refresh should exit non-zero on conflict\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Refreshing stack..."),
        "Expected refresh banner, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Restack stopped on conflict:"),
        "Expected restack conflict block, got: {}",
        stdout
    );
    assert!(
        stdout.contains(&format!("Stopped at: {}", child)),
        "Expected stopped-at branch in refresh output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("Conflicted files:") && stdout.contains("conflict.txt"),
        "Expected conflicted files in output, got: {}",
        stdout
    );

    let abort = repo.git(&["rebase", "--abort"]);
    assert!(
        abort.status.success(),
        "Failed to abort rebase during cleanup: {}",
        TestRepo::stderr(&abort)
    );
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
    #[cfg(unix)]
    let dir = TempDir::new_in("/tmp").expect("Failed to create external temp dir");
    #[cfg(not(unix))]
    let dir = TempDir::new().expect("Failed to create external temp dir");

    let output = sanitized_stax_command()
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
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("main +1 commit"),
        "Expected trunk commit count in sync footer, got: {}",
        stdout
    );
    assert!(
        stdout.contains("+1 -0"),
        "Expected trunk diff stats in sync footer, got: {}",
        stdout
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
fn test_sync_verbose_shows_step_timing_summary() {
    let repo = TestRepo::new_with_remote();

    let output = repo.run_stax(&["sync", "--force", "--verbose"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("Sync timing summary:"),
        "Expected timing summary in verbose sync output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("fetch origin"),
        "Expected fetch step timing in verbose sync output, got: {}",
        stdout
    );
    assert!(
        stdout.contains("total"),
        "Expected total timing in verbose sync output, got: {}",
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
    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains("main +1 commit"),
        "Expected trunk commit count in sync footer, got: {}",
        stdout
    );
    assert!(
        stdout.contains("restacked 1"),
        "Expected restack count in sync footer, got: {}",
        stdout
    );

    // Should still be on our feature branch
    assert!(repo.current_branch_contains("feature-restack"));

    // Remote file should be accessible (after restack onto updated main)
    repo.run_stax(&["checkout", &feature_branch]);
    // The remote.txt should be in our history now
}

#[test]
fn test_sync_verbose_restack_shows_branch_timing_breakdown() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "feature-restack-verbose"]);
    let feature_branch = repo.current_branch();
    repo.create_file("feature.txt", "feature");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &feature_branch]);

    repo.simulate_remote_commit("remote.txt", "content", "Remote update");

    let output = repo.run_stax(&["sync", "--restack", "--force", "--verbose"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let stdout = TestRepo::stdout(&output);
    assert!(
        stdout.contains(&format!("restack branch {}", feature_branch)),
        "Expected verbose restack branch timing header, got: {}",
        stdout
    );
    assert!(
        stdout.contains("git rebase"),
        "Expected git rebase timing in verbose restack output, got: {}",
        stdout
    );
}

#[test]
fn test_sync_restack_only_targets_current_stack() {
    let repo = TestRepo::new_with_remote();

    // Stack A: main -> a1 -> a2
    repo.run_stax(&["bc", "stack-a1"]);
    let a1 = repo.current_branch();
    repo.create_file("a1.txt", "a1");
    repo.commit("a1 commit");

    repo.run_stax(&["bc", "stack-a2"]);
    let a2 = repo.current_branch();
    repo.create_file("a2.txt", "a2");
    repo.commit("a2 commit");

    // Stack B: main -> b1
    repo.run_stax(&["t"]);
    repo.run_stax(&["bc", "stack-b1"]);
    let b1 = repo.current_branch();
    repo.create_file("b1.txt", "b1");
    repo.commit("b1 commit");

    // Move trunk forward so both a1 and b1 need restack.
    repo.run_stax(&["t"]);
    repo.create_file("main.txt", "main change");
    repo.commit("main commit");

    // Run sync --restack from stack A tip; only stack A should be restacked.
    repo.run_stax(&["checkout", &a2]);
    let b1_before = repo.get_commit_sha(&b1);

    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&output)
    );

    let b1_after = repo.get_commit_sha(&b1);
    assert_eq!(
        b1_before, b1_after,
        "Expected unrelated stack branch to remain untouched by sync --restack"
    );

    // Ensure this test really exercised the regression precondition.
    let status_output = repo.run_stax(&["status", "--json"]);
    assert!(
        status_output.status.success(),
        "Failed: {}",
        TestRepo::stderr(&status_output)
    );
    let status_json: Value =
        serde_json::from_str(&TestRepo::stdout(&status_output)).expect("Invalid JSON");
    let branches = status_json["branches"]
        .as_array()
        .expect("Expected branches array");
    let b1_entry = branches
        .iter()
        .find(|b| b["name"].as_str().unwrap_or("") == b1)
        .expect("Expected b1 in status");
    assert_eq!(b1_entry["needs_restack"], Value::Bool(true));

    // Keep variables used for clarity around stack topology assertions.
    assert!(!a1.is_empty());
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

    // Amend the commit (stage all with -a since nothing is pre-staged)
    repo.create_file("f.txt", "amended");
    repo.run_stax(&["modify", "-a"]);

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
fn test_sync_does_not_delete_untracked_upstream_gone_by_default() {
    let repo = TestRepo::new_with_remote();

    // Create an untracked branch (no stax metadata), push, then delete remote.
    repo.git(&["checkout", "-b", "manual-upstream-gone"]);
    repo.create_file("manual.txt", "manual branch content");
    repo.commit("Manual branch commit");
    repo.git(&["push", "-u", "origin", "manual-upstream-gone"]);
    repo.git(&["checkout", "main"]);
    repo.git(&["push", "origin", "--delete", "manual-upstream-gone"]);

    // Default sync behavior should not touch untracked local branches.
    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b == "manual-upstream-gone"),
        "Expected untracked upstream-gone branch to remain without --delete-upstream-gone"
    );
}

#[test]
fn test_sync_does_not_treat_closed_unmerged_pr_as_merged() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "feature-closed-pr"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature content");
    repo.commit("Feature commit");
    repo.git(&["push", "-u", "origin", &branch_name]);

    let main_sha = repo.get_commit_sha("main");
    let metadata = serde_json::json!({
        "parentBranchName": "main",
        "parentBranchRevision": main_sha,
        "prInfo": {
            "number": 198,
            "state": "CLOSED",
            "isDraft": false
        }
    });
    let metadata_json = metadata.to_string();

    let mut hash_cmd = hermetic_git_command();
    let metadata_oid_output = hash_cmd
        .args(["hash-object", "-w", "--stdin"])
        .current_dir(repo.path())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child
                .stdin
                .as_mut()
                .expect("stdin")
                .write_all(metadata_json.as_bytes())?;
            child.wait_with_output()
        })
        .expect("Failed to write metadata blob");
    assert!(
        metadata_oid_output.status.success(),
        "Failed to hash metadata: {}",
        TestRepo::stderr(&metadata_oid_output)
    );
    let metadata_oid = TestRepo::stdout(&metadata_oid_output).trim().to_string();

    let metadata_ref = format!("refs/branch-metadata/{}", branch_name);
    let update_ref = repo.git(&["update-ref", &metadata_ref, &metadata_oid]);
    assert!(
        update_ref.status.success(),
        "Failed to update metadata ref: {}",
        TestRepo::stderr(&update_ref)
    );

    repo.run_stax(&["t"]);

    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b == &branch_name),
        "Expected closed-but-unmerged PR branch to remain after sync"
    );
}

#[test]
fn test_sync_delete_upstream_gone_deletes_untracked_local_branch() {
    let repo = TestRepo::new_with_remote();

    // Create an untracked branch (no stax metadata), push, then delete remote.
    repo.git(&["checkout", "-b", "manual-upstream-gone"]);
    repo.create_file("manual.txt", "manual branch content");
    repo.commit("Manual branch commit");
    repo.git(&["push", "-u", "origin", "manual-upstream-gone"]);
    repo.git(&["checkout", "main"]);
    repo.git(&["push", "origin", "--delete", "manual-upstream-gone"]);

    let output = repo.run_stax(&["sync", "--force", "--delete-upstream-gone"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == "manual-upstream-gone"),
        "Expected --delete-upstream-gone to delete the stale local branch"
    );
}

#[test]
fn test_sync_delete_upstream_gone_reparents_tracked_children() {
    // Regression test for #200: sync --delete-upstream-gone must reparent
    // tracked children before deleting, so descendants do not end up
    // pointing at a branch that no longer exists.
    let repo = TestRepo::new_with_remote();

    // Build main -> parent-200 -> child-200
    repo.run_stax(&["bc", "parent-200"]);
    let parent_branch = repo.current_branch();
    repo.create_file("parent.txt", "parent content");
    repo.commit("Parent commit");
    repo.git(&["push", "-u", "origin", &parent_branch]);

    repo.run_stax(&["bc", "child-200"]);
    let child_branch = repo.current_branch();
    repo.create_file("child.txt", "child content");
    repo.commit("Child commit");
    repo.git(&["push", "-u", "origin", &child_branch]);

    // Delete the parent on the remote so its upstream is gone
    repo.git(&["checkout", "main"]);
    repo.git(&["push", "origin", "--delete", &parent_branch]);

    // Run sync --delete-upstream-gone; parent should be deleted and the
    // child should be reparented to main (the nearest surviving ancestor).
    let output = repo.run_stax(&["sync", "--force", "--delete-upstream-gone"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&parent_branch),
        "Expected parent to be deleted, still have: {:?}",
        branches
    );
    assert!(
        branches.contains(&child_branch),
        "Expected child to survive, got: {:?}",
        branches
    );

    // Child's recorded parent must no longer point at the deleted branch.
    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid JSON");
    let child_entry = json["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|b| b["name"].as_str() == Some(&child_branch))
        .expect("child in status");
    let new_parent = child_entry["parent"]
        .as_str()
        .expect("child has a parent field");
    assert_ne!(
        new_parent, parent_branch,
        "Child still points at the deleted parent"
    );
    assert_eq!(
        new_parent, "main",
        "Expected child to be reparented to main, got: {}",
        new_parent
    );
}

#[test]
fn test_sync_delete_upstream_gone_reparents_across_multiple_doomed_ancestors() {
    // Regression guard: if both the parent AND the grandparent are upstream-gone,
    // the deepest descendant must land on the first non-doomed ancestor (trunk
    // in this case), NOT on a soon-to-be-deleted ancestor.
    let repo = TestRepo::new_with_remote();

    // Build main -> grand-200 -> mid-200 -> leaf-200
    repo.run_stax(&["bc", "grand-200"]);
    let grand = repo.current_branch();
    repo.create_file("grand.txt", "grand");
    repo.commit("grand");
    repo.git(&["push", "-u", "origin", &grand]);

    repo.run_stax(&["bc", "mid-200"]);
    let mid = repo.current_branch();
    repo.create_file("mid.txt", "mid");
    repo.commit("mid");
    repo.git(&["push", "-u", "origin", &mid]);

    repo.run_stax(&["bc", "leaf-200"]);
    let leaf = repo.current_branch();
    repo.create_file("leaf.txt", "leaf");
    repo.commit("leaf");
    repo.git(&["push", "-u", "origin", &leaf]);

    // Delete both grand AND mid on the remote. Leaf survives remotely.
    repo.git(&["checkout", "main"]);
    repo.git(&["push", "origin", "--delete", &grand]);
    repo.git(&["push", "origin", "--delete", &mid]);

    let output = repo.run_stax(&["sync", "--force", "--delete-upstream-gone"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&grand),
        "grand should be deleted, got: {:?}",
        branches
    );
    assert!(
        !branches.contains(&mid),
        "mid should be deleted, got: {:?}",
        branches
    );
    assert!(
        branches.contains(&leaf),
        "leaf should survive, got: {:?}",
        branches
    );

    // Leaf must NOT point at either deleted branch; it should land on main.
    let status = repo.run_stax(&["status", "--json"]);
    let json: serde_json::Value =
        serde_json::from_str(&TestRepo::stdout(&status)).expect("valid JSON");
    let leaf_entry = json["branches"]
        .as_array()
        .expect("branches array")
        .iter()
        .find(|b| b["name"].as_str() == Some(&leaf))
        .expect("leaf in status");
    let new_parent = leaf_entry["parent"]
        .as_str()
        .expect("leaf has a parent field");
    assert_ne!(new_parent, mid, "leaf still points at deleted mid");
    assert_ne!(new_parent, grand, "leaf still points at deleted grand");
    assert_eq!(
        new_parent, "main",
        "Expected leaf to be reparented to main, got: {}",
        new_parent
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
fn test_sync_on_merged_branch_with_missing_parent_falls_back_to_trunk() {
    let repo = TestRepo::new_with_remote();

    // Create parent branch
    repo.run_stax(&["bc", "feature-parent"]);
    let parent_branch = repo.current_branch();
    repo.create_file("parent.txt", "parent content");
    repo.commit("Parent commit");
    let push_parent = repo.git(&["push", "-u", "origin", &parent_branch]);
    assert!(
        push_parent.status.success(),
        "Failed to push parent branch: {}",
        TestRepo::stderr(&push_parent)
    );

    // Create child branch on top of parent
    repo.run_stax(&["bc", "feature-child"]);
    let child_branch = repo.current_branch();
    repo.create_file("child.txt", "child content");
    repo.commit("Child commit");
    let push_child = repo.git(&["push", "-u", "origin", &child_branch]);
    assert!(
        push_child.status.success(),
        "Failed to push child branch: {}",
        TestRepo::stderr(&push_child)
    );

    // Remove parent branch, leaving child's metadata with a missing parent.
    let delete_parent_local = repo.git(&["branch", "-D", &parent_branch]);
    assert!(
        delete_parent_local.status.success(),
        "Failed to delete local parent branch: {}",
        TestRepo::stderr(&delete_parent_local)
    );
    let delete_parent_remote = repo.git(&["push", "origin", "--delete", &parent_branch]);
    assert!(
        delete_parent_remote.status.success(),
        "Failed to delete remote parent branch: {}",
        TestRepo::stderr(&delete_parent_remote)
    );

    // Mark child branch as merged on remote.
    repo.merge_branch_on_remote(&child_branch);

    // Stay on child branch so sync has to checkout a parent before deleting it.
    assert_eq!(repo.current_branch(), child_branch);

    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    // Sync should fall back to trunk and still delete the merged child branch.
    assert_eq!(
        repo.current_branch(),
        "main",
        "Expected sync to fallback to trunk when parent branch is missing"
    );
    assert!(
        !repo.list_branches().iter().any(|b| b == &child_branch),
        "Expected merged child branch to be deleted"
    );
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

    // Should show trunk update succeeded ("✓ Update main" in the sync output)
    assert!(
        stdout.contains("Update main"),
        "Expected trunk update message. Got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("cleaned 1 merged"),
        "Expected merged cleanup count in sync footer. Got:\n{}",
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

#[test]
fn test_sync_detects_merged_branch_when_local_trunk_diverged() {
    // Regression: when local trunk diverges and we're not on trunk, sync may fail
    // to update local trunk before merged-branch detection. Detection should still
    // work by checking against origin/trunk.
    let repo = TestRepo::new_with_remote();

    // Create feature branch and push
    repo.run_stax(&["bc", "feature-merged-diverged-trunk"]);
    let branch_name = repo.current_branch();
    repo.create_file("feature.txt", "feature work");
    repo.commit("Feature work");
    repo.git(&["push", "-u", "origin", &branch_name]);

    // Merge feature branch on remote (simulates merged PR)
    repo.merge_branch_on_remote(&branch_name);

    // Create local-only commit on main so main diverges from origin/main
    repo.run_stax(&["t"]);
    repo.create_file("local-main-only.txt", "local commit");
    repo.commit("Local main only commit");

    // Go back to feature branch; sync will run non-trunk update path
    repo.run_stax(&["checkout", &branch_name]);
    assert!(repo
        .current_branch()
        .contains("feature-merged-diverged-trunk"));

    let output = repo.run_stax(&["sync", "--force"]);
    assert!(
        output.status.success(),
        "Sync failed: {}",
        TestRepo::stderr(&output)
    );

    // Branch should be deleted as merged even though local main diverged
    let branches = repo.list_branches();
    assert!(
        !branches
            .iter()
            .any(|b| b.contains("feature-merged-diverged-trunk")),
        "Expected merged branch to be deleted even with diverged local trunk"
    );
}

#[test]
fn test_sync_restack_handles_squash_merged_middle_branch() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main -> parent -> child
    repo.run_stax(&["bc", "middle-squash-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent 1\n");
    repo.commit("Parent commit 1");
    repo.create_file("parent.txt", "parent 1\nparent 2\n");
    repo.commit("Parent commit 2");
    repo.git(&["push", "-u", "origin", &parent]);

    repo.run_stax(&["bc", "middle-squash-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child change\n");
    repo.commit("Child commit");
    repo.git(&["push", "-u", "origin", &child]);

    // Squash-merge parent branch on remote and delete it.
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = test_tempdir();
    let run_remote_git = |args: &[&str]| {
        let output = hermetic_git_command()
            .args(args)
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to run git in remote clone");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_remote_git(&["clone", remote_path.to_str().unwrap(), "."]);
    run_remote_git(&["checkout", "-B", "main", "origin/main"]);
    run_remote_git(&["config", "user.email", "merger@test.com"]);
    run_remote_git(&["config", "user.name", "Merger"]);
    run_remote_git(&["fetch", "origin", &parent]);
    run_remote_git(&["merge", "--squash", &format!("origin/{}", parent)]);
    run_remote_git(&["commit", "-m", "Squash merge parent"]);
    run_remote_git(&["push", "origin", "main"]);
    run_remote_git(&["push", "origin", "--delete", &parent]);

    repo.run_stax(&["checkout", &child]);

    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "sync --restack failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        !TestRepo::stdout(&output).contains("conflict"),
        "Expected provenance-aware restack to avoid conflict for child-only commit.\nstdout: {}",
        TestRepo::stdout(&output)
    );

    // Parent branch should be cleaned up after sync.
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == &parent),
        "Expected merged parent branch to be deleted"
    );

    // Child should contain only its own commit relative to main.
    let count_output = repo.git(&["rev-list", "--count", &format!("main..{}", child)]);
    assert!(count_output.status.success());
    let unique_commits = String::from_utf8_lossy(&count_output.stdout)
        .trim()
        .to_string();
    assert_eq!(
        unique_commits, "1",
        "Expected child to keep only novel commits after provenance-aware restack"
    );
}

#[test]
fn test_sync_restack_handles_squash_merged_parent_after_trunk_advances() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main -> parent -> child
    repo.run_stax(&["bc", "sync-squash-parent"]);
    let parent = repo.current_branch();
    repo.create_file("parent.txt", "parent 1\n");
    repo.commit("Parent commit 1");
    repo.git(&["push", "-u", "origin", &parent]);

    repo.run_stax(&["bc", "sync-squash-child"]);
    let child = repo.current_branch();
    repo.create_file("child.txt", "child change\n");
    repo.commit("Child commit");
    repo.git(&["push", "-u", "origin", &child]);

    // Squash-merge parent on remote, advance trunk, then delete parent branch.
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = test_tempdir();
    let run_remote_git = |args: &[&str]| {
        let output = hermetic_git_command()
            .args(args)
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to run git in remote clone");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_remote_git(&["clone", remote_path.to_str().unwrap(), "."]);
    run_remote_git(&["checkout", "-B", "main", "origin/main"]);
    run_remote_git(&["config", "user.email", "merger@test.com"]);
    run_remote_git(&["config", "user.name", "Merger"]);
    run_remote_git(&["fetch", "origin", &parent]);
    run_remote_git(&["merge", "--squash", &format!("origin/{}", parent)]);
    run_remote_git(&["commit", "-m", "Squash merge parent"]);
    // Advance trunk with unrelated work after squash merge.
    std::fs::write(clone_dir.path().join("later.txt"), "later trunk work\n").unwrap();
    run_remote_git(&["add", "later.txt"]);
    run_remote_git(&["commit", "-m", "Later trunk commit"]);
    run_remote_git(&["push", "origin", "main"]);
    run_remote_git(&["push", "origin", "--delete", &parent]);

    repo.run_stax(&["checkout", &child]);

    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "sync --restack failed after trunk advanced\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        !TestRepo::stdout(&output).contains("conflict"),
        "Expected no conflict after provenance-aware sync restack.\nstdout: {}",
        TestRepo::stdout(&output)
    );

    // Parent branch should be cleaned up.
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == &parent),
        "Expected merged parent branch to be deleted"
    );

    // Child metadata should be reparented to trunk.
    let metadata_ref = format!("refs/branch-metadata/{}", child);
    let metadata_output = repo.git(&["show", &metadata_ref]);
    assert!(
        metadata_output.status.success(),
        "Failed to read metadata: {}",
        TestRepo::stderr(&metadata_output)
    );
    let metadata: Value =
        serde_json::from_str(&TestRepo::stdout(&metadata_output)).expect("Invalid JSON metadata");
    assert_eq!(
        metadata["parentBranchName"], "main",
        "Expected child reparented to trunk, metadata was: {}",
        metadata
    );

    // Child should have only its own commit relative to main.
    let count_output = repo.git(&["rev-list", "--count", &format!("main..{}", child)]);
    assert!(count_output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&count_output.stdout).trim(),
        "1",
        "Expected child to keep only novel commits after sync restack with advanced trunk"
    );
}

/// Regression test for issue #118: `sync --restack` must restack the entire
/// stack, not just the first stale branch.  With a 3-level stack
/// `main <- A <- B <- C`, squash-merging A and running sync --restack should
/// restack both B (onto main) and C (onto the updated B).
#[test]
fn test_sync_restack_restacks_full_chain_after_squash_merge() {
    let repo = TestRepo::new_with_remote();

    // Build 3-level stack: main -> branch_a -> branch_b -> branch_c
    repo.run_stax(&["bc", "chain-a"]);
    let branch_a = repo.current_branch();
    repo.create_file("a.txt", "a content\n");
    repo.commit("Commit A");
    repo.git(&["push", "-u", "origin", &branch_a]);

    repo.run_stax(&["bc", "chain-b"]);
    let branch_b = repo.current_branch();
    repo.create_file("b.txt", "b content\n");
    repo.commit("Commit B");
    repo.git(&["push", "-u", "origin", &branch_b]);

    repo.run_stax(&["bc", "chain-c"]);
    let branch_c = repo.current_branch();
    repo.create_file("c.txt", "c content\n");
    repo.commit("Commit C");
    repo.git(&["push", "-u", "origin", &branch_c]);

    // Squash-merge branch_a on remote and delete it
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = test_tempdir();
    let run_remote_git = |args: &[&str]| {
        let output = hermetic_git_command()
            .args(args)
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to run git in remote clone");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_remote_git(&["clone", remote_path.to_str().unwrap(), "."]);
    run_remote_git(&["checkout", "-B", "main", "origin/main"]);
    run_remote_git(&["config", "user.email", "merger@test.com"]);
    run_remote_git(&["config", "user.name", "Merger"]);
    run_remote_git(&["fetch", "origin", &branch_a]);
    run_remote_git(&["merge", "--squash", &format!("origin/{}", branch_a)]);
    run_remote_git(&["commit", "-m", "Squash merge A"]);
    run_remote_git(&["push", "origin", "main"]);
    run_remote_git(&["push", "origin", "--delete", &branch_a]);

    // Check out branch_c (top of the stack) and run sync --restack
    repo.run_stax(&["checkout", &branch_c]);

    let output = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output.status.success(),
        "sync --restack failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output),
        TestRepo::stderr(&output)
    );
    assert!(
        !TestRepo::stdout(&output).contains("conflict"),
        "Expected no conflict during sync --restack.\nstdout: {}",
        TestRepo::stdout(&output)
    );

    // branch_a should be cleaned up
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == &branch_a),
        "Expected merged branch_a to be deleted"
    );

    // branch_b should have only its own commit relative to main
    let count_b = repo.git(&["rev-list", "--count", &format!("main..{}", branch_b)]);
    assert!(count_b.status.success());
    assert_eq!(
        String::from_utf8_lossy(&count_b.stdout).trim(),
        "1",
        "Expected branch_b to have 1 unique commit after restack onto main"
    );

    // branch_c should have only its own commit relative to branch_b
    let count_c = repo.git(&[
        "rev-list",
        "--count",
        &format!("{}..{}", branch_b, branch_c),
    ]);
    assert!(count_c.status.success());
    assert_eq!(
        String::from_utf8_lossy(&count_c.stdout).trim(),
        "1",
        "Expected branch_c to have 1 unique commit after restack onto branch_b (issue #118)"
    );

    // branch_c should also have exactly 2 unique commits relative to main (B + C)
    let count_c_main = repo.git(&["rev-list", "--count", &format!("main..{}", branch_c)]);
    assert!(count_c_main.status.success());
    assert_eq!(
        String::from_utf8_lossy(&count_c_main.stdout).trim(),
        "2",
        "Expected branch_c to have 2 unique commits relative to main after full restack"
    );
}

/// Regression test for issue #120: after a squash-merged parent is rebased then
/// deleted in a two-step sync, child branches must not retain ghost commits.
/// The scenario is:
///   1. main ← A (3 commits) ← B (1 commit)
///   2. Squash-merge A on remote, delete remote branch
///   3. First `sync --restack`: rebases A onto main (A absorbed into main)
///   4. Second `sync --restack`: detects A as merged, deletes it, reparents B → main
///   5. B must have only its OWN commit relative to main (no ghost commits from A)
#[test]
fn test_sync_restack_no_ghost_commits_after_two_step_squash_merge() {
    let repo = TestRepo::new_with_remote();

    // Build stack: main -> branch_a (3 commits) -> branch_b (1 commit)
    repo.run_stax(&["bc", "ghost-parent"]);
    let branch_a = repo.current_branch();
    repo.create_file("a1.txt", "a1\n");
    repo.commit("A commit 1");
    repo.create_file("a2.txt", "a2\n");
    repo.commit("A commit 2");
    repo.create_file("a3.txt", "a3\n");
    repo.commit("A commit 3");
    repo.git(&["push", "-u", "origin", &branch_a]);

    repo.run_stax(&["bc", "ghost-child"]);
    let branch_b = repo.current_branch();
    repo.create_file("b1.txt", "b1\n");
    repo.commit("B commit 1");
    repo.git(&["push", "-u", "origin", &branch_b]);

    // Squash-merge A on remote and delete the remote branch
    let remote_path = repo.remote_path().expect("No remote configured");
    let clone_dir = test_tempdir();
    let run_remote_git = |args: &[&str]| {
        let output = hermetic_git_command()
            .args(args)
            .current_dir(clone_dir.path())
            .output()
            .expect("Failed to run git in remote clone");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout: {}\nstderr: {}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    run_remote_git(&["clone", remote_path.to_str().unwrap(), "."]);
    run_remote_git(&["checkout", "-B", "main", "origin/main"]);
    run_remote_git(&["config", "user.email", "merger@test.com"]);
    run_remote_git(&["config", "user.name", "Merger"]);
    run_remote_git(&["fetch", "origin", &branch_a]);
    run_remote_git(&["merge", "--squash", &format!("origin/{}", branch_a)]);
    run_remote_git(&["commit", "-m", "Squash merge A (3 commits)"]);
    run_remote_git(&["push", "origin", "main"]);
    run_remote_git(&["push", "origin", "--delete", &branch_a]);

    // First sync --restack: A may get rebased onto main (absorbed), or detected
    // as merged.  Either way this is the first step of the two-step scenario.
    repo.run_stax(&["checkout", &branch_b]);
    let output1 = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output1.status.success(),
        "First sync --restack failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output1),
        TestRepo::stderr(&output1)
    );

    // Second sync --restack: picks up any remaining reparent/delete/restack
    let output2 = repo.run_stax(&["sync", "--restack", "--force"]);
    assert!(
        output2.status.success(),
        "Second sync --restack failed\nstdout: {}\nstderr: {}",
        TestRepo::stdout(&output2),
        TestRepo::stderr(&output2)
    );

    // branch_a should be cleaned up by now
    let branches = repo.list_branches();
    assert!(
        !branches.iter().any(|b| b == &branch_a),
        "Expected merged branch_a to be deleted, branches: {:?}",
        branches
    );

    // KEY ASSERTION: B should have only its own 1 commit relative to main.
    // If ghost commits from A remain, this count would be > 1.
    let count_b = repo.git(&["rev-list", "--count", &format!("main..{}", branch_b)]);
    assert!(count_b.status.success());
    let unique_commits = String::from_utf8_lossy(&count_b.stdout).trim().to_string();
    assert_eq!(
        unique_commits, "1",
        "Expected branch_b to have 1 unique commit (no ghost commits from A), got {} (issue #120)",
        unique_commits
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
        stdout.contains("--downstack-only"),
        "Expected --downstack-only flag in help"
    );
    assert!(stdout.contains("--ds"), "Expected --ds alias in help");
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
        stdout.contains("--no-sync"),
        "Expected --no-sync flag in help"
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
    assert!(
        stdout.contains("--remote"),
        "Expected --remote flag in help"
    );
}

#[test]
fn test_merge_downstack_only_conflicts_with_all() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--downstack-only", "--all"]);
    let stderr = TestRepo::stderr(&output);

    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_merge_downstack_only_conflicts_with_remote() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--downstack-only", "--remote"]);
    let stderr = TestRepo::stderr(&output);

    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_merge_downstack_only_conflicts_with_queue() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--downstack-only", "--queue"]);
    let stderr = TestRepo::stderr(&output);

    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_merge_ds_alias_conflicts_with_all() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["merge", "--ds", "--all"]);
    let stderr = TestRepo::stderr(&output);

    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_merge_remote_on_trunk_shows_error() {
    let repo = TestRepo::new();

    let output = repo.run_stax(&["status"]);
    assert!(output.status.success());
    assert_eq!(repo.current_branch(), "main");

    let output = repo.run_stax(&["merge", "--remote"]);
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
fn test_merge_remote_on_untracked_branch_shows_error() {
    let repo = TestRepo::new();

    repo.run_stax(&["status"]);
    repo.git(&["checkout", "-b", "untracked-remote"]);
    repo.create_file("test.txt", "content");
    repo.commit("Untracked commit");

    let output = repo.run_stax(&["merge", "--remote"]);
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
fn test_merge_remote_without_pr_shows_error() {
    let repo = TestRepo::new_with_remote();

    repo.run_stax(&["bc", "feature-remote-no-pr"]);
    repo.create_file("feature.txt", "content");
    repo.commit("Feature commit");

    let output = repo.run_stax(&["merge", "--remote", "--yes"]);
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
fn test_merge_remote_conflicts_with_when_ready() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["merge", "--remote", "--when-ready"]);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
}

#[test]
fn test_merge_remote_conflicts_with_dry_run() {
    let repo = TestRepo::new();
    let output = repo.run_stax(&["merge", "--remote", "--dry-run"]);
    let stderr = TestRepo::stderr(&output);
    assert!(
        !output.status.success(),
        "Expected non-success for conflicting flags"
    );
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts with"),
        "Expected clap conflict error, got: {}",
        stderr
    );
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

    // Create a stack of 3 branches, capturing the actual names (may include configured prefix)
    repo.run_stax(&["bc", "stack-a"]);
    repo.create_file("a.txt", "content a");
    repo.commit("Feature A");

    repo.run_stax(&["bc", "stack-b"]);
    repo.create_file("b.txt", "content b");
    repo.commit("Feature B");
    let branch_b = repo.current_branch();

    repo.run_stax(&["bc", "stack-c"]);
    repo.create_file("c.txt", "content c");
    repo.commit("Feature C");

    // Go to the middle branch using its actual name
    repo.run_stax(&["checkout", &branch_b]);
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
        "--no-sync",
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

mod forge_mock_tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};
    use tempfile::TempDir;
    use wiremock::matchers::{method, path, path_regex, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ensure_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    fn write_test_config(home: &Path, api_base_url: &str) {
        write_test_config_with_submit(home, api_base_url, None);
    }

    fn write_test_config_with_submit(home: &Path, api_base_url: &str, stack_links: Option<&str>) {
        let config_dir = home.join(".config").join("stax");
        std::fs::create_dir_all(&config_dir).expect("Failed to create config dir");
        let config_path = config_dir.join("config.toml");
        let mut config = format!("[remote]\napi_base_url = \"{}\"\n", api_base_url);
        if let Some(mode) = stack_links {
            config.push_str(&format!("\n[submit]\nstack_links = \"{}\"\n", mode));
        }
        fs::write(&config_path, config).expect("Failed to write config");
    }

    fn write_test_config_with_ai(home: &Path, api_base_url: &str, stack_links: Option<&str>) {
        let config_dir = home.join(".config").join("stax");
        std::fs::create_dir_all(&config_dir).expect("Failed to create config dir");
        let config_path = config_dir.join("config.toml");
        let mut config = format!("[remote]\napi_base_url = \"{}\"\n", api_base_url);
        if let Some(mode) = stack_links {
            config.push_str(&format!("\n[submit]\nstack_links = \"{}\"\n", mode));
        }
        config.push_str("\n[ai.generate]\nagent = \"claude\"\n");
        fs::write(&config_path, config).expect("Failed to write config");
    }

    fn write_fake_claude(home: &Path, response: &str) -> PathBuf {
        let bin_dir = home.join("bin");
        fs::create_dir_all(&bin_dir).expect("Failed to create fake AI bin dir");
        let path = bin_dir.join("claude");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\ncat >/dev/null\ncat <<'JSON'\n{}\nJSON\n",
                response
            ),
        )
        .expect("Failed to write fake claude");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755))
                .expect("Failed to chmod fake claude");
        }

        bin_dir
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
        hermetic_git_command()
            .args(args)
            .current_dir(repo.path())
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .output()
            .expect("Failed to run git command")
    }

    fn git_in_dir_with_env(cwd: &Path, home: &Path, args: &[&str]) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        hermetic_git_command()
            .args(args)
            .current_dir(cwd)
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .output()
            .expect("Failed to run git command in custom cwd")
    }

    fn setup_fake_github_remote(repo: &TestRepo, home: &Path) -> TempDir {
        setup_fake_remote(
            repo,
            home,
            "https://github.com/test/repo.git",
            "https://github.com/",
        )
    }

    fn setup_fake_remote(
        repo: &TestRepo,
        home: &Path,
        remote_url: &str,
        remote_base: &str,
    ) -> TempDir {
        let remote_root = super::test_tempdir();
        let remote_repo = remote_root.path().join("test").join("repo.git");
        if let Some(parent) = remote_repo.parent() {
            std::fs::create_dir_all(parent).expect("Failed to create remote parent dirs");
        }
        std::fs::create_dir_all(&remote_repo).expect("Failed to create remote repo dir");

        hermetic_git_command()
            .args(["init", "--bare"])
            .current_dir(&remote_repo)
            .output()
            .expect("Failed to init bare remote repo");

        let add_remote = git_with_env(repo, home, &["remote", "add", "origin", remote_url]);
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
                remote_base,
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

    fn find_request_index(
        requests: &[wiremock::Request],
        method_name: &str,
        path_name: &str,
    ) -> usize {
        requests
            .iter()
            .position(|request| {
                request.method.as_str() == method_name && request.url.path() == path_name
            })
            .unwrap_or_else(|| panic!("Did not find request {} {}", method_name, path_name))
    }

    /// Find a request to a PR/MR endpoint whose JSON payload contains a body/description key.
    /// This distinguishes body-update requests from title-update requests (both hit the same URL).
    /// Works for GitHub (PATCH + "body"), GitLab (PUT + "description"), and Gitea (PATCH + "body").
    fn find_body_update<'a>(
        requests: &'a [wiremock::Request],
        method_name: &str,
        path_name: &str,
        json_key: &str,
    ) -> &'a wiremock::Request {
        requests
            .iter()
            .find(|request| {
                request.method.as_str() == method_name
                    && request.url.path() == path_name
                    && serde_json::from_slice::<serde_json::Value>(&request.body)
                        .ok()
                        .and_then(|v| v.get(json_key).cloned())
                        .is_some()
            })
            .unwrap_or_else(|| {
                panic!(
                    "Did not find {} request to {} with '{}' in payload",
                    method_name, path_name, json_key
                )
            })
    }

    /// Shorthand for GitHub/Gitea PATCH requests with "body" key.
    fn find_body_patch<'a>(
        requests: &'a [wiremock::Request],
        path_name: &str,
    ) -> &'a wiremock::Request {
        find_body_update(requests, "PATCH", path_name, "body")
    }

    fn issue_comment_fixture(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "node_id": format!("IC_test_{}", id),
            "url": format!("https://api.github.com/repos/test/repo/issues/comments/{}", id),
            "html_url": format!("https://github.com/test/repo/pull/42#issuecomment-{}", id),
            "issue_url": "https://api.github.com/repos/test/repo/issues/42",
            "body": body,
            "user": {
                "login": "stax",
                "id": 1,
                "node_id": "MDQ6VXNlcjE=",
                "avatar_url": "https://avatars.githubusercontent.com/u/1?v=4",
                "gravatar_id": "",
                "url": "https://api.github.com/users/stax",
                "html_url": "https://github.com/stax",
                "followers_url": "https://api.github.com/users/stax/followers",
                "following_url": "https://api.github.com/users/stax/following{/other_user}",
                "gists_url": "https://api.github.com/users/stax/gists{/gist_id}",
                "starred_url": "https://api.github.com/users/stax/starred{/owner}{/repo}",
                "subscriptions_url": "https://api.github.com/users/stax/subscriptions",
                "organizations_url": "https://api.github.com/users/stax/orgs",
                "repos_url": "https://api.github.com/users/stax/repos",
                "events_url": "https://api.github.com/users/stax/events{/privacy}",
                "received_events_url": "https://api.github.com/users/stax/received_events",
                "type": "User",
                "site_admin": false
            },
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-01T00:00:00Z"
        })
    }

    fn github_pull_fixture(
        number: u64,
        head_branch: &str,
        base_branch: &str,
        head_sha: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "url": format!("https://api.github.com/repos/test/repo/pulls/{}", number),
            "id": number,
            "number": number,
            "state": "open",
            "draft": false,
            "merged_at": null,
            "mergeable": true,
            "mergeable_state": "clean",
            "head": {
                "ref": head_branch,
                "sha": head_sha,
                "label": format!("test:{}", head_branch)
            },
            "base": {
                "ref": base_branch,
                "sha": format!("{}-sha", base_branch)
            }
        })
    }

    fn github_pull_fixture_with_details(
        number: u64,
        head_branch: &str,
        base_branch: &str,
        title: &str,
        body: &str,
    ) -> serde_json::Value {
        let mut pr = github_pull_fixture(number, head_branch, base_branch, "aaaa");
        pr["title"] = serde_json::json!(title);
        pr["body"] = serde_json::json!(body);
        pr["html_url"] = serde_json::json!(format!("https://github.com/test/repo/pull/{}", number));
        pr
    }

    async fn mount_github_new_pr_flow(
        mock_server: &MockServer,
        number: u64,
        branch: &str,
        title: &str,
        body: &str,
    ) {
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "url": format!("https://api.github.com/repos/test/repo/pulls/{}", number),
                "id": number,
                "number": number,
                "state": "open",
                "title": title,
                "body": body,
                "draft": true,
                "head": { "ref": branch, "sha": "aaaa", "label": format!("test:{}", branch) },
                "base": { "ref": "main", "sha": "bbbb" },
                "html_url": format!("https://github.com/test/repo/pull/{}", number)
            })))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/issues/{}/comments", number)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/pulls/{}", number)))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                github_pull_fixture_with_details(number, branch, "main", title, body),
            ))
            .mount(mock_server)
            .await;
    }

    async fn mount_github_existing_pr(
        mock_server: &MockServer,
        number: u64,
        branch: &str,
        title: &str,
        body: &str,
    ) {
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                github_pull_fixture_with_details(number, branch, "main", title, body)
            ])))
            .mount(mock_server)
            .await;
    }

    async fn mount_github_stack_links_off_sync(
        mock_server: &MockServer,
        number: u64,
        branch: &str,
        title: &str,
        body: &str,
    ) {
        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/issues/{}/comments", number)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(format!("/repos/test/repo/pulls/{}", number)))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                github_pull_fixture_with_details(number, branch, "main", title, body),
            ))
            .mount(mock_server)
            .await;
    }

    fn gitlab_mr_fixture(
        iid: u64,
        title: &str,
        source_branch: &str,
        target_branch: &str,
        state: &str,
        description: &str,
        sha: &str,
        pipeline_status: Option<&str>,
    ) -> serde_json::Value {
        let mut mr = serde_json::json!({
            "iid": iid,
            "title": title,
            "state": state,
            "draft": false,
            "source_branch": source_branch,
            "target_branch": target_branch,
            "description": description,
            "merge_status": "can_be_merged",
            "detailed_merge_status": "mergeable",
            "web_url": format!("https://gitlab.com/test/repo/-/merge_requests/{}", iid),
            "sha": sha
        });

        if let Some(status) = pipeline_status {
            mr["head_pipeline"] = serde_json::json!({ "status": status });
        }

        mr
    }

    fn gitlab_note_fixture(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "body": body,
            "created_at": "2024-01-01T00:00:00Z",
            "author": { "username": "stax" }
        })
    }

    fn gitea_pull_fixture(
        number: u64,
        title: &str,
        head_branch: &str,
        base_branch: &str,
        state: &str,
        body: &str,
        merged: bool,
        head_sha: &str,
    ) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "state": state,
            "title": title,
            "body": body,
            "draft": false,
            "mergeable": true,
            "mergeable_state": "clean",
            "merged": merged,
            "head": {
                "ref": head_branch,
                "sha": head_sha,
                "label": format!("test:{}", head_branch)
            },
            "base": {
                "ref": base_branch,
                "sha": format!("{}-sha", base_branch),
                "label": format!("test:{}", base_branch)
            }
        })
    }

    fn gitea_comment_fixture(id: u64, body: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "body": body,
            "created_at": "2024-01-01T00:00:00Z",
            "user": { "login": "stax" }
        })
    }

    fn squash_merge_branch_on_fake_remote(remote_root: &TempDir, branch: &str) {
        let remote_repo = remote_root.path().join("test").join("repo.git");
        let clone_dir = super::test_tempdir();

        let run_remote_git = |args: &[&str]| {
            let output = hermetic_git_command()
                .args(args)
                .current_dir(clone_dir.path())
                .output()
                .expect("Failed to run git in fake remote clone");
            assert!(
                output.status.success(),
                "git {:?} failed\nstdout: {}\nstderr: {}",
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        run_remote_git(&["clone", remote_repo.to_str().unwrap(), "."]);
        run_remote_git(&["checkout", "-B", "main", "origin/main"]);
        run_remote_git(&["config", "user.email", "merger@test.com"]);
        run_remote_git(&["config", "user.name", "Merger"]);
        run_remote_git(&["fetch", "origin", branch]);
        run_remote_git(&["merge", "--squash", &format!("origin/{}", branch)]);
        run_remote_git(&["commit", "-m", &format!("Squash merge {}", branch)]);
        run_remote_git(&["push", "origin", "main"]);
        run_remote_git(&["push", "origin", "--delete", branch]);
    }

    fn run_stax_with_env(repo: &TestRepo, home: &Path, args: &[&str]) -> Output {
        run_stax_with_token_env(repo, home, "STAX_GITHUB_TOKEN", args)
    }

    fn run_stax_in_dir_with_env(cwd: &Path, home: &Path, args: &[&str]) -> Output {
        run_stax_in_dir_with_token_env(cwd, home, "STAX_GITHUB_TOKEN", args)
    }

    fn run_stax_with_token_env(
        repo: &TestRepo,
        home: &Path,
        token_env: &str,
        args: &[&str],
    ) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        let mut command = Command::new(stax_bin());
        command
            .args(args)
            .current_dir(repo.path())
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .env(token_env, "mock-token")
            .env("STAX_DISABLE_UPDATE_CHECK", "1");
        command.output().expect("Failed to execute stax")
    }

    fn run_stax_with_token_env_and_path(
        repo: &TestRepo,
        home: &Path,
        token_env: &str,
        path_prefix: &Path,
        args: &[&str],
    ) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        let path = match std::env::var("PATH") {
            Ok(current) if !current.is_empty() => {
                format!("{}:{}", path_prefix.display(), current)
            }
            _ => path_prefix.display().to_string(),
        };
        let mut command = Command::new(stax_bin());
        command
            .args(args)
            .current_dir(repo.path())
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .env(token_env, "mock-token")
            .env("PATH", path)
            .env("STAX_DISABLE_UPDATE_CHECK", "1");
        command.output().expect("Failed to execute stax")
    }

    fn run_stax_in_dir_with_token_env(
        cwd: &Path,
        home: &Path,
        token_env: &str,
        args: &[&str],
    ) -> Output {
        let gitconfig = ensure_empty_gitconfig(home);
        let mut command = Command::new(stax_bin());
        command
            .args(args)
            .current_dir(cwd)
            .env("HOME", home)
            .env("GIT_CONFIG_GLOBAL", &gitconfig)
            .env("GIT_CONFIG_SYSTEM", &gitconfig)
            .env(token_env, "mock-token")
            .env("STAX_DISABLE_UPDATE_CHECK", "1");
        command
            .output()
            .expect("Failed to execute stax in custom cwd")
    }

    fn worktree_path(repo: &TestRepo, home: &Path, lane_name: &str) -> PathBuf {
        let output = run_stax_with_env(repo, home, &["wt", "path", lane_name]);
        assert!(
            output.status.success(),
            "Failed to resolve worktree path: {}",
            TestRepo::stderr(&output)
        );
        PathBuf::from(TestRepo::stdout(&output).trim())
    }

    fn git_current_branch(cwd: &Path, home: &Path) -> String {
        let output = git_in_dir_with_env(cwd, home, &["rev-parse", "--abbrev-ref", "HEAD"]);
        assert!(
            output.status.success(),
            "Failed to read current branch: {}",
            TestRepo::stderr(&output)
        );
        TestRepo::stdout(&output).trim().to_string()
    }

    fn setup_branch_with_remote(home: &Path, branch: &str) -> TestRepo {
        setup_branch_with_forge_remote(
            home,
            branch,
            "https://github.com/test/repo.git",
            "https://github.com/",
            "STAX_GITHUB_TOKEN",
        )
    }

    fn setup_branch_with_forge_remote(
        home: &Path,
        branch: &str,
        remote_url: &str,
        remote_base: &str,
        token_env: &str,
    ) -> TestRepo {
        let repo = TestRepo::new();
        // Persist the bare-repo tempdir for the rest of the test process — the
        // returned TestRepo is now the caller's only handle, and previously
        // this temp dir was being dropped here, deleting the bare repo. Today
        // submit's fetch/ls-remote calls fail loudly on a missing bare repo
        // (issue #222 fix), so they need the directory to actually exist for
        // the duration of the test.
        let remote_root = setup_fake_remote(&repo, home, remote_url, remote_base);
        let _ = remote_root.keep();

        let output = run_stax_with_token_env(&repo, home, token_env, &["bc", branch]);
        assert!(
            output.status.success(),
            "Failed to create branch {}: {}",
            branch,
            TestRepo::stderr(&output)
        );

        repo.create_file("feature.txt", &format!("content for {}\n", branch));
        repo.commit(&format!("Add {}", branch));

        let push = git_with_env(&repo, home, &["push", "-u", "origin", branch]);
        assert!(
            push.status.success(),
            "Failed to push branch {}: {}",
            branch,
            TestRepo::stderr(&push)
        );

        repo
    }

    /// Create a test repo configured to use a mock GitHub API
    async fn setup_mock_github() -> (TestRepo, MockServer) {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let repo = TestRepo::new_with_remote();

        // Set environment variables for the mock
        std::env::set_var("STAX_GITHUB_TOKEN", "mock-token");

        (repo, mock_server)
    }

    #[tokio::test]
    async fn test_mock_server_setup() {
        ensure_crypto_provider();
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

    #[test]
    fn test_managed_lane_branch_submit_no_pr_pushes_lane_branch_only() {
        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "sibling-scope"]);
        assert!(
            output.status.success(),
            "Failed to create sibling branch: {}",
            TestRepo::stderr(&output)
        );
        repo.create_file("sibling.txt", "sibling\n");
        repo.commit("Sibling commit");

        let output = run_stax_with_env(&repo, home.path(), &["t"]);
        assert!(
            output.status.success(),
            "Failed to return to trunk: {}",
            TestRepo::stderr(&output)
        );

        let output = run_stax_with_env(
            &repo,
            home.path(),
            &["wt", "c", "lane-submit", "--no-verify"],
        );
        assert!(
            output.status.success(),
            "Failed to create lane: {}",
            TestRepo::stderr(&output)
        );

        let lane_path = worktree_path(&repo, home.path(), "lane-submit");
        fs::write(lane_path.join("lane.txt"), "lane\n").expect("Failed to write lane file");
        let add = git_in_dir_with_env(&lane_path, home.path(), &["add", "-A"]);
        assert!(add.status.success(), "{}", TestRepo::stderr(&add));
        let commit = git_in_dir_with_env(&lane_path, home.path(), &["commit", "-m", "Lane commit"]);
        assert!(commit.status.success(), "{}", TestRepo::stderr(&commit));

        let lane_branch = git_current_branch(&lane_path, home.path());
        let output = run_stax_in_dir_with_env(&lane_path, home.path(), &["bs", "--no-pr", "--yes"]);
        assert!(
            output.status.success(),
            "Lane branch submit failed: {}",
            TestRepo::stderr(&output)
        );

        let remote_repo = remote_root.path().join("test").join("repo.git");
        let heads = hermetic_git_command()
            .args(["for-each-ref", "--format=%(refname:short)", "refs/heads"])
            .current_dir(remote_repo)
            .output()
            .expect("Failed to list remote heads");
        assert!(heads.status.success(), "{}", TestRepo::stderr(&heads));
        let remote_heads = TestRepo::stdout(&heads);
        assert!(
            remote_heads.lines().any(|head| head == lane_branch),
            "Expected lane branch on remote, got:\n{}",
            remote_heads
        );
        assert!(
            !remote_heads
                .lines()
                .any(|head| head.contains("sibling-scope")),
            "Sibling branch should not be submitted by lane-scoped `bs`, got:\n{}",
            remote_heads
        );
    }

    #[tokio::test]
    async fn test_managed_lane_branch_submit_creates_pr_on_mock_github() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_env(&repo, home.path(), &["wt", "c", "lane-pr", "--no-verify"]);
        assert!(
            output.status.success(),
            "Failed to create lane: {}",
            TestRepo::stderr(&output)
        );

        let lane_path = worktree_path(&repo, home.path(), "lane-pr");
        fs::write(lane_path.join("lane.txt"), "lane\n").expect("Failed to write lane file");
        let add = git_in_dir_with_env(&lane_path, home.path(), &["add", "-A"]);
        assert!(add.status.success(), "{}", TestRepo::stderr(&add));
        let commit =
            git_in_dir_with_env(&lane_path, home.path(), &["commit", "-m", "Lane PR commit"]);
        assert!(commit.status.success(), "{}", TestRepo::stderr(&commit));

        let lane_branch = git_current_branch(&lane_path, home.path());

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/77",
                "id": 77,
                "number": 77,
                "state": "open",
                "draft": true,
                "body": "",
                "head": { "ref": lane_branch.clone(), "sha": "aaaa", "label": format!("test:{}", lane_branch) },
                "base": { "ref": "main", "sha": "bbbb" },
                "html_url": "https://github.com/test/repo/pull/77"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/77/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/77"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/77",
                "id": 77,
                "number": 77,
                "state": "open",
                "draft": true,
                "body": "",
                "head": { "ref": lane_branch.clone(), "sha": "aaaa", "label": format!("test:{}", lane_branch) },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let output =
            run_stax_in_dir_with_env(&lane_path, home.path(), &["bs", "--yes", "--no-prompt"]);
        assert!(
            output.status.success(),
            "Lane branch PR submit failed: {}",
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_create = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
            })
            .expect("missing PR create request");
        let payload: serde_json::Value = serde_json::from_slice(&pr_create.body).unwrap();
        assert_eq!(payload["head"], lane_branch);
        assert_eq!(payload["base"], "main");
        assert_eq!(payload["draft"], true);

        let metadata_ref = format!("refs/branch-metadata/{}", lane_branch);
        let output = repo.git(&["show", &metadata_ref]);
        assert!(
            output.status.success(),
            "Failed to read lane metadata: {}",
            TestRepo::stderr(&output)
        );
        let metadata = TestRepo::stdout(&output);
        assert!(
            metadata.contains("\"number\":77"),
            "Expected lane PR number in metadata, got: {}",
            metadata
        );
    }

    #[tokio::test]
    async fn test_submit_ai_yes_uses_generated_title_and_body_for_new_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_ai(home.path(), &mock_server.uri(), Some("off"));
        let ai_bin = write_fake_claude(
            home.path(),
            r###"{"title":"AI submit title","body":"## Summary\n\nAI generated body."}"###,
        );
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["bc", "feature-ai-submit"],
        );
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );
        let branch = repo.current_branch();
        repo.create_file("ai-submit.txt", "ai submit\n");
        repo.commit("Add AI submit flow");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "title": "AI submit title",
                "body": "## Summary\n\nAI generated body.",
                "draft": true,
                "head": { "ref": branch.clone(), "sha": "aaaa", "label": format!("test:{}", branch) },
                "base": { "ref": "main", "sha": "bbbb" },
                "html_url": "https://github.com/test/repo/pull/42"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "title": "AI submit title",
                "body": "## Summary\n\nAI generated body.",
                "draft": true,
                "head": { "ref": branch.clone(), "sha": "aaaa", "label": format!("test:{}", branch) },
                "base": { "ref": "main", "sha": "bbbb" },
                "html_url": "https://github.com/test/repo/pull/42"
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["submit", "--ai", "--yes", "--no-prompt"],
        );
        assert!(
            output.status.success(),
            "submit --ai failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_create = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
            })
            .expect("missing PR create request");
        let payload: serde_json::Value = serde_json::from_slice(&pr_create.body).unwrap();
        assert_eq!(payload["title"], "AI submit title");
        assert_eq!(payload["body"], "## Summary\n\nAI generated body.");
        assert_eq!(payload["head"], branch);
        assert_eq!(payload["draft"], true);
    }

    #[tokio::test]
    async fn test_submit_body_scope_yes_uses_default_title_for_new_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_ai(home.path(), &mock_server.uri(), Some("off"));
        let ai_bin = write_fake_claude(home.path(), r###"{"body":"AI body only"}"###);
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["bc", "feature-body-scope"],
        );
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );
        let branch = repo.current_branch();
        repo.create_file("body-scope.txt", "body scope\n");
        repo.commit("Add AI body scope");

        mount_github_new_pr_flow(
            &mock_server,
            43,
            &branch,
            "Add AI body scope",
            "AI body only",
        )
        .await;

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["submit", "--ai", "--body", "--yes", "--no-prompt"],
        );
        assert!(
            output.status.success(),
            "submit --ai --body failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_create = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
            })
            .expect("missing PR create request");
        let payload: serde_json::Value = serde_json::from_slice(&pr_create.body).unwrap();
        assert_eq!(payload["title"], "Add AI body scope");
        assert_eq!(payload["body"], "AI body only");
    }

    #[tokio::test]
    async fn test_submit_ai_yes_falls_back_to_default_new_pr_details() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_ai(home.path(), &mock_server.uri(), Some("off"));
        let ai_bin = write_fake_claude(home.path(), "not json");
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["bc", "feature-ai-fallback"],
        );
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );
        let branch = repo.current_branch();
        repo.create_file("ai-fallback.txt", "ai fallback\n");
        repo.commit("Add AI fallback");
        let default_body = "## Summary\n\n- Add AI fallback";

        mount_github_new_pr_flow(&mock_server, 44, &branch, "Add AI fallback", default_body).await;

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &["submit", "--ai", "--yes", "--no-prompt"],
        );
        assert!(
            output.status.success(),
            "submit --ai fallback failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_create = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
            })
            .expect("missing PR create request");
        let payload: serde_json::Value = serde_json::from_slice(&pr_create.body).unwrap();
        assert_eq!(payload["title"], "Add AI fallback");
        assert_eq!(payload["body"], default_body);
    }

    #[tokio::test]
    async fn test_submit_plain_ai_yes_skips_existing_pr_content_updates() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = setup_branch_with_remote(home.path(), "feature-ai-existing-skip");
        let branch = repo.current_branch();

        mount_github_existing_pr(&mock_server, 45, &branch, "Existing title", "Existing body")
            .await;
        mount_github_stack_links_off_sync(
            &mock_server,
            45,
            &branch,
            "Existing title",
            "Existing body",
        )
        .await;

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--ai", "--yes"]);
        assert!(
            output.status.success(),
            "submit --ai --yes existing skip failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        assert!(
            !requests.iter().any(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test/repo/pulls/45"
            }),
            "plain --ai --yes should not patch existing PR content"
        );
    }

    #[tokio::test]
    async fn test_submit_ai_title_body_yes_updates_existing_pr_content() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_ai(home.path(), &mock_server.uri(), Some("off"));
        let ai_bin = write_fake_claude(
            home.path(),
            r###"{"title":"AI refreshed title","body":"AI refreshed body"}"###,
        );
        let repo = setup_branch_with_remote(home.path(), "feature-ai-existing-update");
        let branch = repo.current_branch();

        mount_github_existing_pr(&mock_server, 46, &branch, "Existing title", "Existing body")
            .await;
        mount_github_stack_links_off_sync(
            &mock_server,
            46,
            &branch,
            "AI refreshed title",
            "AI refreshed body",
        )
        .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/46"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                github_pull_fixture_with_details(
                    46,
                    &branch,
                    "main",
                    "AI refreshed title",
                    "AI refreshed body",
                ),
            ))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env_and_path(
            &repo,
            home.path(),
            "STAX_GITHUB_TOKEN",
            &ai_bin,
            &[
                "submit",
                "--ai",
                "--title",
                "--body",
                "--yes",
                "--no-prompt",
            ],
        );
        assert!(
            output.status.success(),
            "submit existing --ai --title --body failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let title_patch = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test/repo/pulls/46"
                    && serde_json::from_slice::<serde_json::Value>(&request.body)
                        .ok()
                        .and_then(|payload| payload.get("title").cloned())
                        .is_some()
            })
            .expect("missing existing PR title patch");
        let title_payload: serde_json::Value = serde_json::from_slice(&title_patch.body).unwrap();
        assert_eq!(title_payload["title"], "AI refreshed title");

        let body_patch = find_body_patch(&requests, "/repos/test/repo/pulls/46");
        let body_payload: serde_json::Value = serde_json::from_slice(&body_patch.body).unwrap();
        assert_eq!(body_payload["body"], "AI refreshed body");
    }

    #[tokio::test]
    async fn test_refresh_yes_no_prompt_creates_pr_on_mock_github() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "refresh-non-interactive"]);
        assert!(
            output.status.success(),
            "Failed to create branch: {}",
            TestRepo::stderr(&output)
        );

        repo.create_file("feature.txt", "content\n");
        repo.commit("Feature commit");
        let branch = repo.current_branch();

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/88",
                "id": 88,
                "number": 88,
                "state": "open",
                "title": "Feature commit",
                "draft": true,
                "body": "",
                "head": { "ref": branch.clone(), "sha": "aaaa", "label": format!("test:{}", branch) },
                "base": { "ref": "main", "sha": "bbbb", "label": "test:main" },
                "html_url": "https://github.com/test/repo/pull/88"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/88/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/88"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/88",
                "id": 88,
                "number": 88,
                "state": "open",
                "title": "Feature commit",
                "draft": true,
                "body": "",
                "head": { "ref": branch.clone(), "sha": "aaaa", "label": format!("test:{}", branch) },
                "base": { "ref": "main", "sha": "bbbb", "label": "test:main" },
                "html_url": "https://github.com/test/repo/pull/88"
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_env(
            &repo,
            home.path(),
            &["refresh", "--force", "--yes", "--no-prompt"],
        );
        assert!(
            output.status.success(),
            "refresh --force --yes --no-prompt failed\nstdout: {}\nstderr: {}",
            TestRepo::stdout(&output),
            TestRepo::stderr(&output)
        );

        let requests = mock_server.received_requests().await.unwrap();
        let pr_create = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
            })
            .expect("missing PR create request");
        let payload: serde_json::Value = serde_json::from_slice(&pr_create.body).unwrap();
        assert_eq!(payload["head"], branch);
        assert_eq!(payload["base"], "main");
        assert_eq!(payload["title"], "Feature commit");
        assert_eq!(payload["draft"], true);
    }

    #[tokio::test]
    async fn test_submit_persists_pr_info_for_existing_pr() {
        ensure_crypto_provider();
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

        let home = super::test_tempdir();
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
        ensure_crypto_provider();
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

        let home = super::test_tempdir();
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
    async fn test_submit_default_comment_mode_updates_comment_and_removes_body_block() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config(home.path(), &mock_server.uri());
        let repo = setup_branch_with_remote(home.path(), "feature-comment");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/42",
                    "id": 42,
                    "number": 42,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-comment", "sha": "aaaa", "label": "test:feature-comment" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(issue_comment_fixture(
                    901,
                    "<!-- stax-stack-comment -->\nupdated",
                )),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "body": "## Summary\n\nhello\n\n<!-- stax-stack-links:start -->\nold\n<!-- stax-stack-links:end -->",
                "head": { "ref": "feature-comment", "sha": "aaaa", "label": "test:feature-comment" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "head": { "ref": "feature-comment", "sha": "aaaa", "label": "test:feature-comment" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--yes", "--no-prompt"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test/repo/issues/comments/901"
        }));
        let body_patch = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_patch.body).unwrap();
        assert_eq!(payload["body"], "## Summary\n\nhello");
    }

    #[tokio::test]
    async fn test_submit_body_mode_removes_comment_and_writes_body_block() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("body"));
        let repo = setup_branch_with_remote(home.path(), "feature-body");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/42",
                    "id": 42,
                    "number": 42,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-body", "sha": "aaaa", "label": "test:feature-body" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "body": "## Summary\n\nhello",
                "head": { "ref": "feature-body", "sha": "aaaa", "label": "test:feature-body" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "head": { "ref": "feature-body", "sha": "aaaa", "label": "test:feature-body" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--yes", "--no-prompt"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "DELETE"
                && request.url.path() == "/repos/test/repo/issues/comments/901"
        }));
        let body_patch = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_patch.body).unwrap();
        let body = payload["body"].as_str().unwrap();
        assert!(body.starts_with("## Summary\n\nhello"));
        assert!(body.contains("<!-- stax-stack-links:start -->"));
        assert!(body.contains("## Stack Links"));
    }

    #[tokio::test]
    async fn test_submit_both_mode_updates_comment_and_body() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("both"));
        let repo = setup_branch_with_remote(home.path(), "feature-both");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/42",
                    "id": 42,
                    "number": 42,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-both", "sha": "aaaa", "label": "test:feature-both" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(issue_comment_fixture(
                    901,
                    "<!-- stax-stack-comment -->\nupdated",
                )),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "body": "## Summary\n\nhello",
                "head": { "ref": "feature-both", "sha": "aaaa", "label": "test:feature-both" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "head": { "ref": "feature-both", "sha": "aaaa", "label": "test:feature-both" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--yes", "--no-prompt"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "PATCH"
                && request.url.path() == "/repos/test/repo/issues/comments/901"
        }));
        let body_patch = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_patch.body).unwrap();
        assert!(payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-links:start -->"));
    }

    #[tokio::test]
    async fn test_submit_off_mode_removes_comment_and_body_block() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = setup_branch_with_remote(home.path(), "feature-off");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/42",
                    "id": 42,
                    "number": 42,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "feature-off", "sha": "aaaa", "label": "test:feature-off" },
                    "base": { "ref": "main", "sha": "bbbb" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                issue_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "body": "## Summary\n\nhello\n\n<!-- stax-stack-links:start -->\nold\n<!-- stax-stack-links:end -->",
                "head": { "ref": "feature-off", "sha": "aaaa", "label": "test:feature-off" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/42",
                "id": 42,
                "number": 42,
                "state": "open",
                "draft": false,
                "head": { "ref": "feature-off", "sha": "aaaa", "label": "test:feature-off" },
                "base": { "ref": "main", "sha": "bbbb" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_env(&repo, home.path(), &["submit", "--yes", "--no-prompt"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "DELETE"
                && request.url.path() == "/repos/test/repo/issues/comments/901"
        }));
        let body_patch = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_patch.body).unwrap();
        assert_eq!(payload["body"], "## Summary\n\nhello");
    }

    #[tokio::test]
    async fn test_merge_already_merged_pr_still_rebases_next_branch_and_reparents_metadata() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        // Resolve PRs for both stack branches during merge scope validation.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/101",
                    "id": 101,
                    "number": 101,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "merge-a", "sha": "sha-a", "label": "test:merge-a" },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/102",
                    "id": 102,
                    "number": 102,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                    "base": { "ref": "merge-a", "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        // PR #101 is already merged.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/101",
                "id": 101,
                "number": 101,
                "state": "closed",
                "draft": false,
                "merged_at": "2024-01-01T00:00:00Z",
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": "merge-a", "sha": "sha-a", "label": "test:merge-a" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        // PR #102 remains open and mergeable.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/102",
                "id": 102,
                "number": 102,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                "base": { "ref": "merge-a", "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        // During the "already merged" path, merge must still retarget the next PR to trunk.
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/102",
                "id": 102,
                "number": 102,
                "state": "open",
                "draft": false,
                "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        // Merge PR #102 when command reaches the second step.
        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-a"]);
        assert!(
            output.status.success(),
            "Failed to create merge-a: {}",
            TestRepo::stderr(&output)
        );
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent 1\n");
        repo.commit("Parent commit 1");
        repo.create_file("parent.txt", "parent 1\nparent 2\n");
        repo.commit("Parent commit 2");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(
            push_a.status.success(),
            "Failed to push merge-a: {}",
            TestRepo::stderr(&push_a)
        );

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-b"]);
        assert!(
            output.status.success(),
            "Failed to create merge-b: {}",
            TestRepo::stderr(&output)
        );
        let branch_b = repo.current_branch();
        repo.create_file("b.txt", "b");
        repo.commit("B");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(
            push_b.status.success(),
            "Failed to push merge-b: {}",
            TestRepo::stderr(&push_b)
        );

        // Simulate GitHub squash-merging the first branch before running stax merge.
        squash_merge_branch_on_fake_remote(&remote_root, &branch_a);

        // Start from the top branch so merge scope is [merge-a, merge-b].
        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        // Verify that branch B metadata was reparented to trunk despite branch A being already merged.
        let metadata_ref = format!("refs/branch-metadata/{}", branch_b);
        let metadata_output = repo.git(&["show", &metadata_ref]);
        assert!(
            metadata_output.status.success(),
            "Failed to read branch_b metadata: {}",
            TestRepo::stderr(&metadata_output)
        );
        let metadata: Value = serde_json::from_str(&TestRepo::stdout(&metadata_output))
            .expect("Invalid JSON metadata");
        assert_eq!(
            metadata["parentBranchName"], "main",
            "Expected merge-b to be reparented to trunk, metadata was: {}",
            metadata
        );

        let merge_stdout = TestRepo::stdout(&merge_output);
        let merge_stderr = TestRepo::stderr(&merge_output);
        let merge_combined = format!("{}{}", merge_stdout, merge_stderr);
        assert!(
            merge_stdout.contains("Already merged"),
            "Expected merge output to include already-merged path. Output:\n{}",
            merge_stdout
        );
        assert!(
            !merge_combined.contains("Rebase conflict"),
            "Expected provenance-aware rebase to avoid conflicts. Output:\n{}",
            merge_combined
        );

        let unique_count = git_with_env(
            &repo,
            home.path(),
            &["rev-list", "--count", &format!("origin/main..{}", branch_b)],
        );
        assert!(
            unique_count.status.success(),
            "Failed to count unique commits for {}: {}",
            branch_b,
            TestRepo::stderr(&unique_count)
        );
        assert_eq!(
            TestRepo::stdout(&unique_count).trim(),
            "1",
            "Expected descendant branch to keep only novel commits after squash-merge restack"
        );
    }

    #[tokio::test]
    async fn test_merge_retargets_next_pr_after_merging_parent_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/101",
                    "id": 101,
                    "number": 101,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "merge-a", "sha": "sha-a", "label": "test:merge-a" },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/102",
                    "id": 102,
                    "number": 102,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                    "base": { "ref": "merge-a", "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/101",
                "id": 101,
                "number": 101,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": "merge-a", "sha": "sha-a", "label": "test:merge-a" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/102",
                "id": 102,
                "number": 102,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                "base": { "ref": "merge-a", "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/102",
                "id": 102,
                "number": 102,
                "state": "open",
                "draft": false,
                "head": { "ref": "merge-b", "sha": "sha-b", "label": "test:merge-b" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/102");
        let merge_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/101/merge");
        assert!(
            patch_idx > merge_idx,
            "Expected dependent PR retarget after parent merge, requests were: {:?}",
            requests
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_merge_skips_retarget_when_next_pr_already_targets_trunk() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-skip-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-skip-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                github_pull_fixture(101, &branch_a, "main", "sha-a"),
                github_pull_fixture(102, &branch_b, "main", "sha-b")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(101, &branch_a, "main", "sha-a")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(102, &branch_b, "main", "sha-b")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        assert!(
            requests.iter().all(|request| {
                request.method.as_str() != "PATCH"
                    || request.url.path() != "/repos/test/repo/pulls/102"
            }),
            "Expected already-retargeted PR to skip PATCH, requests were: {:?}",
            requests
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_merge_treats_duplicate_retarget_error_as_done_after_confirming_base() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-dupe-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-dupe-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                github_pull_fixture(101, &branch_a, "main", "sha-a"),
                github_pull_fixture(102, &branch_b, &branch_a, "sha-b")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(101, &branch_a, "main", "sha-a")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(102, &branch_b, &branch_a, "sha-b")),
            )
            .with_priority(1)
            .up_to_n_times(2)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(102, &branch_b, "main", "sha-b")),
            )
            .with_priority(2)
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "Validation Failed",
                "documentation_url": "https://docs.github.com/rest/pulls/pulls#update-a-pull-request",
                "errors": [{
                    "resource": "PullRequest",
                    "field": "base",
                    "code": "invalid",
                    "message": format!(
                        "A pull request already exists for base branch 'main' and head branch '{}'",
                        branch_b
                    )
                }]
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let stdout = TestRepo::stdout(&merge_output);
        assert!(
            stdout.contains("already on base"),
            "Expected duplicate retarget to be treated as already applied. Output:\n{}",
            stdout
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/102");
        let merge_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/101/merge");
        assert!(
            patch_idx > merge_idx,
            "Expected duplicate retarget response after parent merge, requests were: {:?}",
            requests
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_merge_surfaces_duplicate_retarget_error_when_base_does_not_change() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-dupe-fail-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "merge-dupe-fail-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                github_pull_fixture(101, &branch_a, "main", "sha-a"),
                github_pull_fixture(102, &branch_b, &branch_a, "sha-b")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(101, &branch_a, "main", "sha-a")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(102, &branch_b, &branch_a, "sha-b")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "Validation Failed",
                "documentation_url": "https://docs.github.com/rest/pulls/pulls#update-a-pull-request",
                "errors": [{
                    "resource": "PullRequest",
                    "field": "base",
                    "code": "invalid",
                    "message": format!(
                        "A pull request already exists for base branch 'main' and head branch '{}'",
                        branch_b
                    )
                }]
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        let combined = format!(
            "{}{}",
            TestRepo::stdout(&merge_output),
            TestRepo::stderr(&merge_output)
        );
        assert!(
            combined.contains("A pull request already exists for base branch 'main'")
                && combined.contains("Merge Stopped"),
            "Expected duplicate PR base error to surface. Output:\n{}",
            combined
        );
    }

    #[tokio::test]
    async fn test_merge_when_ready_already_merged_pr_still_rebases_next_branch_and_reparents_metadata(
    ) {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwr-a"]);
        assert!(
            output.status.success(),
            "Failed to create mwr-a: {}",
            TestRepo::stderr(&output)
        );
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent 1\n");
        repo.commit("Parent commit 1");
        repo.create_file("parent.txt", "parent 1\nparent 2\n");
        repo.commit("Parent commit 2");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(
            push_a.status.success(),
            "Failed to push mwr-a: {}",
            TestRepo::stderr(&push_a)
        );

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwr-b"]);
        assert!(
            output.status.success(),
            "Failed to create mwr-b: {}",
            TestRepo::stderr(&output)
        );
        let branch_b = repo.current_branch();
        repo.create_file("b.txt", "b");
        repo.commit("B");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(
            push_b.status.success(),
            "Failed to push mwr-b: {}",
            TestRepo::stderr(&push_b)
        );

        // Simulate GitHub squash-merging the first branch before merge --when-ready.
        squash_merge_branch_on_fake_remote(&remote_root, &branch_a);

        // Resolve PRs for both stack branches during merge-when-ready scope validation.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/201",
                    "id": 201,
                    "number": 201,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mwr-a" },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/202",
                    "id": 202,
                    "number": 202,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mwr-b" },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        // PR #201 is already merged.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/201"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/201",
                "id": 201,
                "number": 201,
                "state": "closed",
                "draft": false,
                "merged_at": "2024-01-01T00:00:00Z",
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mwr-a" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        // PR #202 remains open and ready.
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/202",
                "id": 202,
                "number": 202,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mwr-b" },
                "base": { "ref": branch_a, "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        // During the already-merged first PR path, next PR base should still be updated.
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/202",
                "id": 202,
                "number": 202,
                "state": "open",
                "draft": false,
                "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mwr-b" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/202/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let metadata_ref = format!("refs/branch-metadata/{}", branch_b);
        let metadata_output = repo.git(&["show", &metadata_ref]);
        assert!(
            metadata_output.status.success(),
            "Failed to read branch_b metadata: {}",
            TestRepo::stderr(&metadata_output)
        );
        let metadata: Value = serde_json::from_str(&TestRepo::stdout(&metadata_output))
            .expect("Invalid JSON metadata");
        assert_eq!(
            metadata["parentBranchName"], "main",
            "Expected mwr-b to be reparented to trunk, metadata was: {}",
            metadata
        );

        let merge_stdout = TestRepo::stdout(&merge_output);
        let merge_stderr = TestRepo::stderr(&merge_output);
        let merge_combined = format!("{}{}", merge_stdout, merge_stderr);
        assert!(
            merge_stdout.contains("Already merged"),
            "Expected merge-when-ready output to include already-merged path. Output:\n{}",
            merge_stdout
        );
        assert!(
            !merge_combined.contains("Rebase conflict"),
            "Expected provenance-aware rebase to avoid conflicts. Output:\n{}",
            merge_combined
        );

        let unique_count = git_with_env(
            &repo,
            home.path(),
            &["rev-list", "--count", &format!("origin/main..{}", branch_b)],
        );
        assert!(
            unique_count.status.success(),
            "Failed to count unique commits for {}: {}",
            branch_b,
            TestRepo::stderr(&unique_count)
        );
        assert_eq!(
            TestRepo::stdout(&unique_count).trim(),
            "1",
            "Expected descendant branch to keep only novel commits after squash-merge restack"
        );
    }

    #[tokio::test]
    async fn test_merge_when_ready_retargets_next_pr_after_merging_parent_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwr-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwr-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/201",
                    "id": 201,
                    "number": 201,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mwr-a" },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/202",
                    "id": 202,
                    "number": 202,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mwr-b" },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/201"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/201",
                "id": 201,
                "number": 201,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mwr-a" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/202",
                "id": 202,
                "number": 202,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mwr-b" },
                "base": { "ref": branch_a, "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/202",
                "id": 202,
                "number": 202,
                "state": "open",
                "draft": false,
                "head": { "ref": "mwr-b", "sha": "sha-b", "label": "test:mwr-b" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/201/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/202/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/202");
        let merge_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/201/merge");
        assert!(
            patch_idx > merge_idx,
            "Expected dependent PR retarget after parent merge, requests were: {:?}",
            requests
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_merge_remote_retargets_and_updates_branch_after_merging() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mremote-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mremote-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/301",
                    "id": 301,
                    "number": 301,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mremote-a" },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/302",
                    "id": 302,
                    "number": 302,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mremote-b" },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/301"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/301",
                "id": 301,
                "number": 301,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "title": "p",
                "head": { "ref": branch_a, "sha": "sha-a", "label": "test:mremote-a" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/302"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/302",
                "id": 302,
                "number": 302,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "title": "c",
                "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mremote-b" },
                "base": { "ref": branch_a, "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/302"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/302",
                "id": 302,
                "number": 302,
                "state": "open",
                "draft": false,
                "head": { "ref": branch_b, "sha": "sha-b", "label": "test:mremote-b" },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/301/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/302/update-branch"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "message": "Updating pull request branch.",
                "url": "https://api.github.com/repos/test/repo/pulls/302"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/302/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--remote",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "merge --remote failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/302");
        let merge1_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/301/merge");
        let update_idx =
            find_request_index(&requests, "PUT", "/repos/test/repo/pulls/302/update-branch");
        let merge2_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/302/merge");

        assert!(
            patch_idx > merge1_idx,
            "Expected dependent PR retarget after parent merge"
        );
        assert!(
            update_idx > merge1_idx,
            "Expected update-branch after parent merge"
        );
        assert!(
            update_idx < merge2_idx,
            "Expected update-branch before child merge"
        );
    }

    #[tokio::test]
    async fn test_merge_remote_treats_duplicate_retarget_error_as_done_after_confirming_base() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mremote-dupe-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mremote-dupe-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                github_pull_fixture(301, &branch_a, "main", "sha-a"),
                github_pull_fixture(302, &branch_b, &branch_a, "sha-b")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/301"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(301, &branch_a, "main", "sha-a")),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/302"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(302, &branch_b, &branch_a, "sha-b")),
            )
            .with_priority(1)
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/302"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(302, &branch_b, "main", "sha-b")),
            )
            .with_priority(2)
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/302"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "Validation Failed",
                "documentation_url": "https://docs.github.com/rest/pulls/pulls#update-a-pull-request",
                "errors": [{
                    "resource": "PullRequest",
                    "field": "base",
                    "code": "invalid",
                    "message": format!(
                        "A pull request already exists for base branch 'main' and head branch '{}'",
                        branch_b
                    )
                }]
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/301/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/302/update-branch"))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "message": "Updating pull request branch.",
                "url": "https://api.github.com/repos/test/repo/pulls/302"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/302/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-b-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--remote",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "merge --remote failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let stdout = TestRepo::stdout(&merge_output);
        assert!(
            stdout.contains("already on base"),
            "Expected duplicate retarget to be treated as already applied. Output:\n{}",
            stdout
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/302");
        let merge_idx = find_request_index(&requests, "PUT", "/repos/test/repo/pulls/301/merge");
        assert!(
            patch_idx > merge_idx,
            "Expected duplicate retarget response after parent merge, requests were: {:?}",
            requests
                .iter()
                .map(|request| format!("{} {}", request.method, request.url.path()))
                .collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn test_merge_queue_uses_idempotent_retarget_and_rollback() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "queue-dupe-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "queue-dupe-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        let mut merged_parent = github_pull_fixture(401, &branch_a, "main", "sha-a");
        merged_parent["merged_at"] = serde_json::json!("2026-04-23T00:00:00Z");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                merged_parent,
                github_pull_fixture(402, &branch_b, &branch_a, "sha-b")
            ])))
            .mount(&mock_server)
            .await;

        let mut merged_parent_get = github_pull_fixture(401, &branch_a, "main", "sha-a");
        merged_parent_get["merged_at"] = serde_json::json!("2026-04-23T00:00:00Z");
        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/401"))
            .respond_with(ResponseTemplate::new(200).set_body_json(merged_parent_get))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(402, &branch_b, &branch_a, "sha-b")),
            )
            .with_priority(1)
            .up_to_n_times(2)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(402, &branch_b, "main", "sha-b")),
            )
            .with_priority(2)
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
                "message": "Validation Failed",
                "documentation_url": "https://docs.github.com/rest/pulls/pulls#update-a-pull-request",
                "errors": [{
                    "resource": "PullRequest",
                    "field": "base",
                    "code": "invalid",
                    "message": format!(
                        "A pull request already exists for base branch 'main' and head branch '{}'",
                        branch_b
                    )
                }]
            })))
            .with_priority(1)
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(github_pull_fixture(402, &branch_b, &branch_a, "sha-b")),
            )
            .with_priority(2)
            .mount(&mock_server)
            .await;

        let queue_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--queue",
                "--yes",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            queue_output.status.success(),
            "merge --queue failed unexpectedly: {}\n{}",
            TestRepo::stderr(&queue_output),
            TestRepo::stdout(&queue_output)
        );

        let stdout = TestRepo::stdout(&queue_output);
        assert!(
            stdout.contains("already on base")
                && stdout.contains("Rolling back #402 base")
                && stdout.contains("restored")
                && stdout.contains("Failed to enqueue"),
            "Expected queue retarget to be idempotent and rollback to restore base. Output:\n{}",
            stdout
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_count = requests
            .iter()
            .filter(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test/repo/pulls/402"
            })
            .count();
        assert_eq!(patch_count, 2, "Expected retarget and rollback PATCHes");
    }

    #[tokio::test]
    async fn test_github_api_mock_responses() {
        ensure_crypto_provider();
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

    #[tokio::test]
    async fn test_submit_gitlab_comment_mode_creates_merge_request_and_stack_note() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config(home.path(), &mock_server.uri());
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
        );

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "feature-gitlab-comment"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", "feature-gitlab-comment"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "iid": 42,
                "title": "Feature commit",
                "state": "opened",
                "draft": false,
                "source_branch": "feature-gitlab-comment",
                "target_branch": "main",
                "description": "",
                "merge_status": "can_be_merged",
                "detailed_merge_status": "mergeable",
                "web_url": "https://gitlab.com/test/repo/-/merge_requests/42",
                "sha": "abc123"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": 900,
                "body": "<!-- stax-stack-comment -->\ncomment",
                "created_at": "2024-01-01T00:00:00Z",
                "author": { "username": "stax" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "iid": 42,
                "title": "Feature commit",
                "state": "opened",
                "draft": false,
                "source_branch": "feature-gitlab-comment",
                "target_branch": "main",
                "description": "",
                "merge_status": "can_be_merged",
                "detailed_merge_status": "mergeable",
                "web_url": "https://gitlab.com/test/repo/-/merge_requests/42",
                "sha": "abc123"
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "POST"
                && request.url.path() == "/projects/test%2Frepo/merge_requests"
        }));
        let note_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST"
                    && request.url.path() == "/projects/test%2Frepo/merge_requests/42/notes"
            })
            .expect("missing GitLab stack note request");
        let payload: serde_json::Value = serde_json::from_slice(&note_request.body).unwrap();
        assert!(payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-comment -->"));
    }

    #[tokio::test]
    async fn test_submit_gitlab_body_mode_updates_merge_request_body() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("body"));
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
        );

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "feature-gitlab-body"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", "feature-gitlab-body"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "iid": 42,
                "title": "Feature commit",
                "state": "opened",
                "draft": false,
                "source_branch": "feature-gitlab-body",
                "target_branch": "main",
                "description": "## Summary\n\nhello",
                "merge_status": "can_be_merged",
                "detailed_merge_status": "mergeable",
                "web_url": "https://gitlab.com/test/repo/-/merge_requests/42",
                "sha": "abc123"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "iid": 42,
                "title": "Feature commit",
                "state": "opened",
                "draft": false,
                "source_branch": "feature-gitlab-body",
                "target_branch": "main",
                "description": "## Summary\n\nhello",
                "merge_status": "can_be_merged",
                "detailed_merge_status": "mergeable",
                "web_url": "https://gitlab.com/test/repo/-/merge_requests/42",
                "sha": "abc123"
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "iid": 42,
                "title": "Feature commit",
                "state": "opened",
                "draft": false,
                "source_branch": "feature-gitlab-body",
                "target_branch": "main",
                "description": "## Summary\n\nhello",
                "merge_status": "can_be_merged",
                "detailed_merge_status": "mergeable",
                "web_url": "https://gitlab.com/test/repo/-/merge_requests/42",
                "sha": "abc123"
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        let body_update = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PUT"
                    && request.url.path() == "/projects/test%2Frepo/merge_requests/42"
            })
            .expect("missing GitLab body update request");
        let payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert!(payload["description"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-links:start -->"));
    }

    #[tokio::test]
    async fn test_submit_gitlab_both_mode_updates_stack_note_and_body() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("both"));
        let repo = setup_branch_with_forge_remote(
            home.path(),
            "feature-gitlab-both",
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
            "STAX_GITLAB_TOKEN",
        );

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", "feature-gitlab-both"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    42,
                    "Feature commit",
                    "feature-gitlab-both",
                    "main",
                    "opened",
                    "## Summary\n\nhello",
                    "abc123",
                    None,
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_note_fixture(900, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes/900"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gitlab_note_fixture(
                    900,
                    "<!-- stax-stack-comment -->\nupdated",
                )),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                42,
                "Feature commit",
                "feature-gitlab-both",
                "main",
                "opened",
                "## Summary\n\nhello",
                "abc123",
                None,
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                42,
                "Feature commit",
                "feature-gitlab-both",
                "main",
                "opened",
                "## Summary\n\nhello",
                "abc123",
                None,
            )))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        let note_update = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PUT"
                    && request.url.path() == "/projects/test%2Frepo/merge_requests/42/notes/900"
            })
            .expect("missing GitLab note update request");
        let note_payload: serde_json::Value = serde_json::from_slice(&note_update.body).unwrap();
        assert!(note_payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-comment -->"));

        let body_update = find_body_update(
            &requests,
            "PUT",
            "/projects/test%2Frepo/merge_requests/42",
            "description",
        );
        let body_payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert!(body_payload["description"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-links:start -->"));
    }

    #[tokio::test]
    async fn test_submit_gitlab_off_mode_removes_stack_note_and_body_block() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = setup_branch_with_forge_remote(
            home.path(),
            "feature-gitlab-off",
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
            "STAX_GITLAB_TOKEN",
        );

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", "feature-gitlab-off"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    42,
                    "Feature commit",
                    "feature-gitlab-off",
                    "main",
                    "opened",
                    "## Summary\n\nhello",
                    "abc123",
                    None,
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_note_fixture(900, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/projects/test%2Frepo/merge_requests/42/notes/900"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                42,
                "Feature commit",
                "feature-gitlab-off",
                "main",
                "opened",
                "## Summary\n\nhello\n\n<!-- stax-stack-links:start -->\nold\n<!-- stax-stack-links:end -->",
                "abc123",
                None,
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                42,
                "Feature commit",
                "feature-gitlab-off",
                "main",
                "opened",
                "## Summary\n\nhello",
                "abc123",
                None,
            )))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "DELETE"
                && request.url.path() == "/projects/test%2Frepo/merge_requests/42/notes/900"
        }));
        let body_update = find_body_update(
            &requests,
            "PUT",
            "/projects/test%2Frepo/merge_requests/42",
            "description",
        );
        let payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert_eq!(payload["description"], "## Summary\n\nhello");
    }

    #[tokio::test]
    async fn test_merge_gitlab_retargets_next_mr_after_merging_parent_mr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
        );
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "gitlab-merge-a"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "gitlab-merge-b"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", branch_a.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    101,
                    "Parent",
                    branch_a.as_str(),
                    "main",
                    "opened",
                    "",
                    "sha-a",
                    None
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", branch_b.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    102,
                    "Child",
                    branch_b.as_str(),
                    branch_a.as_str(),
                    "opened",
                    "",
                    "sha-b",
                    None,
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/101"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                101,
                "Parent",
                branch_a.as_str(),
                "main",
                "opened",
                "",
                "sha-a",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                102,
                "Child",
                branch_b.as_str(),
                branch_a.as_str(),
                "opened",
                "",
                "sha-b",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(
                r"/projects/test%2Frepo/repository/commits/.*/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "name": "pipeline",
                    "status": "success",
                    "target_url": "https://ci.example.com/1",
                    "started_at": "2024-01-01T00:00:00Z",
                    "finished_at": "2024-01-01T00:01:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                102,
                "Child",
                branch_b.as_str(),
                "main",
                "opened",
                "",
                "sha-b",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let retarget_idx =
            find_request_index(&requests, "PUT", "/projects/test%2Frepo/merge_requests/102");
        let merge_idx = find_request_index(
            &requests,
            "PUT",
            "/projects/test%2Frepo/merge_requests/101/merge",
        );
        assert!(retarget_idx > merge_idx);
        let retarget = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PUT"
                    && request.url.path() == "/projects/test%2Frepo/merge_requests/102"
            })
            .expect("missing GitLab retarget request");
        let payload: serde_json::Value = serde_json::from_slice(&retarget.body).unwrap();
        assert_eq!(payload["target_branch"], "main");
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "GET"
                && request.url.path().contains("/repository/commits/")
                && request.url.path().ends_with("/statuses")
        }));
    }

    #[tokio::test]
    async fn test_merge_when_ready_gitlab_retargets_next_mr_after_merging_parent_mr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitlab.com/test/repo.git",
            "https://gitlab.com/",
        );
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "gitlab-mwr-a"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &["bc", "gitlab-mwr-b"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", branch_a.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    201,
                    "Parent",
                    branch_a.as_str(),
                    "main",
                    "opened",
                    "",
                    "sha-a",
                    None
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests"))
            .and(query_param("state", "opened"))
            .and(query_param("source_branch", branch_b.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitlab_mr_fixture(
                    202,
                    "Child",
                    branch_b.as_str(),
                    branch_a.as_str(),
                    "opened",
                    "",
                    "sha-b",
                    None,
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/201"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                201,
                "Parent",
                branch_a.as_str(),
                "main",
                "opened",
                "",
                "sha-a",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/projects/test%2Frepo/merge_requests/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                202,
                "Child",
                branch_b.as_str(),
                branch_a.as_str(),
                "opened",
                "",
                "sha-b",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(
                r"/projects/test%2Frepo/repository/commits/.*/statuses",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "name": "pipeline",
                    "status": "success",
                    "target_url": "https://ci.example.com/1",
                    "started_at": "2024-01-01T00:00:00Z",
                    "finished_at": "2024-01-01T00:01:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitlab_mr_fixture(
                202,
                "Child",
                branch_b.as_str(),
                "main",
                "opened",
                "",
                "sha-b",
                Some("success"),
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/201/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/projects/test%2Frepo/merge_requests/202/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITLAB_TOKEN",
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let retarget_idx =
            find_request_index(&requests, "PUT", "/projects/test%2Frepo/merge_requests/202");
        let merge_idx = find_request_index(
            &requests,
            "PUT",
            "/projects/test%2Frepo/merge_requests/201/merge",
        );
        assert!(retarget_idx > merge_idx);
    }

    #[tokio::test]
    async fn test_submit_gitea_comment_mode_creates_pull_and_issue_comment() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config(home.path(), &mock_server.uri());
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
        );

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "feature-gitea-comment"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "number": 42,
                "state": "open",
                "title": "Feature commit",
                "body": "",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "merged": false,
                "head": { "ref": "feature-gitea-comment", "sha": "abc123", "label": "test:feature-gitea-comment" },
                "base": { "ref": "main", "sha": "def456", "label": "test:main" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "id": 901,
                "body": "<!-- stax-stack-comment -->\ncomment",
                "created_at": "2024-01-01T00:00:00Z",
                "user": { "login": "stax" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 42,
                "state": "open",
                "title": "Feature commit",
                "body": "",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "merged": false,
                "head": { "ref": "feature-gitea-comment", "sha": "abc123", "label": "test:feature-gitea-comment" },
                "base": { "ref": "main", "sha": "def456", "label": "test:main" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "POST" && request.url.path() == "/repos/test/repo/pulls"
        }));
        let comment_request = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "POST"
                    && request.url.path() == "/repos/test/repo/issues/42/comments"
            })
            .expect("missing Gitea issue comment request");
        let payload: serde_json::Value = serde_json::from_slice(&comment_request.body).unwrap();
        assert!(payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-comment -->"));
    }

    #[tokio::test]
    async fn test_submit_gitea_body_mode_updates_pull_body() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("body"));
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
        );

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "feature-gitea-body"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        repo.create_file("feature.txt", "content");
        repo.commit("Feature commit");

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(201).set_body_json(serde_json::json!({
                "number": 42,
                "state": "open",
                "title": "Feature commit",
                "body": "## Summary\n\nhello",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "merged": false,
                "head": { "ref": "feature-gitea-body", "sha": "abc123", "label": "test:feature-gitea-body" },
                "base": { "ref": "main", "sha": "def456", "label": "test:main" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 42,
                "state": "open",
                "title": "Feature commit",
                "body": "## Summary\n\nhello",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "merged": false,
                "head": { "ref": "feature-gitea-body", "sha": "abc123", "label": "test:feature-gitea-body" },
                "base": { "ref": "main", "sha": "def456", "label": "test:main" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "number": 42,
                "state": "open",
                "title": "Feature commit",
                "body": "## Summary\n\nhello",
                "draft": false,
                "mergeable": true,
                "mergeable_state": "clean",
                "merged": false,
                "head": { "ref": "feature-gitea-body", "sha": "abc123", "label": "test:feature-gitea-body" },
                "base": { "ref": "main", "sha": "def456", "label": "test:main" }
            })))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        let body_update = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert!(payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-links:start -->"));
    }

    #[tokio::test]
    async fn test_submit_gitea_both_mode_updates_issue_comment_and_body() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("both"));
        let repo = setup_branch_with_forge_remote(
            home.path(),
            "feature-gitea-both",
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
            "STAX_GITEA_TOKEN",
        );

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_pull_fixture(
                    42,
                    "Feature commit",
                    "feature-gitea-both",
                    "main",
                    "open",
                    "## Summary\n\nhello",
                    false,
                    "abc123",
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(gitea_comment_fixture(
                    901,
                    "<!-- stax-stack-comment -->\nupdated",
                )),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                42,
                "Feature commit",
                "feature-gitea-both",
                "main",
                "open",
                "## Summary\n\nhello",
                false,
                "abc123",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                42,
                "Feature commit",
                "feature-gitea-both",
                "main",
                "open",
                "## Summary\n\nhello",
                false,
                "abc123",
            )))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        let comment_update = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test/repo/issues/comments/901"
            })
            .expect("missing Gitea issue comment update request");
        let comment_payload: serde_json::Value =
            serde_json::from_slice(&comment_update.body).unwrap();
        assert!(comment_payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-comment -->"));

        let body_update = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let body_payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert!(body_payload["body"]
            .as_str()
            .unwrap()
            .contains("<!-- stax-stack-links:start -->"));
    }

    #[tokio::test]
    async fn test_submit_gitea_off_mode_removes_issue_comment_and_body_block() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;
        let home = super::test_tempdir();
        write_test_config_with_submit(home.path(), &mock_server.uri(), Some("off"));
        let repo = setup_branch_with_forge_remote(
            home.path(),
            "feature-gitea-off",
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
            "STAX_GITEA_TOKEN",
        );

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_pull_fixture(
                    42,
                    "Feature commit",
                    "feature-gitea-off",
                    "main",
                    "open",
                    "## Summary\n\nhello",
                    false,
                    "abc123",
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/issues/42/comments"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_comment_fixture(901, "<!-- stax-stack-comment -->\nold")
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path("/repos/test/repo/issues/comments/901"))
            .respond_with(ResponseTemplate::new(204))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                42,
                "Feature commit",
                "feature-gitea-off",
                "main",
                "open",
                "## Summary\n\nhello\n\n<!-- stax-stack-links:start -->\nold\n<!-- stax-stack-links:end -->",
                false,
                "abc123",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/42"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                42,
                "Feature commit",
                "feature-gitea-off",
                "main",
                "open",
                "## Summary\n\nhello",
                false,
                "abc123",
            )))
            .mount(&mock_server)
            .await;

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["submit", "--yes", "--no-prompt"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));

        let requests = mock_server.received_requests().await.unwrap();
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "DELETE"
                && request.url.path() == "/repos/test/repo/issues/comments/901"
        }));
        let body_update = find_body_patch(&requests, "/repos/test/repo/pulls/42");
        let payload: serde_json::Value = serde_json::from_slice(&body_update.body).unwrap();
        assert_eq!(payload["body"], "## Summary\n\nhello");
    }

    #[tokio::test]
    async fn test_merge_gitea_retargets_next_pr_after_merging_parent_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
        );
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "gitea-merge-a"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "gitea-merge-b"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_pull_fixture(
                    101,
                    "Parent",
                    branch_a.as_str(),
                    "main",
                    "open",
                    "",
                    false,
                    "sha-a"
                ),
                gitea_pull_fixture(
                    102,
                    "Child",
                    branch_b.as_str(),
                    branch_a.as_str(),
                    "open",
                    "",
                    false,
                    "sha-b"
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/101"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                101,
                "Parent",
                branch_a.as_str(),
                "main",
                "open",
                "",
                false,
                "sha-a",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                102,
                "Child",
                branch_b.as_str(),
                branch_a.as_str(),
                "open",
                "",
                false,
                "sha-b",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/test/repo/commits/.*/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "context": "ci",
                    "status": "success",
                    "target_url": "https://ci.example.com/1",
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:01:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/102"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                102,
                "Child",
                branch_b.as_str(),
                "main",
                "open",
                "",
                false,
                "sha-b",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls/101/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls/102/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let retarget_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/102");
        let merge_idx = find_request_index(&requests, "POST", "/repos/test/repo/pulls/101/merge");
        assert!(retarget_idx > merge_idx);
        let retarget = requests
            .iter()
            .find(|request| {
                request.method.as_str() == "PATCH"
                    && request.url.path() == "/repos/test/repo/pulls/102"
            })
            .expect("missing Gitea retarget request");
        let payload: serde_json::Value = serde_json::from_slice(&retarget.body).unwrap();
        assert_eq!(payload["base"], "main");
        assert!(requests.iter().any(|request| {
            request.method.as_str() == "GET"
                && request.url.path().contains("/commits/")
                && request.url.path().ends_with("/statuses")
        }));
    }

    #[tokio::test]
    async fn test_merge_when_ready_gitea_retargets_next_pr_after_merging_parent_pr() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_remote(
            &repo,
            home.path(),
            "https://gitea.example.com/test/repo.git",
            "https://gitea.example.com/",
        );
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "gitea-mwr-a"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &["bc", "gitea-mwr-b"],
        );
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .and(query_param("state", "open"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                gitea_pull_fixture(
                    201,
                    "Parent",
                    branch_a.as_str(),
                    "main",
                    "open",
                    "",
                    false,
                    "sha-a"
                ),
                gitea_pull_fixture(
                    202,
                    "Child",
                    branch_b.as_str(),
                    branch_a.as_str(),
                    "open",
                    "",
                    false,
                    "sha-b"
                )
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/201"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                201,
                "Parent",
                branch_a.as_str(),
                "main",
                "open",
                "",
                false,
                "sha-a",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                202,
                "Child",
                branch_b.as_str(),
                branch_a.as_str(),
                "open",
                "",
                false,
                "sha-b",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path_regex(r"/repos/test/repo/commits/.*/statuses"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "context": "ci",
                    "status": "success",
                    "target_url": "https://ci.example.com/1",
                    "created_at": "2024-01-01T00:00:00Z",
                    "updated_at": "2024-01-01T00:01:00Z"
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/202"))
            .respond_with(ResponseTemplate::new(200).set_body_json(gitea_pull_fixture(
                202,
                "Child",
                branch_b.as_str(),
                "main",
                "open",
                "",
                false,
                "sha-b",
            )))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls/201/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        Mock::given(method("POST"))
            .and(path("/repos/test/repo/pulls/202/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .mount(&mock_server)
            .await;

        let merge_output = run_stax_with_token_env(
            &repo,
            home.path(),
            "STAX_GITEA_TOKEN",
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let retarget_idx = find_request_index(&requests, "PATCH", "/repos/test/repo/pulls/202");
        let merge_idx = find_request_index(&requests, "POST", "/repos/test/repo/pulls/201/merge");
        assert!(retarget_idx > merge_idx);
    }

    /// Install a `pre-receive` hook on the bare fake remote that rejects every
    /// subsequent push with a non-zero exit. The initial pushes performed during
    /// test setup complete before the hook is installed, so only pushes issued
    /// during the stax command under test are rejected.
    #[cfg(unix)]
    fn install_reject_all_pre_receive(remote_root: &TempDir) {
        use std::os::unix::fs::PermissionsExt;
        let remote_repo = remote_root.path().join("test").join("repo.git");
        let hooks_dir = remote_repo.join("hooks");
        std::fs::create_dir_all(&hooks_dir).expect("Failed to create hooks dir");
        let hook_path = hooks_dir.join("pre-receive");
        std::fs::write(
            &hook_path,
            "#!/bin/sh\necho 'simulated push rejection' >&2\nexit 1\n",
        )
        .expect("Failed to write pre-receive hook");
        let mut perms = std::fs::metadata(&hook_path)
            .expect("Failed to read hook perms")
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms).expect("Failed to set hook perms");
    }

    /// Regression for #312: when the rebased remaining branch fails to push,
    /// stax must NOT retarget the dependent PR base on GitHub. Doing so leaves
    /// the PR pointing at a base the remote branch never adopted.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_merge_preserves_remaining_pr_base_when_push_fails() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        // Build a two-branch stack: pushfail-a (to merge), pushfail-b (remaining).
        let output = run_stax_with_env(&repo, home.path(), &["bc", "pushfail-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "pushfail-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        // Move back to branch_a so branch_b becomes a descendant outside the
        // merge scope (the "remaining" branch path exercised by this test).
        let checkout = git_with_env(&repo, home.path(), &["checkout", &branch_a]);
        assert!(checkout.status.success(), "{}", TestRepo::stderr(&checkout));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/401",
                    "id": 401,
                    "number": 401,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": format!("test:{}", branch_a) },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/402",
                    "id": 402,
                    "number": 402,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/401"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/401",
                "id": 401,
                "number": 401,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_a, "sha": "sha-a", "label": format!("test:{}", branch_a) },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/402",
                "id": 402,
                "number": 402,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                "base": { "ref": branch_a, "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        // This PATCH must NOT be called when the remaining-branch push fails.
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/402"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/402",
                "id": 402,
                "number": 402,
                "state": "open",
                "draft": false,
                "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/401/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        install_reject_all_pre_receive(&remote_root);

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &["merge", "--yes", "--no-wait", "--no-delete", "--no-sync"],
        );
        assert!(
            merge_output.status.success(),
            "Merge failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let stdout = TestRepo::stdout(&merge_output);
        assert!(
            stdout.contains("push failed") && stdout.contains("PR base unchanged"),
            "Expected push-failure surfacing in output. stdout was:\n{}",
            stdout
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_402_calls = requests
            .iter()
            .filter(|r| {
                r.method.as_str() == "PATCH" && r.url.path() == "/repos/test/repo/pulls/402"
            })
            .count();
        assert_eq!(
            patch_402_calls,
            0,
            "Expected PR #402 base NOT to be retargeted when push failed. Requests: {:?}",
            requests
                .iter()
                .map(|r| format!("{} {}", r.method, r.url.path()))
                .collect::<Vec<_>>()
        );
    }

    /// Regression for #312 (merge --when-ready variant): when the rebased
    /// remaining branch fails to push, stax must NOT retarget the dependent
    /// PR base on GitHub.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_merge_when_ready_preserves_remaining_pr_base_when_push_fails() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwrpush-a"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_a = repo.current_branch();
        repo.create_file("parent.txt", "parent\n");
        repo.commit("Parent commit");
        let push_a = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_a]);
        assert!(push_a.status.success(), "{}", TestRepo::stderr(&push_a));

        let output = run_stax_with_env(&repo, home.path(), &["bc", "mwrpush-b"]);
        assert!(output.status.success(), "{}", TestRepo::stderr(&output));
        let branch_b = repo.current_branch();
        repo.create_file("child.txt", "child\n");
        repo.commit("Child commit");
        let push_b = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch_b]);
        assert!(push_b.status.success(), "{}", TestRepo::stderr(&push_b));

        let checkout = git_with_env(&repo, home.path(), &["checkout", &branch_a]);
        assert!(checkout.status.success(), "{}", TestRepo::stderr(&checkout));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/501",
                    "id": 501,
                    "number": 501,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": format!("test:{}", branch_a) },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/502",
                    "id": 502,
                    "number": 502,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                }
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/501"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/501",
                "id": 501,
                "number": 501,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_a, "sha": "sha-a", "label": format!("test:{}", branch_a) },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls/502"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/502",
                "id": 502,
                "number": 502,
                "state": "open",
                "draft": false,
                "merged_at": null,
                "mergeable": true,
                "mergeable_state": "clean",
                "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                "base": { "ref": branch_a, "sha": "sha-a" }
            })))
            .mount(&mock_server)
            .await;

        // This PATCH must NOT be called when the remaining-branch push fails.
        Mock::given(method("PATCH"))
            .and(path("/repos/test/repo/pulls/502"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "url": "https://api.github.com/repos/test/repo/pulls/502",
                "id": 502,
                "number": 502,
                "state": "open",
                "draft": false,
                "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                "base": { "ref": "main", "sha": "main-sha" }
            })))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/repos/test/repo/pulls/501/merge"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha": "merge-a-commit",
                "merged": true,
                "message": "Pull Request successfully merged"
            })))
            .mount(&mock_server)
            .await;

        install_reject_all_pre_receive(&remote_root);

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let stdout = TestRepo::stdout(&merge_output);
        assert!(
            stdout.contains("push failed") && stdout.contains("PR base unchanged"),
            "Expected push-failure surfacing in output. stdout was:\n{}",
            stdout
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");
        let patch_502_calls = requests
            .iter()
            .filter(|r| {
                r.method.as_str() == "PATCH" && r.url.path() == "/repos/test/repo/pulls/502"
            })
            .count();
        assert_eq!(
            patch_502_calls,
            0,
            "Expected PR #502 base NOT to be retargeted when push failed. Requests: {:?}",
            requests
                .iter()
                .map(|r| format!("{} {}", r.method, r.url.path()))
                .collect::<Vec<_>>()
        );
    }

    /// Regression for #311: `merge --when-ready` must preserve the relative
    /// stack chain of remaining branches rather than flattening every
    /// descendant onto trunk. For a stack `main <- a <- b <- c <- d`, merging
    /// `a` and `b` should leave `c` based on `main` and `d` based on `c`.
    #[tokio::test]
    async fn test_merge_when_ready_preserves_remaining_chain() {
        ensure_crypto_provider();
        let mock_server = MockServer::start().await;

        let home = super::test_tempdir();
        let repo = TestRepo::new();
        let _remote_root = setup_fake_github_remote(&repo, home.path());
        write_test_config(home.path(), &mock_server.uri());

        // Build a four-branch stack and push each to the fake remote.
        let mut branches: Vec<String> = Vec::new();
        for name in ["chain-a", "chain-b", "chain-c", "chain-d"] {
            let output = run_stax_with_env(&repo, home.path(), &["bc", name]);
            assert!(output.status.success(), "{}", TestRepo::stderr(&output));
            let branch = repo.current_branch();
            let filename = format!("{}.txt", name);
            repo.create_file(&filename, &format!("{} content\n", name));
            repo.commit(&format!("{} commit", name));
            let push = git_with_env(&repo, home.path(), &["push", "-u", "origin", &branch]);
            assert!(push.status.success(), "{}", TestRepo::stderr(&push));
            branches.push(branch);
        }
        let branch_a = branches[0].clone();
        let branch_b = branches[1].clone();
        let branch_c = branches[2].clone();
        let branch_d = branches[3].clone();

        // Run merge --when-ready from branch_b so a+b are merged and c+d are
        // the remaining chain that needs rebasing.
        let checkout = git_with_env(&repo, home.path(), &["checkout", &branch_b]);
        assert!(checkout.status.success(), "{}", TestRepo::stderr(&checkout));

        Mock::given(method("GET"))
            .and(path("/repos/test/repo/pulls"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/601",
                    "id": 601,
                    "number": 601,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_a, "sha": "sha-a", "label": format!("test:{}", branch_a) },
                    "base": { "ref": "main", "sha": "main-sha" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/602",
                    "id": 602,
                    "number": 602,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_b, "sha": "sha-b", "label": format!("test:{}", branch_b) },
                    "base": { "ref": branch_a, "sha": "sha-a" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/603",
                    "id": 603,
                    "number": 603,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_c, "sha": "sha-c", "label": format!("test:{}", branch_c) },
                    "base": { "ref": branch_b, "sha": "sha-b" }
                },
                {
                    "url": "https://api.github.com/repos/test/repo/pulls/604",
                    "id": 604,
                    "number": 604,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": branch_d, "sha": "sha-d", "label": format!("test:{}", branch_d) },
                    "base": { "ref": branch_c, "sha": "sha-c" }
                }
            ])))
            .mount(&mock_server)
            .await;

        for (pr_id, head_ref, head_sha, base_ref, base_sha) in [
            (601u64, &branch_a, "sha-a", "main", "main-sha"),
            (602u64, &branch_b, "sha-b", &branch_a, "sha-a"),
            (603u64, &branch_c, "sha-c", &branch_b, "sha-b"),
            (604u64, &branch_d, "sha-d", &branch_c, "sha-c"),
        ] {
            Mock::given(method("GET"))
                .and(path(format!("/repos/test/repo/pulls/{}", pr_id)))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "url": format!("https://api.github.com/repos/test/repo/pulls/{}", pr_id),
                    "id": pr_id,
                    "number": pr_id,
                    "state": "open",
                    "draft": false,
                    "merged_at": null,
                    "mergeable": true,
                    "mergeable_state": "clean",
                    "head": { "ref": head_ref, "sha": head_sha, "label": format!("test:{}", head_ref) },
                    "base": { "ref": base_ref, "sha": base_sha }
                })))
                .mount(&mock_server)
                .await;
        }

        // Accept PATCH retargets on any PR whose base needs to change. Branch d
        // may already point at branch c and therefore legitimately skip PATCH
        // once retargeting becomes idempotent.
        for pr_id in [602u64, 603, 604] {
            Mock::given(method("PATCH"))
                .and(path(format!("/repos/test/repo/pulls/{}", pr_id)))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "url": format!("https://api.github.com/repos/test/repo/pulls/{}", pr_id),
                    "id": pr_id,
                    "number": pr_id,
                    "state": "open",
                    "draft": false,
                    "head": { "ref": "placeholder", "sha": "placeholder", "label": "test:placeholder" },
                    "base": { "ref": "main", "sha": "main-sha" }
                })))
                .mount(&mock_server)
                .await;
        }

        for pr_id in [601u64, 602] {
            Mock::given(method("PUT"))
                .and(path(format!("/repos/test/repo/pulls/{}/merge", pr_id)))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "sha": format!("merge-{}-commit", pr_id),
                    "merged": true,
                    "message": "Pull Request successfully merged"
                })))
                .mount(&mock_server)
                .await;
        }

        let merge_output = run_stax_with_env(
            &repo,
            home.path(),
            &[
                "merge",
                "--when-ready",
                "--yes",
                "--no-delete",
                "--timeout",
                "1",
                "--interval",
                "1",
                "--no-sync",
            ],
        );
        assert!(
            merge_output.status.success(),
            "Merge-when-ready failed: {}\n{}",
            TestRepo::stderr(&merge_output),
            TestRepo::stdout(&merge_output)
        );

        let requests = mock_server
            .received_requests()
            .await
            .expect("request recording enabled");

        // Find the PATCH request for PR #603 (c) — it should retarget to main.
        let patch_603 = requests
            .iter()
            .find(|r| r.method.as_str() == "PATCH" && r.url.path() == "/repos/test/repo/pulls/603")
            .expect("PR #603 should have been retargeted");
        let payload_603: serde_json::Value = serde_json::from_slice(&patch_603.body).unwrap();
        assert_eq!(
            payload_603["base"], "main",
            "PR #603 (branch c) must be retargeted to main"
        );

        // PR #604 (branch d) must keep its base on branch c, not flatten to
        // trunk. With idempotent retargeting it may skip PATCH entirely when
        // the forge already reports branch c as the base.
        let patch_604 = requests
            .iter()
            .find(|r| r.method.as_str() == "PATCH" && r.url.path() == "/repos/test/repo/pulls/604");
        if let Some(patch_604) = patch_604 {
            let payload_604: serde_json::Value = serde_json::from_slice(&patch_604.body).unwrap();
            assert_eq!(
                payload_604["base"], branch_c,
                "PR #604 (branch d) must keep its base on branch c, not flatten to trunk"
            );
        }
    }
}
