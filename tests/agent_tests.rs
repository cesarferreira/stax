mod common;

use common::{OutputAssertions, TestRepo};
use serde_json::Value;
use std::fs;

// ── helpers ─────────────────────────────────────────────────────────────────

/// Read `.git/stax/agent-worktrees.json` and return parsed entries (empty if missing).
fn read_registry(repo: &TestRepo) -> Vec<Value> {
    let path = repo
        .path()
        .join(".git")
        .join("stax")
        .join("agent-worktrees.json");
    if !path.exists() {
        return Vec::new();
    }
    let content = fs::read_to_string(&path).expect("Failed to read registry");
    serde_json::from_str::<Vec<Value>>(&content).expect("Registry JSON should parse")
}

fn registry_has_entry(repo: &TestRepo, name: &str) -> bool {
    read_registry(repo)
        .iter()
        .any(|e| e["name"].as_str() == Some(name))
}

fn registry_entry(repo: &TestRepo, name: &str) -> Option<Value> {
    read_registry(repo)
        .into_iter()
        .find(|e| e["name"].as_str() == Some(name))
}

// ── slugify (exercised via create) ───────────────────────────────────────────

#[test]
fn slugify_basic() {
    let repo = TestRepo::new();

    // "Add dark mode" → folder "add-dark-mode"
    let out = repo.run_stax(&["agent", "create", "Add dark mode"]);
    out.assert_success();

    let worktree_path = repo
        .path()
        .join(".stax")
        .join("trees")
        .join("add-dark-mode");
    assert!(
        worktree_path.exists(),
        "Expected worktree dir at .stax/trees/add-dark-mode"
    );
}

#[test]
fn slugify_strips_special_chars() {
    let repo = TestRepo::new();

    let out = repo.run_stax(&["agent", "create", "Fix: auth bug!"]);
    out.assert_success();

    let worktree_path = repo.path().join(".stax").join("trees").join("fix-auth-bug");
    assert!(
        worktree_path.exists(),
        "Expected worktree dir 'fix-auth-bug', special chars stripped"
    );
}

// ── create ───────────────────────────────────────────────────────────────────

#[test]
fn agent_create_basic() {
    let repo = TestRepo::new();

    let out = repo.run_stax(&["agent", "create", "add-auth"]);
    out.assert_success();

    // Worktree dir exists
    let worktree_path = repo.path().join(".stax").join("trees").join("add-auth");
    assert!(
        worktree_path.exists(),
        "Expected worktree at {}",
        worktree_path.display()
    );

    // Branch exists
    let branches = repo.list_branches();
    assert!(
        branches.iter().any(|b| b.contains("add-auth")),
        "Expected branch containing 'add-auth', got: {:?}",
        branches
    );

    // Registry has the entry
    assert!(
        registry_has_entry(&repo, "add-auth"),
        "Registry missing 'add-auth'"
    );

    // .gitignore contains the worktrees dir
    let gitignore = fs::read_to_string(repo.path().join(".gitignore")).unwrap_or_default();
    assert!(
        gitignore.contains(".stax/trees"),
        ".gitignore should contain .stax/trees, got:\n{}",
        gitignore
    );
}

#[test]
fn agent_create_with_stack_on() {
    let repo = TestRepo::new();

    // Create a base branch first
    repo.run_stax(&["create", "base-feature"]).assert_success();
    let base_branch = repo.current_branch();

    // Go back to main and create agent stacked on base-feature
    repo.run_stax(&["checkout", "main"]).assert_success();

    let out = repo.run_stax(&[
        "agent",
        "create",
        "child-feature",
        "--stack-on",
        &base_branch,
    ]);
    out.assert_success();

    // Worktree should exist
    let worktree_path = repo
        .path()
        .join(".stax")
        .join("trees")
        .join("child-feature");
    assert!(worktree_path.exists());

    // Registry entry should have a branch
    let entry =
        registry_entry(&repo, "child-feature").expect("Registry should have 'child-feature'");
    assert!(
        entry["branch"].as_str().is_some(),
        "Registry entry should have a branch field"
    );
}

#[test]
fn agent_create_duplicate_fails() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "duplicate-test"])
        .assert_success();

    // Second create with same title should fail
    let out = repo.run_stax(&["agent", "create", "duplicate-test"]);
    out.assert_failure();
}

#[test]
fn agent_create_empty_slug_fails() {
    let repo = TestRepo::new();

    // Title that slugifies to nothing but hyphens → empty slug
    let out = repo.run_stax(&["agent", "create", "!!! ###"]);
    out.assert_failure();
}

// ── list ─────────────────────────────────────────────────────────────────────

#[test]
fn agent_list_empty() {
    let repo = TestRepo::new();

    let out = repo.run_stax(&["agent", "list"]);
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("No agent") || stdout.contains("none"),
        "Expected empty-list message, got:\n{}",
        stdout
    );
}

#[test]
fn agent_list_shows_entries() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "alpha-feature"])
        .assert_success();
    repo.run_stax(&["agent", "create", "beta-feature"])
        .assert_success();

    let out = repo.run_stax(&["agent", "list"]);
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("alpha-feature"),
        "Expected 'alpha-feature' in list output"
    );
    assert!(
        stdout.contains("beta-feature"),
        "Expected 'beta-feature' in list output"
    );
}

// ── register ─────────────────────────────────────────────────────────────────

#[test]
fn agent_register_current_branch() {
    let repo = TestRepo::new();

    // Create a branch to register
    repo.run_stax(&["create", "reg-feature"]).assert_success();
    let branch = repo.current_branch();
    let slug = branch.split('/').next_back().unwrap_or(&branch).to_string();

    let out = repo.run_stax(&["agent", "register"]);
    out.assert_success();

    assert!(
        registry_has_entry(&repo, &slug),
        "Registry should contain slug '{}' after register, entries: {:?}",
        slug,
        read_registry(&repo)
    );
}

#[test]
fn agent_register_duplicate_fails() {
    let repo = TestRepo::new();

    repo.run_stax(&["create", "dup-reg"]).assert_success();
    repo.run_stax(&["agent", "register"]).assert_success();

    // Second register on same branch should fail
    let out = repo.run_stax(&["agent", "register"]);
    out.assert_failure();
}

// ── remove ───────────────────────────────────────────────────────────────────

#[test]
fn agent_remove_cleans_worktree_and_registry() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "to-remove"])
        .assert_success();

    let worktree_path = repo.path().join(".stax").join("trees").join("to-remove");
    assert!(
        worktree_path.exists(),
        "Worktree should exist before remove"
    );
    assert!(registry_has_entry(&repo, "to-remove"));

    let out = repo.run_stax(&["agent", "remove", "to-remove"]);
    out.assert_success();

    assert!(
        !worktree_path.exists(),
        "Worktree dir should be gone after remove"
    );
    assert!(
        !registry_has_entry(&repo, "to-remove"),
        "Registry should not have 'to-remove' after remove"
    );
}

#[test]
fn agent_remove_with_delete_branch() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "del-branch-test"])
        .assert_success();

    let entry = registry_entry(&repo, "del-branch-test").expect("Registry entry should exist");
    let branch_name = entry["branch"].as_str().unwrap().to_string();

    // Return to main so the branch is not currently checked out
    repo.run_stax(&["checkout", "main"]).assert_success();

    let out = repo.run_stax(&["agent", "remove", "del-branch-test", "--delete-branch"]);
    out.assert_success();

    let branches = repo.list_branches();
    assert!(
        !branches.contains(&branch_name),
        "Branch '{}' should be deleted after remove --delete-branch, branches: {:?}",
        branch_name,
        branches
    );
}

#[test]
fn agent_remove_unknown_name_fails() {
    let repo = TestRepo::new();

    let out = repo.run_stax(&["agent", "remove", "does-not-exist"]);
    out.assert_failure();
}

// ── prune ────────────────────────────────────────────────────────────────────

#[test]
fn agent_prune_removes_dead_entries() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "prune-target"])
        .assert_success();

    assert!(registry_has_entry(&repo, "prune-target"));

    let worktree_path = repo.path().join(".stax").join("trees").join("prune-target");

    // Manually remove just the dir (simulating external deletion)
    fs::remove_dir_all(&worktree_path).expect("Failed to manually remove worktree dir");

    let out = repo.run_stax(&["agent", "prune"]);
    out.assert_success();

    assert!(
        !registry_has_entry(&repo, "prune-target"),
        "Registry should not have dead entry after prune"
    );
    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.contains("Pruned") || stdout.contains("1"),
        "Output should mention pruning, got:\n{}",
        stdout
    );
}

#[test]
fn agent_prune_nothing_to_do() {
    let repo = TestRepo::new();

    repo.run_stax(&["agent", "create", "still-alive"])
        .assert_success();

    let out = repo.run_stax(&["agent", "prune"]);
    out.assert_success();

    let stdout = TestRepo::stdout(&out);
    assert!(
        stdout.to_lowercase().contains("nothing") || stdout.to_lowercase().contains("all"),
        "Expected 'nothing to prune' message, got:\n{}",
        stdout
    );

    assert!(
        registry_has_entry(&repo, "still-alive"),
        "Live worktree should NOT be pruned"
    );
}
